import { useEffect, useRef, useState } from "react";
import { Button } from "@fluentui/react-components";
import { ArrowDownToLine, RefreshCw } from "lucide-react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type Props = { version: string; dirty: boolean; busy?: boolean; onBusyChange?: (busy: boolean) => void };

export function UpdatesSection({ version, dirty, busy = false, onBusyChange }: Props) {
  const [phase, setPhase] = useState<"idle" | "checking" | "available" | "downloading" | "installed">("idle");
  const [message, setMessage] = useState("");
  const [percent, setPercent] = useState<number | null>(null);
  const update = useRef<Update | null>(null);
  const mounted = useRef(true);
  const working = phase === "checking" || phase === "downloading";
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; void update.current?.close().catch(() => {}); };
  }, []);

  async function checkForUpdates() {
    if (busy || working) return;
    setPhase("checking"); setMessage("Checking for updates…");
    onBusyChange?.(true);
    try {
      await update.current?.close();
      update.current = null;
      const result = await check({ timeout: 30000 });
      if (!mounted.current) { await result?.close(); return; }
      update.current = result;
      setPhase(result ? "available" : "idle");
      setMessage(result ? `Version ${result.version} is available.` : "Up to date");
    } catch {
      if (mounted.current) { setPhase("idle"); setMessage("Could not check for updates. Check your connection and try again."); }
    } finally { onBusyChange?.(false); }
  }

  async function install() {
    if (busy || working || dirty || !update.current) return;
    const alreadyInstalled = phase === "installed";
    setPhase("downloading"); setMessage(alreadyInstalled ? "Restarting…" : "Downloading update…");
    onBusyChange?.(true);
    let installed = alreadyInstalled;
    try {
      if (!installed) {
        let total = 0, received = 0;
        await update.current.downloadAndInstall((event) => {
          if (!mounted.current) return;
          if (event.event === "Started") total = event.data.contentLength ?? 0;
          if (event.event === "Progress") {
            received += event.data.chunkLength;
            const next = total ? Math.min(100, Math.floor(received * 100 / total)) : null;
            setPercent(next);
            setMessage(next === null ? "Downloading update…" : `Downloading update… ${next}%`);
          }
          if (event.event === "Finished") { setPercent(null); setMessage("Verifying and installing update…"); }
        }, { timeout: 900000 });
        installed = true;
      }
      setMessage("Restarting…");
      await relaunch();
    } catch {
      if (mounted.current) {
        setPhase(installed ? "installed" : "available"); setPercent(null);
        setMessage(installed ? "Update installed. Restart Spice Route to finish." : "The update could not be installed. Check your connection and free space, then try again.");
      }
    } finally { onBusyChange?.(false); }
  }

  return <section className="updates-section" aria-labelledby="updates-title">
    <div className="updates-copy"><h2 id="updates-title">Updates</h2><span>Spice Route {version}</span></div>
    <div className="updates-actions">
      {update.current && phase !== "checking"
        ? <Button className="primary-button" icon={<ArrowDownToLine size={16} />} disabled={busy || working || dirty} onClick={() => void install()}>{phase === "installed" ? "Restart" : "Install and restart"}</Button>
        : <Button icon={<RefreshCw size={16} className={phase === "checking" ? "spin" : undefined} />} disabled={busy || working} onClick={() => void checkForUpdates()}>Check for updates</Button>}
      {phase === "downloading" && <progress aria-label="Update download progress" max={100} value={percent ?? undefined} />}
      <p role="status">{dirty && phase === "available" ? "Save your settings before installing." : message}</p>
    </div>
  </section>;
}
