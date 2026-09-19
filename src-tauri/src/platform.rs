use crate::error::{Result, SpiceError};
use crate::models::{CloudCandidate, CloudProvider};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};

// Our own read-only `codex --version` process is not a writer. Take writer
// inventories outside that child's lifetime instead of ignoring external PIDs.
// Hold this only around version probes or one process inventory, never around
// capture, filesystem work, or a whole transfer.
static WRITER_OBSERVATION_GATE: Mutex<()> = Mutex::new(());

fn with_writer_observation<T>(action: impl FnOnce() -> T) -> T {
    let _guard = WRITER_OBSERVATION_GATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    action()
}

pub fn cloud_candidates() -> Vec<CloudCandidate> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    for (variable, label) in [
        ("OneDriveCommercial", "OneDrive for Business"),
        ("OneDriveConsumer", "OneDrive"),
        ("OneDrive", "OneDrive"),
    ] {
        if let Some(value) = env::var_os(variable) {
            add_candidate(
                &mut result,
                &mut seen,
                CloudProvider::OneDrive,
                PathBuf::from(value),
                label,
            );
        }
    }
    if let Some(home) = dirs::home_dir() {
        for (path, provider, label) in [
            (
                home.join("iCloudDrive"),
                CloudProvider::ICloud,
                "iCloud Drive",
            ),
            (
                home.join("My Drive"),
                CloudProvider::GoogleDrive,
                "Google Drive",
            ),
            (
                home.join("Google Drive"),
                CloudProvider::GoogleDrive,
                "Google Drive",
            ),
        ] {
            add_candidate(&mut result, &mut seen, provider, path, label);
        }
        #[cfg(target_os = "macos")]
        add_macos_cloud_candidates(&home, &mut result, &mut seen);
    }
    #[cfg(windows)]
    for letter in b'D'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        let my_drive = root.join("My Drive");
        add_candidate(
            &mut result,
            &mut seen,
            CloudProvider::GoogleDrive,
            my_drive,
            "Google Drive",
        );
    }
    result
}

#[cfg(any(target_os = "macos", test))]
fn add_macos_cloud_candidates(
    home: &Path,
    result: &mut Vec<CloudCandidate>,
    seen: &mut HashSet<String>,
) {
    add_candidate(
        result,
        seen,
        CloudProvider::ICloud,
        home.join("Library/Mobile Documents/com~apple~CloudDocs"),
        "iCloud Drive",
    );

    let cloud_storage = home.join("Library/CloudStorage");
    let Ok(entries) = std::fs::read_dir(cloud_storage) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_lowercase());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let folded = name.to_ascii_lowercase();
        let path = entry.path();
        if folded.starts_with("googledrive-") || folded == "google drive" {
            let my_drive = path.join("My Drive");
            add_candidate(
                result,
                seen,
                CloudProvider::GoogleDrive,
                if my_drive.is_dir() { my_drive } else { path },
                "Google Drive",
            );
        } else if folded.starts_with("onedrive-") || folded == "onedrive" {
            let account = name
                .split_once('-')
                .map(|(_, suffix)| suffix.trim())
                .filter(|suffix| !suffix.is_empty());
            let label = account
                .map(|account| format!("OneDrive - {account}"))
                .unwrap_or_else(|| "OneDrive".to_string());
            add_candidate(result, seen, CloudProvider::OneDrive, path, &label);
        }
    }
}

fn add_candidate(
    result: &mut Vec<CloudCandidate>,
    seen: &mut HashSet<String>,
    provider: CloudProvider,
    path: PathBuf,
    label: &str,
) {
    if !path.is_dir() {
        return;
    }
    let normalized = path.to_string_lossy().to_lowercase();
    if seen.insert(normalized) {
        result.push(CloudCandidate {
            provider,
            path: path.to_string_lossy().into_owned(),
            label: label.to_string(),
        });
    }
}

fn process_inventory() -> System {
    // Writer detection and runtime discovery only need names and executable paths.
    // Avoid reading every process's memory, environment, command line, and CPU data,
    // and avoid immediately repeating the refresh performed by this constructor.
    System::new_with_specifics(
        RefreshKind::nothing()
            .with_processes(ProcessRefreshKind::nothing().with_exe(UpdateKind::Always)),
    )
}

fn codex_processes() -> Vec<(sysinfo::Pid, String)> {
    with_writer_observation(|| {
        let system = process_inventory();
        system
            .processes()
            .iter()
            .filter_map(|(pid, process)| {
                let name = process.name().to_string_lossy().to_lowercase();
                let exe = process
                    .exe()
                    .map(|value| value.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                is_codex_process(&name, &exe).then_some((*pid, name))
            })
            .collect()
    })
}

fn is_codex_process(name: &str, executable: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let executable = executable.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "codex.exe" | "codex" | "codex-cli.exe" | "codex-cli"
    ) || executable.contains("\\openai\\codex\\")
        || executable.contains("/openai/codex/")
        || (matches!(name.as_str(), "chatgpt.exe" | "chatgpt")
            && (executable.contains("\\openai.codex_") || executable.contains("/openai.codex_")))
}

pub fn codex_running() -> bool {
    !codex_processes().is_empty()
}

pub fn assert_codex_closed() -> Result<()> {
    if codex_running() {
        Err(SpiceError::CodexRunning)
    } else {
        Ok(())
    }
}

/// Watch for new Codex writers while file I/O runs. One inventory per interval
/// replaces one inventory per project file, and cancellation reaches chunked I/O.
pub(crate) struct WriterMonitor {
    reopened: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    stop: mpsc::Sender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

impl WriterMonitor {
    pub(crate) fn start(cancel: Arc<AtomicBool>) -> Result<Self> {
        Self::start_with_checker(cancel, Duration::from_millis(100), codex_running)
    }

    fn start_with_checker(
        cancel: Arc<AtomicBool>,
        interval: Duration,
        mut writer_running: impl FnMut() -> bool + Send + 'static,
    ) -> Result<Self> {
        let reopened = Arc::new(AtomicBool::new(false));
        let worker_reopened = reopened.clone();
        let worker_cancel = cancel.clone();
        let (stop, receiver) = mpsc::channel();
        let (first_poll, ready) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("spice-route-writer-monitor".into())
            .spawn(move || {
                let mut first_poll = Some(first_poll);
                loop {
                    let running = writer_running();
                    if running {
                        worker_reopened.store(true, Ordering::SeqCst);
                        worker_cancel.store(true, Ordering::SeqCst);
                    }
                    if let Some(first_poll) = first_poll.take() {
                        let _ = first_poll.send(());
                    }
                    if running {
                        break;
                    }
                    match receiver.recv_timeout(interval) {
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            })?;
        let monitor = Self {
            reopened,
            cancel,
            stop,
            worker: Some(worker),
        };
        // Do not start capture or restore while the monitor is still waiting
        // for its first scheduled inventory. Drop joins on every failed start.
        ready.recv().map_err(|_| {
            SpiceError::User(
                "The Codex writer monitor could not start. Try the handoff again.".into(),
            )
        })?;
        monitor.check()?;
        Ok(monitor)
    }

    pub(crate) fn check(&self) -> Result<()> {
        if self.reopened.load(Ordering::SeqCst) {
            Err(SpiceError::CodexReopened)
        } else if self.cancel.load(Ordering::SeqCst) {
            Err(SpiceError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(crate) fn explain_error(&self, error: SpiceError) -> SpiceError {
        match error {
            SpiceError::CodexRunning => {
                // A synchronous database guard can notice the writer before the
                // next background poll. It is the same latched stop condition.
                self.reopened.store(true, Ordering::SeqCst);
                self.cancel.store(true, Ordering::SeqCst);
                SpiceError::CodexReopened
            }
            SpiceError::Cancelled if self.reopened.load(Ordering::SeqCst) => {
                SpiceError::CodexReopened
            }
            other => other,
        }
    }
}

impl Drop for WriterMonitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn request_codex_close() -> Result<bool> {
    let processes = codex_processes();
    if processes.is_empty() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::collections::HashSet as StdHashSet;
        use windows_sys::core::BOOL;
        use windows_sys::Win32::Foundation::{HWND, LPARAM};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
        };
        unsafe extern "system" fn close_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let ids = &*(lparam as *const StdHashSet<u32>);
            let mut pid = 0_u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if ids.contains(&pid) && IsWindowVisible(hwnd) != 0 {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
            1
        }
        let ids: StdHashSet<u32> = processes.iter().map(|(pid, _)| pid.as_u32()).collect();
        unsafe {
            EnumWindows(Some(close_window), &ids as *const _ as LPARAM);
        }
    }
    #[cfg(target_os = "macos")]
    {
        // Ask the desktop application to terminate normally. CLI writers have no
        // application window, so the existing writer wait still protects them.
        let _ = hidden_command("osascript")
            .args(["-e", "tell application \"Codex\" to quit"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if !codex_running() {
            return Ok(true);
        }
        thread::sleep(Duration::from_millis(250));
    }
    Ok(false)
}

pub fn find_codex_executable() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CODEX_INSTALL_DIR")
        .map(PathBuf::from)
        .and_then(|path| codex_executable_in(&path))
    {
        return Some(path);
    }
    // Explorer-launched apps may inherit a PATH from before Codex was installed.
    // Prefer the installed runtime already serving Codex over an unrelated CLI on PATH.
    let system = process_inventory();
    if let Some(path) = system
        .processes()
        .values()
        .filter_map(|process| process.exe())
        .find(|path| is_bundled_codex_cli(path))
    {
        return Some(path.to_path_buf());
    }
    // Once Codex is closed for a transfer, still prefer its installed bundled runtime.
    // A separately installed CLI on PATH can be an older or different build.
    if let Some(path) = installed_codex_cli() {
        return Some(path);
    }
    let command = if cfg!(windows) { "where" } else { "which" };
    hidden_command(command)
        .arg("codex")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| {
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .map(PathBuf::from)
                .find(|path| path.is_file())
        })
}

fn is_bundled_codex_cli(path: &Path) -> bool {
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    (normalized.contains("/openai/codex/bin/")
        && (normalized.ends_with("/codex.exe") || normalized.ends_with("/codex")))
        || (normalized.contains("/codex.app/contents/resources/")
            && (normalized.ends_with("/codex")
                || normalized.ends_with("/bin/codex")
                || normalized.ends_with("/codex/codex")))
}

fn installed_codex_cli() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let root = PathBuf::from(env::var_os("LOCALAPPDATA")?).join("OpenAI/Codex/bin");
        let mut candidates: Vec<_> = std::fs::read_dir(root)
            .ok()?
            .flatten()
            .map(|entry| entry.path().join("codex.exe"))
            .filter(|path| path.is_file())
            .collect();
        candidates.sort_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .ok()
        });
        candidates.pop()
    }
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir()?;
        macos_installed_codex_candidates(&home)
            .into_iter()
            .find(|path| path.is_file())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_installed_codex_candidates(home: &Path) -> Vec<PathBuf> {
    let mut candidates = macos_codex_app_candidates(&home.join("Applications/Codex.app"));
    candidates.extend(macos_codex_app_candidates(Path::new(
        "/Applications/Codex.app",
    )));
    candidates.extend([
        home.join(".local/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
    ]);
    candidates
}

fn codex_executable_in(root: &Path) -> Option<PathBuf> {
    if root.is_file() {
        return Some(root.to_path_buf());
    }
    #[cfg(windows)]
    {
        [root.join("codex.exe"), root.join("bin/codex.exe")]
            .into_iter()
            .find(|path| path.is_file())
    }
    #[cfg(target_os = "macos")]
    {
        let mut candidates = vec![root.join("codex"), root.join("bin/codex")];
        if root.extension().is_some_and(|extension| extension == "app") {
            candidates.extend(macos_codex_app_candidates(root));
        } else {
            candidates.extend(macos_codex_app_candidates(&root.join("Codex.app")));
        }
        candidates.into_iter().find(|path| path.is_file())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        [root.join("codex"), root.join("bin/codex")]
            .into_iter()
            .find(|path| path.is_file())
    }
}

#[cfg(any(target_os = "macos", test))]
fn macos_codex_app_candidates(bundle: &Path) -> Vec<PathBuf> {
    [
        "Contents/Resources/codex",
        "Contents/Resources/bin/codex",
        "Contents/Resources/codex/codex",
    ]
    .into_iter()
    .map(|relative| bundle.join(relative))
    .collect()
}

pub fn codex_version(executable: Option<&Path>) -> Option<String> {
    executable
        .and_then(|path| {
            with_writer_observation(|| hidden_command(path).arg("--version").output().ok())
        })
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Run a console tool without creating a console window in the desktop app.
/// Callers still configure and capture stdin/stdout/stderr normally.
pub(crate) fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    command
}

pub fn open_codex() -> Result<()> {
    #[cfg(windows)]
    {
        hidden_command("explorer.exe")
            .arg(r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App")
            .spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        hidden_command("open").args(["-a", "Codex"]).spawn()?;
        return Ok(());
    }
    #[allow(unreachable_code)]
    Err(SpiceError::User(
        "Opening Codex is not implemented for this platform.".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_monitor_waits_for_owned_probe_and_still_detects_an_external_writer() {
        let probe_active = Arc::new(AtomicBool::new(false));
        let external_open = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let sampled_own_probe = Arc::new(AtomicBool::new(false));
        let (probe_started, probe_ready) = mpsc::channel();
        let (release_probe, probe_release) = mpsc::channel();
        let probe_flag = probe_active.clone();
        let probe = thread::spawn(move || {
            with_writer_observation(|| {
                probe_flag.store(true, Ordering::SeqCst);
                probe_started.send(()).unwrap();
                probe_release.recv_timeout(Duration::from_secs(5)).unwrap();
                probe_flag.store(false, Ordering::SeqCst);
            });
        });
        probe_ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let (start_requested, starting) = mpsc::channel();
        let (monitor_started, monitor_ready) = mpsc::channel();
        let worker_cancel = cancel.clone();
        let worker_probe_active = probe_active.clone();
        let worker_external = external_open.clone();
        let worker_sampled_probe = sampled_own_probe.clone();
        let factory = thread::spawn(move || {
            start_requested.send(()).unwrap();
            let monitor = WriterMonitor::start_with_checker(
                worker_cancel,
                Duration::from_millis(1),
                move || {
                    with_writer_observation(|| {
                        if worker_probe_active.load(Ordering::SeqCst) {
                            worker_sampled_probe.store(true, Ordering::SeqCst);
                        }
                        worker_external.load(Ordering::SeqCst)
                    })
                },
            );
            monitor_started.send(monitor).unwrap();
        });
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            monitor_ready.recv_timeout(Duration::from_millis(20)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        release_probe.send(()).unwrap();
        probe.join().unwrap();
        let monitor = monitor_ready
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        factory.join().unwrap();
        assert!(!sampled_own_probe.load(Ordering::SeqCst));
        monitor.check().unwrap();
        external_open.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancel.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(cancel.load(Ordering::SeqCst));
        assert!(matches!(monitor.check(), Err(SpiceError::CodexReopened)));
    }

    #[test]
    fn writer_monitor_latches_reopening_and_interrupts_chunk_cancellation() {
        let writer_open = Arc::new(AtomicBool::new(false));
        let worker_open = writer_open.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let monitor = WriterMonitor::start_with_checker(
            cancel.clone(),
            Duration::from_millis(1),
            move || worker_open.load(Ordering::SeqCst),
        )
        .unwrap();
        writer_open.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancel.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            cancel.load(Ordering::SeqCst),
            "existing chunked I/O must receive cancellation"
        );
        writer_open.store(false, Ordering::SeqCst);
        assert!(matches!(monitor.check(), Err(SpiceError::CodexReopened)));
        assert!(matches!(
            monitor.explain_error(SpiceError::Cancelled),
            SpiceError::CodexReopened
        ));
        // Closing the fake writer again does not clear its latched appearance.
        assert!(matches!(monitor.check(), Err(SpiceError::CodexReopened)));
    }

    #[test]
    fn writer_monitor_file_checks_do_not_enumerate_processes_and_shutdown_is_prompt() {
        use std::sync::atomic::AtomicUsize;
        let polls = Arc::new(AtomicUsize::new(0));
        let worker_polls = polls.clone();
        let (first_poll, ready) = mpsc::channel();
        let monitor = WriterMonitor::start_with_checker(
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(30),
            move || {
                worker_polls.fetch_add(1, Ordering::SeqCst);
                let _ = first_poll.send(());
                false
            },
        )
        .unwrap();
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..20_000 {
            monitor.check().unwrap();
        }
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        let started = Instant::now();
        drop(monitor);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn writer_monitor_preserves_user_cancellation_and_other_failures() {
        let cancel = Arc::new(AtomicBool::new(false));
        let monitor =
            WriterMonitor::start_with_checker(cancel.clone(), Duration::from_secs(30), || false)
                .unwrap();
        cancel.store(true, Ordering::SeqCst);
        assert!(matches!(monitor.check(), Err(SpiceError::Cancelled)));
        assert!(matches!(
            monitor.explain_error(SpiceError::Cancelled),
            SpiceError::Cancelled
        ));
        let error = monitor.explain_error(SpiceError::User("Recovery point preserved".into()));
        assert_eq!(error.to_string(), "Recovery point preserved");
    }

    #[test]
    fn writer_monitor_waits_for_initial_inventory_and_rejects_reopening_at_start() {
        let cancel = Arc::new(AtomicBool::new(false));
        let started = Instant::now();
        let result =
            WriterMonitor::start_with_checker(cancel.clone(), Duration::from_secs(30), || true);
        assert!(matches!(result, Err(SpiceError::CodexReopened)));
        assert!(cancel.load(Ordering::SeqCst));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn writer_monitor_failed_start_joins_without_waiting_for_the_next_poll() {
        let started = Instant::now();
        let result = WriterMonitor::start_with_checker(
            Arc::new(AtomicBool::new(true)),
            Duration::from_secs(30),
            || false,
        );
        assert!(matches!(result, Err(SpiceError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn synchronous_database_guard_latches_reopening_before_the_next_poll() {
        let cancel = Arc::new(AtomicBool::new(false));
        let monitor =
            WriterMonitor::start_with_checker(cancel.clone(), Duration::from_secs(30), || false)
                .unwrap();
        assert!(matches!(
            monitor.explain_error(SpiceError::CodexRunning),
            SpiceError::CodexReopened
        ));
        assert!(cancel.load(Ordering::SeqCst));
        assert!(matches!(monitor.check(), Err(SpiceError::CodexReopened)));
    }

    #[cfg(windows)]
    #[test]
    fn hidden_tools_have_no_console_and_preserve_pipes_and_exit_status() {
        use std::io::Write;
        use std::process::Stdio;
        let mut child = hidden_command("powershell.exe")
            .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
            .arg(r#"Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class ConsoleProbe { [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow(); }'; if ([ConsoleProbe]::GetConsoleWindow() -ne [IntPtr]::Zero) { exit 9 }; [Console]::Out.Write([Console]::In.ReadLine()); [Console]::Error.Write('diagnostic'); exit 7"#)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"spice route\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(7));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "spice route");
        assert_eq!(String::from_utf8_lossy(&output.stderr), "diagnostic");
    }

    #[test]
    fn lightweight_inventory_retains_process_names_and_executable_paths() {
        let started = Instant::now();
        let inventory = process_inventory();
        let current = inventory
            .process(sysinfo::get_current_pid().unwrap())
            .unwrap();
        assert!(!current.name().is_empty());
        assert!(current.exe().is_some_and(|path| path.is_file()));
        eprintln!("Name/executable inventory took {:?}", started.elapsed());
    }

    #[test]
    fn identifies_bundled_cli_without_confusing_desktop_or_other_tools() {
        assert!(is_bundled_codex_cli(Path::new(
            r"C:\Users\Alex\AppData\Local\OpenAI\Codex\bin\abc\codex.exe"
        )));
        assert!(!is_bundled_codex_cli(Path::new(r"C:\tools\codex.exe")));
        assert!(!is_bundled_codex_cli(Path::new(
            r"C:\OpenAI\Codex\bin\abc\ChatGPT.exe"
        )));
        assert!(is_bundled_codex_cli(Path::new(
            "/Applications/Codex.app/Contents/Resources/codex"
        )));
        assert!(is_bundled_codex_cli(Path::new(
            "/Applications/Codex.app/Contents/Resources/bin/codex"
        )));
        assert!(!is_bundled_codex_cli(Path::new(
            "/Applications/Codex.app/Contents/MacOS/Codex"
        )));
    }

    #[test]
    fn mac_cloud_discovery_finds_file_provider_and_icloud_roots() {
        let home = tempfile::tempdir().unwrap();
        let cloud_storage = home.path().join("Library/CloudStorage");
        let google = cloud_storage.join("GoogleDrive-person@example.com/My Drive");
        let one_drive = cloud_storage.join("OneDrive-Personal");
        let icloud = home
            .path()
            .join("Library/Mobile Documents/com~apple~CloudDocs");
        std::fs::create_dir_all(&google).unwrap();
        std::fs::create_dir_all(&one_drive).unwrap();
        std::fs::create_dir_all(&icloud).unwrap();

        let mut result = Vec::new();
        let mut seen = HashSet::new();
        add_macos_cloud_candidates(home.path(), &mut result, &mut seen);

        assert_eq!(result.len(), 3);
        assert!(result.iter().any(|candidate| {
            matches!(&candidate.provider, CloudProvider::ICloud)
                && candidate.label == "iCloud Drive"
                && Path::new(&candidate.path) == icloud
        }));
        assert!(result.iter().any(|candidate| {
            matches!(&candidate.provider, CloudProvider::GoogleDrive)
                && candidate.label == "Google Drive"
                && Path::new(&candidate.path) == google
        }));
        assert!(result.iter().any(|candidate| {
            matches!(&candidate.provider, CloudProvider::OneDrive)
                && candidate.label == "OneDrive - Personal"
                && Path::new(&candidate.path) == one_drive
        }));
    }

    #[test]
    fn mac_app_candidate_order_prefers_the_direct_resource_cli() {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("Codex.app");
        let candidates = macos_codex_app_candidates(&bundle);
        assert_eq!(
            candidates,
            vec![
                bundle.join("Contents/Resources/codex"),
                bundle.join("Contents/Resources/bin/codex"),
                bundle.join("Contents/Resources/codex/codex"),
            ]
        );
    }

    #[test]
    fn mac_installed_runtime_prefers_the_desktop_bundle_to_standalone_clis() {
        let home = Path::new("/Users/example");
        let candidates = macos_installed_codex_candidates(home);
        assert_eq!(
            candidates.first(),
            Some(&home.join("Applications/Codex.app/Contents/Resources/codex"))
        );
        let system_bundle_end = candidates
            .iter()
            .position(|path| path == Path::new("/Applications/Codex.app/Contents/Resources/codex/codex"))
            .unwrap();
        let standalone_start = candidates
            .iter()
            .position(|path| path == &home.join(".local/bin/codex"))
            .unwrap();
        assert!(system_bundle_end < standalone_start);
    }

    #[test]
    fn configured_runtime_may_point_directly_to_an_executable() {
        let root = tempfile::tempdir().unwrap();
        let executable = root.path().join("codex-runtime");
        std::fs::write(&executable, b"fixture").unwrap();
        assert_eq!(codex_executable_in(&executable), Some(executable));
    }

    #[test]
    fn recognizes_desktop_package_and_cli_processes() {
        assert!(is_codex_process("codex.exe", r"C:\tools\codex.exe"));
        assert!(is_codex_process(
            "chatgpt.exe",
            r"C:\Program Files\WindowsApps\OpenAI.Codex_1.2.3_x64__2p2nqsd0c76g0\app\ChatGPT.exe"
        ));
        assert!(!is_codex_process(
            "chatgpt.exe",
            r"C:\Apps\Unrelated\ChatGPT.exe"
        ));
        assert!(!is_codex_process(
            "spice-route.exe",
            r"D:\Apps\spice-route.exe"
        ));
    }
}
