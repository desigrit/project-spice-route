use crate::error::{Result, SpiceError};
use crate::models::{CloudCandidate, CloudProvider};
use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};

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
        .map(|path| path.join("codex.exe"))
        .filter(|path| path.is_file())
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
    normalized.contains("/openai/codex/bin/") && normalized.ends_with("/codex.exe")
}

fn installed_codex_cli() -> Option<PathBuf> {
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

pub fn codex_version(executable: Option<&Path>) -> Option<String> {
    executable
        .and_then(|path| hidden_command(path).arg("--version").output().ok())
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
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
        Command::new("explorer.exe")
            .arg(r"shell:AppsFolder\OpenAI.Codex_2p2nqsd0c76g0!App")
            .spawn()?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").args(["-a", "Codex"]).spawn()?;
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
