import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { Button, Dropdown, Option, Switch, Menu, MenuTrigger, MenuPopover, MenuList, MenuItem } from "@fluentui/react-components";
import { AppTheme } from "./fluent-theme";
import { usePageConfirmation } from "./use-page-confirmation";
import boatMark from "./assets/boat-mark.png";
import { open, save as saveFile } from "@tauri-apps/plugin-dialog";
import {
  ArchiveRestore,
  ArrowLeft,
  ArrowDownToLine,
  ArrowUpFromLine,
  Check,
  ChevronRight,
  CircleAlert,
  Cloud,
  Download,
  FolderCode,
  Folder,
  MoreHorizontal,
  HardDrive,
  History,
  Laptop,
  LoaderCircle,
  Moon,
  RefreshCw,
  Route,
  Search,
  Settings,
  ShieldCheck,
  Sparkles,
  Sun,
  Trash2,
  X,
} from "lucide-react";
import { api, formatBytes, formatTime } from "./api";
import type {
  AppConfig,
  CloudCleanupPreview,
  ConflictResolution,
  ContentCatalog,
  DiagnosticsReport,
  EnvironmentDiscovery,
  OperationPreview,
  OperationProgress,
  Page,
  ProjectMode,
  ProjectSummary,
  RecoverySummary,
  RequiredMapping,
  SyncStatus,
} from "./types";

const navItems: Array<{ id: Page; label: string; icon: typeof Route }> = [
  { id: "overview", label: "Overview", icon: Route },
  { id: "selection", label: "What to sync", icon: FolderCode },
  { id: "recovery", label: "Recovery", icon: ArchiveRestore },
  { id: "settings", label: "Settings", icon: Settings },
];

const appVersion = "1.5.2";

type BusyState = { label: string; operationId?: string } | null;
type EstimateState = "pending" | "ready" | "unavailable";

export default function App() {
  const [page, setPage] = useState<Page>("overview");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [environment, setEnvironment] = useState<EnvironmentDiscovery | null>(null);
  const [catalog, setCatalog] = useState<ContentCatalog | null>(null);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const [recoveries, setRecoveries] = useState<RecoverySummary[]>([]);
  const refreshGeneration = useRef(0);
  const [catalogSource, setCatalogSource] = useState<{ generation: number; config: AppConfig } | null>(null);
  const [detailsGeneration, setDetailsGeneration] = useState<number | null>(null);
  const [recoveryGeneration, setRecoveryGeneration] = useState<number | null>(null);
  const detailsRequest = useRef<{ generation: number; promise: Promise<ContentCatalog> } | null>(null);
  const recoveryRequest = useRef<{ generation: number; promise: Promise<RecoverySummary[]> } | null>(null);
  const [detailsError, setDetailsError] = useState<string | null>(null);
  const [recoveryError, setRecoveryError] = useState<string | null>(null);
  const [busy, setBusy] = useState<BusyState>({ label: "Inspecting Codex…" });
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const confirmation = usePageConfirmation(page);
  const [preview, setPreview] = useState<OperationPreview | null>(null);
  const [cleanupPreview, setCleanupPreview] = useState<CloudCleanupPreview | null>(null);
  const [operationProgress, setOperationProgress] = useState<OperationProgress | null>(null);

  const refresh = useCallback(async (knownConfig?: AppConfig) => {
    const generation = ++refreshGeneration.current;
    setError(null);
    setDetailsError(null);
    setRecoveryError(null);
    try {
      const [nextEnvironment, nextConfig] = await Promise.all([
        api.discoverEnvironment(),
        knownConfig ? Promise.resolve(knownConfig) : api.loadConfig(),
      ]);
      if (generation !== refreshGeneration.current) return;
      setEnvironment(nextEnvironment);
      setConfig(nextConfig);
      if (nextConfig.onboardingComplete) {
        const [nextCatalog, nextStatus] = await Promise.all([
          api.listContentQuick(nextConfig),
          api.getSyncStatus(nextConfig),
        ]);
        if (generation !== refreshGeneration.current) return;
        setCatalog(nextCatalog);
        setSyncStatus(nextStatus);
        setCatalogSource({ generation, config: nextConfig });
      }
    } catch (cause) {
      if (generation === refreshGeneration.current) setError(toMessage(cause));
    } finally {
      if (generation === refreshGeneration.current) setBusy(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
    return () => { refreshGeneration.current += 1; };
  }, [refresh]);

  // Workspace walks and rollback-size scans belong to the pages that need them.
  // Reuse in-flight reads if navigation returns before they finish, but never
  // apply results captured for an older refresh or an unmounted page.
  useEffect(() => {
    if (page !== "selection" || !catalogSource || detailsGeneration === catalogSource.generation) return;
    let disposed = false;
    const { generation, config: sourceConfig } = catalogSource;
    setDetailsError(null);
    if (detailsRequest.current?.generation !== generation) {
      detailsRequest.current = { generation, promise: api.listContent(sourceConfig) };
    }
    void detailsRequest.current.promise.then((nextCatalog) => {
      if (disposed || generation !== refreshGeneration.current) return;
      setCatalog(nextCatalog);
      setDetailsGeneration(generation);
    }).catch((cause) => {
      if (detailsRequest.current?.generation === generation) detailsRequest.current = null;
      if (!disposed && generation === refreshGeneration.current) setDetailsError(toMessage(cause));
    });
    return () => { disposed = true; };
  }, [page, catalogSource, detailsGeneration]);

  useEffect(() => {
    if (page !== "recovery" || !catalogSource || recoveryGeneration === catalogSource.generation) return;
    let disposed = false;
    const { generation } = catalogSource;
    setRecoveryError(null);
    if (recoveryRequest.current?.generation !== generation) {
      recoveryRequest.current = { generation, promise: api.listRecoveries() };
    }
    void recoveryRequest.current.promise.then((nextRecoveries) => {
      if (disposed || generation !== refreshGeneration.current) return;
      setRecoveries(nextRecoveries);
      setRecoveryGeneration(generation);
    }).catch((cause) => {
      if (recoveryRequest.current?.generation === generation) recoveryRequest.current = null;
      if (!disposed && generation === refreshGeneration.current) setRecoveryError(toMessage(cause));
    });
    return () => { disposed = true; };
  }, [page, catalogSource, recoveryGeneration]);

  useEffect(() => {
    const operationId = busy?.operationId;
    if (!operationId) {
      setOperationProgress(null);
      return;
    }
    let disposed = false;
    let reading = false;
    const read = async () => {
      if (disposed || reading) return;
      reading = true;
      try {
        const next = await api.getOperationProgress(operationId);
        if (!disposed && next) setOperationProgress(next);
      } catch {
        // Execution reports the actionable error; progress polling is advisory.
      } finally {
        reading = false;
      }
    };
    void read();
    const timer = window.setInterval(() => void read(), 450);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [busy?.operationId]);

  const save = async (next: AppConfig, message?: string) => {
    setBusy({ label: "Saving preferences…" });
    setError(null);
    try {
      const saved = await api.saveConfig(next);
      setConfig(saved);
      await refresh(saved);
      if (message) confirmation.show(message);
    } catch (cause) {
      setError(toMessage(cause));
      setBusy(null);
    }
  };

  const beginPreview = async (direction: "push" | "pull", snapshotId?: string) => {
    if (!config) return;
    setBusy({ label: direction === "push" ? "Comparing selected chats and project files…" : "Comparing incoming history with this device…" });
    setError(null);
    setNotice(null);
    try {
      const result = direction === "push" ? await api.previewPush(config) : await api.previewPull(config, snapshotId);
      setPreview(result);
    } catch (cause) {
      setError(toMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const execute = async (resolutions: ConflictResolution[]) => {
    if (!config || !preview) return;
    const activePreview = preview;
    setBusy({
      label: activePreview.direction === "push" ? "Publishing verified snapshot…" : "Applying verified snapshot…",
      operationId: activePreview.operationId,
    });
    setError(null);
    try {
      if (activePreview.requiresCodexClose) {
        const closed = await api.requestCodexClose();
        if (!closed) throw new Error("Codex is still running. Save your work, fully quit Codex, and try again.");
      }
      const result = activePreview.direction === "push"
        ? await api.executePush(config, activePreview.operationId)
        : await api.executePull(config, activePreview.operationId, resolutions);
      setPreview(null);
      setNotice(`${result.statusMessage} Handoff ID ${result.snapshot.shortId}.`);
      await refresh(config);
    } catch (cause) {
      await refresh(config);
      setError(toMessage(cause));
    }
  };

  const saveProjectMappings = async (mappings: Array<RequiredMapping & { destinationPath: string }>) => {
    if (!config) return;
    const selectedSnapshotId = preview?.snapshotId ?? undefined;
    const destinationRoots = { ...config.destinationRoots };
    const sourceRoots = { ...config.sourceRoots };
    for (const mapping of mappings) {
      if (mapping.rootIndex === 0) delete destinationRoots[mapping.projectId];
      destinationRoots[`${mapping.projectId}:${mapping.rootIndex}`] = mapping.destinationPath.trim();
      sourceRoots[`${mapping.projectId}:${mapping.rootIndex}`] = mapping.destinationPath.trim();
    }
    const next = { ...config, destinationRoots, sourceRoots };
    setPreview(null);
    setBusy({ label: "Saving project destinations…" });
    setError(null);
    try {
      const saved = await api.saveConfig(next);
      setConfig(saved);
      const refreshedPreview = await api.previewPull(saved, selectedSnapshotId);
      setPreview(refreshedPreview);
    } catch (cause) {
      setError(toMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const beginCloudCleanup = async () => {
    if (!config) return;
    setBusy({ label: "Inspecting cloud history…" });
    setError(null);
    try {
      setCleanupPreview(await api.previewCloudCleanup(config));
    } catch (cause) {
      setError(toMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  const executeCloudCleanup = async (confirmation: string) => {
    if (!config || !cleanupPreview) return;
    const active = cleanupPreview;
    setCleanupPreview(null);
    setBusy({ label: "Resetting cloud history…" });
    setError(null);
    try {
      const result = await api.executeCloudCleanup(config, active.operationId, confirmation);
      setNotice(`Removed ${result.snapshotsRemoved} snapshots and ${result.objectsRemoved} stored objects (${formatBytes(result.bytesRemoved)}). Push when you are ready to create a clean history.`);
      await refresh(config);
    } catch (cause) {
      setError(toMessage(cause));
      setBusy(null);
    }
  };

  if (!config || !environment) {
    return <AppTheme mode="system"><LaunchScreen error={error} /></AppTheme>;
  }

  const needsOnboarding = !config.onboardingComplete;

  return (
    <AppTheme mode={config.theme}>
    <div className="app-shell">
      <div className="mac-titlebar" data-tauri-drag-region aria-hidden="true" />
      <aside className="sidebar" aria-label="Main navigation">
        <div className="brand" data-tauri-drag-region>
          <img className="brand-logo" src={boatMark} width="38" height="38" alt="" />
          <span>
            <strong>Spice Route</strong>
            <small>Codex handoff</small>
          </span>
        </div>

        <nav>
          {navItems.map(({ id, label, icon: Icon }) => (
            <Button
              key={id}
              className={page === id || (page === "diagnostics" && id === "recovery") ? "nav-item active" : "nav-item"}
              onClick={() => setPage(id)}
              aria-current={page === id || (page === "diagnostics" && id === "recovery") ? "page" : undefined}
            >
              <Icon size={18} />
              <span>{label}</span>
              {id === "recovery" && syncStatus?.pendingRecovery && (
                <span className="nav-dot" title="Recovery needs attention" />
              )}
            </Button>
          ))}
        </nav>

        <div className="sidebar-footer">
          <div className="device-chip">
            <Laptop size={16} />
            <span><small>This device</small>{config.deviceName || "Not named"}</span>
          </div>
          <div className="sidebar-version">Spice Route {appVersion}</div>
        </div>
      </aside>

      <main className="main-content">
        <header className="topbar">
          <div>
            <p className="eyebrow">{page === "selection" ? "Sync policy" : page === "diagnostics" ? "Recovery tools" : page}</p>
            <h1>{page === "diagnostics" ? "Diagnostics" : navItems.find((item) => item.id === page)?.label}</h1>
          </div>
          <Button className="icon-button" onClick={() => void refresh(config)} aria-label="Refresh">
            <RefreshCw size={18} />
          </Button>
        </header>

        {error && !preview && <Banner kind="error" onClose={() => setError(null)}>{error}</Banner>}
        {notice && <Banner kind="success" onClose={() => setNotice(null)}>{notice}</Banner>}
        {confirmation.message && <div className="save-confirmation" role="status"><Check size={14} aria-hidden="true" />{confirmation.message}</div>}

        {page === "overview" && (
          <Overview
            config={config}
            environment={environment}
            catalog={catalog}
            status={syncStatus}
            onPush={() => void beginPreview("push")}
            onPull={(snapshotId) => void beginPreview("pull", snapshotId)}
          />
        )}
        {page === "selection" && catalog && (
          <>
          {detailsError && <Banner kind="error">Could not calculate workspace sizes: {detailsError} Select Refresh to try again.</Banner>}
          <SelectionScreen config={config} catalog={catalog} estimateState={detailsGeneration === catalogSource?.generation ? "ready" : detailsError ? "unavailable" : "pending"} onSave={(next) => void save(next, "Sync choices saved. Project folder mappings stay on this device.")} />
          </>
        )}
        {page === "recovery" && (
          <>
          {recoveryError ? <Banner kind="error">Could not load recovery points: {recoveryError} Select Refresh to try again.</Banner>
            : recoveryGeneration !== catalogSource?.generation ? <p role="status">Loading recovery points…</p> : (
          <RecoveryScreen
            recoveries={recoveries}
            onDiagnose={() => setPage("diagnostics")}
            onRestore={async (id) => {
              setBusy({ label: "Restoring rollback set…" });
              try {
                const closed = await api.requestCodexClose();
                if (!closed) throw new Error("Codex is still running. Save your work, fully quit Codex, and try the restore again.");
                await api.restoreRecovery(id);
                setNotice("Rollback restored. You can open Codex again.");
                await refresh(config);
              } catch (cause) {
                setError(toMessage(cause));
                setBusy(null);
              }
            }}
          />
          )}
          </>
        )}
        {page === "diagnostics" && (
          <DiagnosticsScreen config={config} onBack={() => {
            setPage("recovery");
            window.requestAnimationFrame(() => document.querySelector<HTMLButtonElement>("[data-diagnostics-trigger]")?.focus());
          }} />
        )}
        {page === "settings" && (
          <SettingsScreen config={config} environment={environment} onSave={(next) => void save(next, "Settings saved.")} onResetCloudHistory={() => void beginCloudCleanup()} />
        )}
      </main>

      {needsOnboarding && (
        <Onboarding config={config} environment={environment} onComplete={(next) => { setPage("selection"); void save(next); }} />
      )}
      {preview && (
        <PreviewDialog
          key={preview.operationId}
          preview={preview}
          suspended={Boolean(busy)}
          executionError={error}
          onCancel={() => setPreview(null)}
          onExecute={execute}
          onRefresh={() => void beginPreview(preview.direction, preview.snapshotId ?? undefined)}
          onSaveMappings={saveProjectMappings}
        />
      )}
      {cleanupPreview && (
        <CloudCleanupDialog
          preview={cleanupPreview}
          onCancel={() => setCleanupPreview(null)}
          onConfirm={(confirmation) => void executeCloudCleanup(confirmation)}
        />
      )}
      {busy && (
        <BusyOverlay
          label={operationProgress?.message || busy.label}
          progress={operationProgress}
          cancellable={Boolean(busy.operationId)}
          onCancel={busy.operationId ? () => void api.cancelOperation(busy.operationId!) : undefined}
        />
      )}
    </div>
    </AppTheme>
  );
}

function LaunchScreen({ error }: { error: string | null }) {
  return (
    <div className="launch-screen">
      <img className="brand-logo" src={boatMark} width="64" height="64" alt="" />
      <h1>Spice Route</h1>
      <p>{error || "Finding your Codex sessions and cloud folders…"}</p>
      {!error && <LoaderCircle className="spin" size={20} />}
    </div>
  );
}

export function Overview({
  config,
  environment,
  catalog,
  status,
  onPush,
  onPull,
}: {
  config: AppConfig;
  environment: EnvironmentDiscovery;
  catalog: ContentCatalog | null;
  status: SyncStatus | null;
  onPush: () => void;
  onPull: (snapshotId?: string) => void;
}) {
  const latest = status?.latestSnapshot;
  const heads = status?.visibleHeads ?? [];
  const hasBranches = heads.length > 1;
  const mergeReady = Boolean(status?.mergeReady);
  const canReplaceCloudHistory = heads.length > 0 && !status?.lastAppliedSnapshotId;
  const ready = environment.compatibility?.supported && Boolean(config.cloudRoot);
  return (
    <div className="page-stack">
      <section className={`hero-card status-${status?.state ?? "needsSetup"}`}>
        <div className="hero-copy">
          <div className="status-pill">
            {status?.state === "ready" ? <Check size={14} /> : <CircleAlert size={14} />}
            {!environment.compatibility?.supported ? "Compatibility check needed" : mergeReady ? "Ready to publish merge" : status?.state === "ready" ? "Ready to hand off" : hasBranches ? "Review cloud branches" : status?.state === "needsPull" ? "Pull before pushing" : "Setup needs attention"}
          </div>
          <h2>{!environment.compatibility?.supported ? "Codex compatibility could not be confirmed." : status?.message || "Choose a cloud folder to begin."}</h2>
          {!environment.compatibility?.supported && <p className="compatibility-explanation">{environment.compatibility?.explanation || "Check the Codex data folder in Settings, then refresh."}</p>}
          <p>
            {mergeReady
              ? `${heads.length} cloud branches are now represented in this device's reviewed state.`
              : hasBranches
              ? `${heads.length} published branches are visible. Choose one below to review with this device.`
              : latest
              ? `Latest visible snapshot is ${latest.shortId} from ${latest.deviceName}, ${formatTime(latest.createdAt)}.`
              : "No complete snapshots are visible in this cloud folder yet."}
          </p>
        </div>
        <div className="route-art" aria-hidden="true">
          <span className="route-node source"><Laptop size={19} /></span>
          <span className="route-line"><i /></span>
          <span className="route-node cloud"><Cloud size={21} /></span>
        </div>
      </section>

      <section className="action-grid" aria-label="Handoff actions">
        <Button className="action-card push" onClick={onPush} disabled={!ready || (hasBranches && !mergeReady && !canReplaceCloudHistory)}>
          <span className="action-icon"><ArrowUpFromLine size={23} /></span>
          <span className="action-copy"><strong>{mergeReady ? "Publish merged history" : "Push this device"}</strong><small>{mergeReady ? "Combine every visible branch" : "Publish a new verified snapshot"}</small></span>
          <ChevronRight size={19} />
        </Button>
        <Button className="action-card pull" onClick={() => onPull()} disabled={!ready || !latest || hasBranches}>
          <span className="action-icon"><ArrowDownToLine size={23} /></span>
          <span className="action-copy"><strong>Pull latest</strong><small>Preview and apply incoming changes</small></span>
          <ChevronRight size={19} />
        </Button>
      </section>

      {hasBranches && (
        <section className="panel branch-panel">
          <div className="panel-heading"><div><p className="eyebrow">Concurrent handoffs</p><h3>Choose a branch to merge</h3></div></div>
          <p className="branch-help">{mergeReady ? "The reviewed result is ready. Publish merged history to create one new head whose parents are every branch below." : "Each branch stays retained. Pulling one compares it with this device from their common ancestor; you choose any conflicting session or project versions. The next Push records all visible branches as parents."}</p>
          <div className="branch-list">
            {heads.map((head) => (
              <div className="branch-row" key={head.id}>
                <span className="snapshot-icon"><History size={18} /></span>
                <div><strong>{head.deviceName}</strong><small>{formatTime(head.createdAt)} · {head.objectCount} objects</small></div>
                <code>{head.shortId}</code>
                <Button className="secondary-button small" onClick={() => onPull(head.id)}>Review branch</Button>
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="metric-grid">
        <Metric icon={History} label="Selected chats" value={String(selectedThreadCount(config, catalog))} detail={`${catalog?.threads.length ?? 0} discovered`} />
        <Metric icon={FolderCode} label="Full projects" value={String(fullProjectCount(config, catalog))} detail={`${catalog?.projects.length ?? 0} discovered`} />
        <Metric icon={HardDrive} label="Cloud storage" value={formatBytes(status?.cloudBytes ?? 0)} detail="Snapshots retained" />
      </section>

      <section className="panel recent-panel">
        <div className="panel-heading">
          <div><p className="eyebrow">Handoff state</p><h3>Latest visible snapshot</h3></div>
        </div>
        {latest ? (
          <div className="snapshot-row">
            <span className="snapshot-icon"><ShieldCheck size={20} /></span>
            <div><strong>{latest.deviceName}</strong><small>{formatTime(latest.createdAt)} · {latest.objectCount} objects</small></div>
            <code>{latest.shortId}</code>
            <div className="verification">{latest.verified ? <><Check size={15} /> Received and verified</> : <><Cloud size={15} /> Visible in sync folder · contents checked during Pull</>}</div>
          </div>
        ) : hasBranches ? (
          <EmptyState icon={History} title={`${heads.length} branches await review`} detail="Choose a branch above. No history is discarded automatically." />
        ) : (
          <EmptyState icon={Cloud} title="Nothing published yet" detail="Push this device to create the first immutable snapshot." />
        )}
        <p className="cloud-note">
          <CircleAlert size={15} /> A saved snapshot may still be uploading in your drive client. Match the handoff ID on the receiving computer before pulling.
        </p>
      </section>
    </div>
  );
}

export function SelectionScreen({ config, catalog, onSave, estimateState = "ready" }: { config: AppConfig; catalog: ContentCatalog; onSave: (config: AppConfig) => void; estimateState?: EstimateState }) {
  const [draft, setDraft] = useState(config);
  const [query, setQuery] = useState("");
  const [tab, setTab] = useState<"projects" | "projectChats" | "projectless">("projects");
  const estimateMessage = estimateState === "pending" ? "Calculating size…" : "Size unavailable";
  useEffect(() => setDraft(config), [config]);

  const visibleProjects = catalog.projects.filter((project) => `${project.name} ${project.roots.join(" ")} ${project.localRoots.join(" ")}`.toLowerCase().includes(query.toLowerCase()));
  const [folderError, setFolderError] = useState<string | null>(null);
  const setProjectFolder = (projectId: string, index: number, path: string | null) => {
    setDraft((current) => {
      const sourceRoots = { ...current.sourceRoots };
      const destinationRoots = { ...current.destinationRoots };
      const key = `${projectId}:${index}`;
      if (path === null) {
        delete sourceRoots[key];
        delete destinationRoots[key];
        if (index === 0) { delete destinationRoots[projectId]; delete sourceRoots[projectId]; }
      } else {
        if (index === 0) { delete destinationRoots[projectId]; delete sourceRoots[projectId]; }
        sourceRoots[key] = path;
        destinationRoots[key] = path;
      }
      return { ...current, sourceRoots, destinationRoots };
    });
  };
  const browseProject = async (projectId: string, index: number, currentPath: string) => {
    setFolderError(null);
    try {
      const selected = await open({ directory: true, multiple: false, defaultPath: currentPath || undefined });
      if (typeof selected === "string") setProjectFolder(projectId, index, selected);
    } catch (cause) { setFolderError(toMessage(cause)); }
  };
  const visibleChats = catalog.threads.filter((thread) => {
    const matchesKind = tab === "projectless" ? thread.projectless : !thread.projectless;
    const project = thread.projectId ? catalog.projects.find((item) => item.id === thread.projectId) : undefined;
    const matchesQuery = `${thread.title} ${thread.preview} ${project?.name || ""}`.toLowerCase().includes(query.toLowerCase());
    return matchesKind && matchesQuery;
  });
  const updateSelection = (patch: Partial<AppConfig["selection"]>) => setDraft({
    ...draft,
    selection: { ...draft.selection, ...patch, revision: crypto.randomUUID() },
  });
  const tabs = ["projects", "projectChats", "projectless"] as const;
  const selectAdjacentTab = (event: React.KeyboardEvent<HTMLButtonElement>, current: typeof tabs[number]) => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const index = tabs.indexOf(current);
    const nextIndex = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1 : (index + (event.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
    const next = tabs[nextIndex];
    setTab(next);
    event.currentTarget.parentElement?.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus();
  };

  return (
    <div className="page-stack selection-page">
      <section className="selection-intro">
        <div>
          <h2>Keep the work you need</h2>
          <p>Choose what travels. Folder locations stay on this device.</p>
        </div>
        <div className="selection-save"><span role="status" aria-live="polite">{estimateState === "ready" ? <>{formatBytes(estimatedSelectedBytes(draft, catalog))} <small>estimated</small></> : estimateMessage}</span><Button className="primary-button" onClick={() => onSave(draft)}>Save choices</Button></div>
      </section>
      {folderError && <Banner kind="error" onClose={() => setFolderError(null)}>{folderError}</Banner>}

      <div className="selection-toolbar">
        <div className="segmented" role="tablist" aria-label="Content type">
          <Button id="sync-tab-projects" data-tab="projects" role="tab" aria-controls="sync-content-panel" aria-selected={tab === "projects"} tabIndex={tab === "projects" ? 0 : -1} onKeyDown={(event) => selectAdjacentTab(event, "projects")} onClick={() => setTab("projects")}>Projects <span>{catalog.projects.length}</span></Button>
          <Button id="sync-tab-projectChats" data-tab="projectChats" role="tab" aria-controls="sync-content-panel" aria-selected={tab === "projectChats"} tabIndex={tab === "projectChats" ? 0 : -1} onKeyDown={(event) => selectAdjacentTab(event, "projectChats")} onClick={() => setTab("projectChats")}>Project chats <span>{catalog.threads.filter((t) => !t.projectless).length}</span></Button>
          <Button id="sync-tab-projectless" data-tab="projectless" role="tab" aria-controls="sync-content-panel" aria-selected={tab === "projectless"} tabIndex={tab === "projectless" ? 0 : -1} onKeyDown={(event) => selectAdjacentTab(event, "projectless")} onClick={() => setTab("projectless")}>Projectless chats <span>{catalog.threads.filter((t) => t.projectless).length}</span></Button>
        </div>
        <label className="search-box"><Search size={16} /><input aria-label={`Search ${tab === "projects" ? "projects" : "chats"}`} value={query} onChange={(event) => setQuery(event.target.value)} placeholder={`Search ${tab === "projects" ? "projects" : "chats"}`} /></label>
      </div>

      {tab === "projects" ? (
        <section id="sync-content-panel" className="panel content-list" role="tabpanel" aria-labelledby="sync-tab-projects">
          <div className="list-header">
            <div><strong>Default for new projects</strong></div>
            <ModeSelect value={draft.selection.defaultProjectMode} onChange={(mode) => updateSelection({ defaultProjectMode: mode })} />
          </div>
          {visibleProjects.map((project) => {
            const mode = draft.selection.projectModes[project.id] ?? draft.selection.defaultProjectMode;
            return (
              <div className={`project-item project-${mode}`} key={project.id} aria-label={project.name}>
              <div className="content-row">
                <span className="content-icon"><FolderCode size={19} /></span>
                <div className="content-main"><strong>{project.name}</strong><small>{project.threadCount} {project.threadCount === 1 ? "chat" : "chats"}{project.roots.length > 1 ? ` · ${project.roots.length} folders` : ""}</small></div>
                {project.linkedWorktree && <span className="tag">worktree</span>}
                <span className="project-size" aria-label={`Estimated sync size for ${project.name}`} aria-live="polite">{estimateState === "ready" ? <>{formatBytes(estimatedProjectBytes(draft, catalog, project))}<small>estimated</small></> : estimateMessage}</span>
                <ModeSelect label={`Sync mode for ${project.name}`} value={mode} onChange={(nextMode) => updateSelection({ projectModes: { ...draft.selection.projectModes, [project.id]: nextMode } })} />
              </div>
              {mode === "full" ? <div className="project-folder-list">
                {project.roots.map((discoveredPath, index) => {
                  const key = `${project.id}:${index}`;
                  const override = draft.sourceRoots[key] ?? (index === 0 ? draft.sourceRoots[project.id] : undefined);
                  const localPath = override ?? discoveredPath;
                  const destination = draft.destinationRoots[key] ?? (index === 0 ? draft.destinationRoots[project.id] : undefined);
                  return <div className="project-folder-row" key={key}>
                    <FolderLocation label={project.roots.length > 1 ? `Folder ${index + 1} for ${project.name}` : `Local folder for ${project.name}`} path={localPath}
                      onChoose={() => void browseProject(project.id, index, localPath)}
                      onReset={override !== undefined ? () => setProjectFolder(project.id, index, null) : undefined} />
                    {destination && destination !== localPath && <small className="project-path-label">Pull destination: {destination}</small>}
                  </div>;
                })}
                {!project.roots.length && <p className="section-help">No workspace folder is recorded for this project.</p>}
              </div> : <p className="project-mode-note">{mode === "historyOnly" ? "Chats and sidebar listing · no project files" : "Not included in future transfers"}</p>}
              </div>
            );
          })}
          {!visibleProjects.length && <EmptyState icon={Search} title="No matching projects" detail="Try a different search." />}
        </section>
      ) : (
        <section id="sync-content-panel" className="panel content-list" role="tabpanel" aria-labelledby={`sync-tab-${tab}`}>
          <div className="list-header">
            <div><strong>{tab === "projectless" ? "Projectless chats" : "Chats in included projects"}</strong><small>All chats, including archived chats, are selected by default.</small></div>
            <label className="switch-label"><input type="checkbox" checked={draft.selection.includeArchived} onChange={(event) => updateSelection({ includeArchived: event.target.checked })} /><span /> Include archived</label>
          </div>
          {visibleChats.map((thread) => {
            const included = !draft.selection.excludedThreadIds.includes(thread.id);
            const project = thread.projectId ? catalog.projects.find((item) => item.id === thread.projectId) : undefined;
            const projectIncluded = !thread.projectId || (draft.selection.projectModes[thread.projectId] ?? draft.selection.defaultProjectMode) !== "excluded";
            return (
              <label className="content-row check-row" key={thread.id}>
                <input
                  type="checkbox"
                  checked={included}
                  disabled={!projectIncluded}
                  onChange={() => updateSelection({
                    excludedThreadIds: included
                      ? [...draft.selection.excludedThreadIds, thread.id]
                      : draft.selection.excludedThreadIds.filter((id) => id !== thread.id),
                  })}
                />
                <span className="content-main"><strong>{thread.title || "Untitled chat"}</strong><small>{project ? `${project.name} · ` : ""}{thread.preview || thread.cwd} · {estimateState === "ready" ? formatBytes(thread.estimatedBytes) : estimateMessage}</small></span>
                {thread.archived && <span className="tag">archived</span>}
                {!projectIncluded && <span className="tag warning">project excluded</span>}
              </label>
            );
          })}
          {!visibleChats.length && <EmptyState icon={Search} title="No matching chats" detail="Try a different search." />}
        </section>
      )}

      <details className="file-options"><summary>File exclusions <span>Dependencies, build outputs and custom patterns</span></summary><section className="compact-settings">
        <ToggleRow title="Include dependency and build folders" detail="Adds folders such as node_modules, target, dist, .next, and caches." checked={draft.selection.includeBuildOutputs} onChange={(checked) => updateSelection({ includeBuildOutputs: checked })} />
        <ToggleRow title="Include project configuration and secrets" detail="Includes .env files, keys, certificates, and local credentials inside selected project folders. Your custom exclusions still apply. Codex sign-in and machine settings stay on this PC." checked={draft.selection.includeSensitiveFiles} onChange={(checked) => updateSelection({ includeSensitiveFiles: checked })} />
        <label className="pattern-field">
          <span><strong>Additional file exclusions</strong><small>Optional comma-separated glob patterns, such as <code>coverage/**, *.iso</code>.</small></span>
          <input
            value={draft.selection.extraExcludePatterns.join(", ")}
            placeholder="coverage/**, recordings/**"
            onChange={(event) => updateSelection({ extraExcludePatterns: event.target.value.split(",").map((value) => value.trim()).filter(Boolean) })}
          />
        </label>
      </section></details>
    </div>
  );
}

export function RecoveryScreen({ recoveries, onRestore, onDiagnose }: { recoveries: RecoverySummary[]; onRestore: (id: string) => void; onDiagnose: () => void }) {
  return (
    <div className="page-stack">
      <section className="panel recovery-hero">
        <span className="large-icon"><ArchiveRestore size={25} /></span>
        <div><h2>Every pull starts with a rollback set</h2><p>Spice Route keeps the ten latest local recovery points. Interrupted operations stay here until resolved.</p></div>
      </section>
      <section className="panel content-list">
        <div className="list-header"><div><strong>Local rollback sets</strong><small>Stored only on this device.</small></div></div>
        {recoveries.map((item) => (
          <div className="content-row" key={item.id}>
            <span className="content-icon"><History size={19} /></span>
            <div className="content-main"><strong>{item.reason}</strong><small>{formatTime(item.createdAt)} · {formatBytes(item.sizeBytes)}{item.sourceSnapshotId ? ` · ${item.sourceSnapshotId.slice(0, 10)}` : ""}</small></div>
            <span className={`tag ${item.status === "pending" ? "warning" : ""}`}>{item.status}</span>
            <Button className="secondary-button small" onClick={() => onRestore(item.id)}>Restore</Button>
          </div>
        ))}
        {!recoveries.length && <EmptyState icon={ShieldCheck} title="No recovery points yet" detail="Your first successful pull will create one automatically." />}
      </section>
      <section className="diagnostics-entry">
        <div><strong>Something missing after a Pull?</strong><p>Run a private, read-only check of the configured Codex profile and export a support log without conversation text.</p></div>
        <Button className="secondary-button" data-diagnostics-trigger onClick={onDiagnose}>Open diagnostics <ChevronRight size={16} /></Button>
      </section>
    </div>
  );
}

export function DiagnosticsScreen({ config, onBack }: { config: AppConfig; onBack: () => void }) {
  const headingRef = useRef<HTMLHeadingElement>(null);
  const [report, setReport] = useState<DiagnosticsReport | null>(null);
  const [running, setRunning] = useState(true);
  const [runOutcome, setRunOutcome] = useState<"idle" | "running" | "succeeded" | "failed">("idle");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);

  const runChecks = useCallback(async () => {
    setRunning(true);
    setRunOutcome("running");
    setReport(null);
    setError(null);
    setStatus(null);
    try {
      setReport(await api.getDiagnosticsReport(config));
      setRunOutcome("succeeded");
    } catch (cause) {
      setError(toMessage(cause));
      setRunOutcome("failed");
    } finally {
      setRunning(false);
    }
  }, [config]);

  useEffect(() => {
    headingRef.current?.focus();
    void runChecks();
  }, [runChecks]);

  const exportLog = async () => {
    setError(null);
    setStatus(null);
    try {
      const stamp = new Date().toISOString().replace(/[:.]/g, "-");
      const path = await saveFile({
        defaultPath: `spice-route-diagnostics-${stamp}.json`,
        filters: [{ name: "JSON diagnostics", extensions: ["json"] }],
      });
      if (!path) return;
      await api.exportDiagnostics(config, path);
      setStatus("Diagnostics exported. The report excludes conversation text and Codex credentials.");
    } catch (cause) {
      setError(toMessage(cause));
    }
  };

  const profile = report?.report.configuredProfile;
  const stateCounts = profile?.stateDatabase?.counts ?? {};
  const historyCounts = profile?.historyDatabase?.counts ?? {};
  const countEntries = [...Object.entries(stateCounts), ...Object.entries(historyCounts)];

  return (
    <div className="page-stack diagnostics-page" aria-busy={running}>
      <div className="diagnostics-toolbar">
        <Button className="text-button" onClick={onBack}><ArrowLeft size={16} /> Back to Recovery</Button>
        <div>
          <span className="diagnostics-run-status" role="status" aria-live="polite">{runOutcome === "running" ? "Running checks…" : runOutcome === "succeeded" ? "Checks complete" : runOutcome === "failed" ? "Checks failed" : ""}</span>
          <Button className="secondary-button" disabled={running} onClick={() => void runChecks()}><RefreshCw className={running ? "spin" : undefined} size={16} /> Run checks</Button>
          <Button className="primary-button" disabled={running || !report} onClick={() => void exportLog()}><Download size={16} /> Export log</Button>
        </div>
      </div>

      <section className="diagnostics-intro">
        <span className="large-icon"><ShieldCheck size={25} /></span>
        <div><h2 ref={headingRef} tabIndex={-1}>See where a handoff landed</h2><p>Diagnostics inspect paths, database counts, compatibility, and recent Pull phases. Conversation text, credentials, and file contents are excluded.</p></div>
      </section>

      {error && <Banner kind="error">{error}</Banner>}
      {status && <Banner kind="success">{status}</Banner>}
      {running && !report && <div className="diagnostics-loading" role="status"><LoaderCircle className="spin" size={19} /> Inspecting the configured profile…</div>}

      {report && <>
        <section className="diagnostics-summary">
          <div><span>Profile</span><strong>{profile?.path || "Not reported"}</strong>{profile?.canonicalPath && profile.canonicalPath !== profile.path && <small>Resolves to {profile.canonicalPath}</small>}</div>
          <div><span>Records found</span><strong>{countEntries.reduce((total, [, value]) => total + Number(value || 0), 0).toLocaleString()}</strong><small>Bounded database counts</small></div>
          <div><span>Generated</span><strong>{formatTime(report.generatedAt)}</strong><small>Report schema {report.schemaVersion}</small></div>
        </section>
        <section className="diagnostics-findings" aria-label="Diagnostic findings">
          <div className="list-header"><div><strong>Findings</strong><small>{report.summary}</small></div></div>
          {report.findings.map((finding, index) => (
            <div className={`diagnostic-finding ${finding.severity}`} key={`${finding.title}-${index}`}>
              <span aria-hidden="true">{finding.severity === "error" ? <CircleAlert size={18} /> : finding.severity === "warning" ? <CircleAlert size={18} /> : <Check size={18} />}</span>
              <div><span className="diagnostic-severity">{finding.severity}</span><strong>{finding.title}</strong><p>{finding.detail}</p></div>
            </div>
          ))}
          {!report.findings.length && <EmptyState icon={ShieldCheck} title="No problems found" detail="The configured profile and recent handoff metadata look consistent." />}
        </section>
      </>}
    </div>
  );
}

export function SettingsScreen({ config, environment, onSave, onResetCloudHistory }: { config: AppConfig; environment: EnvironmentDiscovery; onSave: (config: AppConfig) => void; onResetCloudHistory: () => void }) {
  const [draft, setDraft] = useState(config);
  useEffect(() => setDraft(config), [config]);
  const browse = async (key: "cloudRoot" | "codexHome" | "projectlessRoot" | "projectsRoot") => {
    const selected = await open({ directory: true, multiple: false, defaultPath: draft[key] || undefined });
    if (selected) setDraft({ ...draft, [key]: selected });
  };
  return (
    <div className="page-stack">
      <section className="panel settings-form">
        <div className="form-section"><p className="eyebrow">Device</p><h2>This computer</h2></div>
        <TextField label="Device name" value={draft.deviceName} onChange={(value) => setDraft({ ...draft, deviceName: value })} help="Shown beside snapshots on your other computers." />
        <div className="form-section divider"><p className="eyebrow">Local folders</p><h2>Where Codex work lives</h2><p className="section-help">These locations have different roles and stay on this device.</p></div>
        <PathField label="Codex task & history folder" value={draft.codexHome} onBrowse={() => void browse("codexHome")} help={environment.codexHome === draft.codexHome && environment.codexHomeResolved && environment.codexHomeResolved !== draft.codexHome ? `Contains Codex databases, task metadata, and rollouts. This path resolves to ${environment.codexHomeResolved}.` : "Contains Codex databases, task metadata, and rollouts. Usually named .codex; it does not contain your project code."} />
        <PathField label="Projectless chat workspaces" value={draft.projectlessRoot} onBrowse={() => void browse("projectlessRoot")} help="Contains files and artifacts created by chats that are not attached to a saved project." />
        <PathField label="Default restore location (optional)" value={draft.projectsRoot} onChange={(value) => setDraft({ ...draft, projectsRoot: value })} onBrowse={() => void browse("projectsRoot")} help="Only prefills suggestions on Pull. Choose each project’s local folder in What to sync, including folders on different drives. Leave blank to use Documents / Codex Projects." />

        <div className="form-section divider"><p className="eyebrow">Cloud transport</p><h2>Synced folder</h2></div>
        <ChoiceField label="Provider" value={draft.cloudProvider} options={providerOptions} onChange={(value) => setDraft({ ...draft, cloudProvider: value as AppConfig["cloudProvider"] })} />
        <PathField label="Spice Route folder" value={draft.cloudRoot} onBrowse={() => void browse("cloudRoot")} help="The provider's desktop client handles sign-in and network transfer." />

        <div className="form-section divider"><p className="eyebrow">Appearance</p><h2>Theme</h2></div>
        <div className="theme-options" role="radiogroup" aria-label="Theme">
          {([['system', Laptop], ['light', Sun], ['dark', Moon]] as const).map(([value, Icon]) => <Button key={value} className={draft.theme === value ? "theme-choice active" : "theme-choice"} role="radio" aria-checked={draft.theme === value} onClick={() => setDraft({ ...draft, theme: value })}><Icon size={18} /> {value[0].toUpperCase() + value.slice(1)}</Button>)}
        </div>
        <div className="form-actions"><Button className="primary-button" onClick={() => onSave(draft)}>Save settings</Button></div>
      </section>
      <section className="panel compatibility-card">
        <span className={environment.compatibility?.supported ? "compat-icon ok" : "compat-icon warning"}>{environment.compatibility?.supported ? <Check size={20} /> : <CircleAlert size={20} />}</span>
        <div><strong>{environment.compatibility?.supported ? "Codex format supported" : "Restore compatibility blocked"}</strong><p>{environment.compatibility?.explanation || "Finish setup to inspect the Codex data format."}</p><code>{environment.compatibility?.adapter || "No adapter"}</code></div>
      </section>
      <section className="panel danger-zone">
        <span className="danger-icon"><Trash2 size={20} /></span>
        <div><strong>Reset cloud history</strong><p>Remove every published snapshot and content object from this sync folder. Local Codex sessions, projects, settings, and rollback sets stay in place. Your current selection policy remains ready for the next Push.</p></div>
        <Button className="danger-button" onClick={onResetCloudHistory}>Review reset</Button>
      </section>
    </div>
  );
}

export function Onboarding({ config, environment, onComplete }: { config: AppConfig; environment: EnvironmentDiscovery; onComplete: (config: AppConfig) => void }) {
  const dialogRef = useRef<HTMLElement>(null);
  const [step, setStep] = useState(0);
  const [draft, setDraft] = useState({ ...config, codexHome: config.codexHome || environment.codexHome || "" });
  const [previewCatalog, setPreviewCatalog] = useState<ContentCatalog | null>(null);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [providerChosen, setProviderChosen] = useState(false);
  const candidate = environment.cloudCandidates.find((item) => item.provider === draft.cloudProvider) ?? (!providerChosen ? environment.cloudCandidates[0] : undefined);
  useModalKeyboard(dialogRef, undefined, step);
  useEffect(() => {
    if (!draft.cloudRoot && candidate) {
      const separator = candidate.path.includes("\\") ? "\\" : "/";
      setDraft((value) => ({ ...value, cloudRoot: candidate.path + separator + "Spice Route", cloudProvider: providerChosen ? value.cloudProvider : candidate.provider }));
    }
  }, [candidate, draft.cloudRoot, providerChosen]);
  useEffect(() => {
    let active = true;
    setCatalogError(null);
    api.listContent(draft)
      .then((value) => {
        if (active) setPreviewCatalog(value);
      })
      .catch((cause) => {
        if (active) {
          setPreviewCatalog(null);
          setCatalogError(toMessage(cause));
        }
      });
    return () => {
      active = false;
    };
  }, [draft.codexHome, draft.projectlessRoot]);
  const chooseLocalFolder = async (key: "codexHome" | "projectlessRoot" | "projectsRoot") => {
    const selected = await open({ directory: true, multiple: false, defaultPath: draft[key] || undefined });
    if (selected) setDraft({ ...draft, [key]: selected });
  };
  const chooseFolder = async () => {
    const selected = await open({ directory: true, multiple: false, defaultPath: candidate?.path });
    if (selected) setDraft({ ...draft, cloudRoot: selected });
  };
  return (
    <div className="modal-backdrop onboarding-backdrop">
      <section ref={dialogRef} tabIndex={-1} className={`onboarding-card ${step === 0 ? "local-folders-step" : ""}`} role="dialog" aria-modal="true" aria-labelledby="welcome-title">
        <div className="onboarding-progress"><span style={{ width: `${((step + 1) / 4) * 100}%` }} /></div>
        {step === 0 && <>
          <p className="eyebrow">Step 1 of 4</p><h2 id="welcome-title">Choose your local Codex folders</h2>
          <p className="lead">Confirm where your chat history and projectless workspaces live. You’ll review each project’s own folder in What to sync.</p>
          <div className="onboarding-paths">
            <PathField label="1. Codex task & history folder" value={draft.codexHome} onBrowse={() => void chooseLocalFolder("codexHome")} help={environment.codexHome === draft.codexHome && environment.codexHomeResolved && environment.codexHomeResolved !== draft.codexHome ? `Codex databases, task metadata, and rollouts. This path resolves to ${environment.codexHomeResolved}.` : "Codex databases, task metadata, and rollouts. Usually the .codex folder; this is not your code folder."} />
            <PathField label="2. Projectless chat workspaces" value={draft.projectlessRoot} onBrowse={() => void chooseLocalFolder("projectlessRoot")} help="Files and artifacts made by chats that do not belong to a saved project." />
          </div>
          <div className="privacy-note"><FolderCode size={17} /><span>Projects can live anywhere on this PC. Spice Route discovers their folders and remembers each destination separately.</span></div>
          <Button className="primary-button wide" disabled={!draft.codexHome || !draft.projectlessRoot} onClick={() => setStep(1)}>Continue <ChevronRight size={17} /></Button>
        </>}
        {step === 1 && <>
          <p className="eyebrow">Step 2 of 4</p><h2 id="welcome-title">Name this device</h2><p className="lead">You will see this name beside its snapshots on your other computers.</p>
          <TextField label="Device name" value={draft.deviceName} autoFocus onChange={(deviceName) => setDraft({ ...draft, deviceName })} />
          <div className="onboarding-actions"><Button className="text-button" onClick={() => setStep(0)}>Back</Button><Button className="primary-button" disabled={!draft.deviceName.trim()} onClick={() => setStep(2)}>Continue <ChevronRight size={17} /></Button></div>
        </>}
        {step === 2 && <>
          <p className="eyebrow">Step 3 of 4</p><h2 id="welcome-title">Choose your cloud folder</h2><p className="lead">Sign-in stays with Google Drive, OneDrive, or iCloud. Spice Route only reads and writes the folder you choose.</p>
          <ChoiceField label="Cloud drive" value={draft.cloudProvider} options={providerOptions} onChange={(value) => { setProviderChosen(true); setDraft({ ...draft, cloudProvider: value as AppConfig["cloudProvider"], cloudRoot: "" }); }} />
          <Button className="folder-picker" onClick={() => void chooseFolder()}><span><Cloud size={21} /></span><div><strong>{draft.cloudRoot ? "Selected folder" : "Choose a synced folder"}</strong><small>{draft.cloudRoot || "No folder selected"}</small></div><ChevronRight size={18} /></Button>
          <div className="privacy-note"><ShieldCheck size={17} /><span>Credentials and global Codex settings never enter the snapshot.</span></div>
          <div className="onboarding-actions"><Button className="text-button" onClick={() => setStep(1)}>Back</Button><Button className="primary-button" disabled={!draft.cloudRoot} onClick={() => setStep(3)}>Review <ChevronRight size={17} /></Button></div>
        </>}
        {step === 3 && <>
          <p className="eyebrow">Step 4 of 4</p><h2 id="welcome-title">Review your starting choices</h2><p className="lead">Next, review each project’s local folder and choose what to include before your first Push.</p>
          {previewCatalog ? (
            <div className="onboarding-summary">
              <div><strong>{selectedThreadCount(draft, previewCatalog)}</strong><span>selected chats</span></div>
              <div><strong>{fullProjectCount(draft, previewCatalog)}</strong><span>full projects</span></div>
              <div><strong>{formatBytes(estimatedSelectedBytes(draft, previewCatalog))}</strong><span>estimated content</span></div>
            </div>
          ) : !catalogError ? (
            <div className="onboarding-loading"><LoaderCircle className="spin" size={18} /> Inspecting selected content…</div>
          ) : null}
          {catalogError && <Banner kind="warning">{catalogError}</Banner>}
          <div className="privacy-note"><ShieldCheck size={17} /><span>Dependency folders and build outputs stay local by default. Project configuration and secrets follow your sync choices. Codex sign-in and global settings stay on this PC.</span></div>
          <div className="onboarding-actions"><Button className="text-button" onClick={() => setStep(2)}>Back</Button><Button className="primary-button" onClick={() => onComplete({ ...draft, onboardingComplete: true })}>Finish setup <Check size={17} /></Button></div>
        </>}
      </section>
    </div>
  );
}

export function PreviewDialog({ preview, onCancel, onExecute, onSaveMappings, suspended = false, executionError, onRefresh }: {
  preview: OperationPreview;
  suspended?: boolean;
  executionError?: string | null;
  onCancel: () => void;
  onExecute: (resolutions: ConflictResolution[]) => void;
  onRefresh?: () => void;
  onSaveMappings: (mappings: Array<RequiredMapping & { destinationPath: string }>) => void;
}) {
  const dialogRef = useRef<HTMLElement>(null);
  const conflicts = preview.changes.filter((change) => change.action === "conflict");
  const [resolutions, setResolutions] = useState<Record<string, "local" | "incoming">>({});
  const [mappingPaths, setMappingPaths] = useState<Record<string, string>>(() => Object.fromEntries(
    preview.requiredMappings.map((mapping) => [`${mapping.projectId}:${mapping.rootIndex}`, mapping.suggestedPath]),
  ));
  const unresolved = conflicts.filter((conflict) => !resolutions[conflict.key]);
  const counts = useMemo(() => preview.changes.reduce<Record<string, number>>((result, change) => ({ ...result, [change.action]: (result[change.action] || 0) + 1 }), {}), [preview.changes]);
  const mappingsReady = preview.requiredMappings.every((mapping) => mappingPaths[`${mapping.projectId}:${mapping.rootIndex}`]?.trim());
  useModalKeyboard(dialogRef, onCancel, preview.operationId, !suspended);
  const blocked = preview.blockedReasons.length > 0 || unresolved.length > 0 || preview.requiredMappings.length > 0;
  const hasChanges = preview.changes.some((change) => change.action !== "unchanged");
  const replacement = preview.replacesCloudHistory;
  const acknowledge = preview.direction === "pull" && !hasChanges;
  const actionLabel = acknowledge ? "Acknowledge snapshot" : preview.direction === "push" ? "Push" : "Pull";
  const executeLabel = actionLabel;
  const chooseMapping = async (mapping: RequiredMapping) => {
    const key = `${mapping.projectId}:${mapping.rootIndex}`;
    const selected = await open({ directory: true, multiple: false, defaultPath: mappingPaths[key] || mapping.suggestedPath });
    if (typeof selected === "string") setMappingPaths((current) => ({ ...current, [key]: selected }));
  };
  const saveMappings = () => onSaveMappings(preview.requiredMappings.map((mapping) => ({
    ...mapping,
    destinationPath: mappingPaths[`${mapping.projectId}:${mapping.rootIndex}`] || "",
  })));
  return (
    <div className="modal-backdrop" hidden={suspended}>
      <section ref={dialogRef} tabIndex={-1} className="preview-dialog" role="dialog" aria-modal="true" aria-labelledby="preview-title">
        <header><h2 id="preview-title">{replacement ? "Review cloud replacement" : "Review this handoff"}</h2><Button className="icon-button" onClick={onCancel} aria-label="Close"><X size={18} /></Button></header>
        <div className="preview-summary">
          {(["add", "update", "delete", "conflict"] as const).map((action) => <div key={action}><strong>{counts[action] || 0}</strong><span>{action === "add" ? "New" : action[0].toUpperCase() + action.slice(1)}</span></div>)}
          <div><strong>{formatBytes(preview.estimatedBytes)}</strong><span>Transfer</span></div>
        </div>
        {executionError && <Banner kind="error"><strong>Handoff did not complete.</strong> {executionError} {onRefresh && <Button className="text-button" onClick={onRefresh}>Refresh review</Button>}</Banner>}
        {[...new Set(preview.blockedReasons)].map((reason) => <Banner key={reason} kind="error">{reason}</Banner>)}
        {preview.warnings.length > 0 && <p className="eyebrow">{preview.direction === "pull" ? "Notes recorded when this snapshot was created" : "Capture notes"}</p>}
        {[...new Set(preview.warnings)].map((warning) => <Banner key={warning} kind="warning">{warning}</Banner>)}
        {conflicts.length > 0 && <p className="conflict-status" role="status">{unresolved.length > 0 ? `Choose a version for ${unresolved.length} ${unresolved.length === 1 ? "remaining conflict" : "remaining conflicts"}.` : "All conflicts have a choice. Apply the handoff to finish."}</p>}
        {preview.requiredMappings.length > 0 && (
          <div className="mapping-list">
            <div className="mapping-intro">
              <strong>Choose where these projects live on this device</strong>
              <small>Choose a folder for each project, on any local drive. These destinations are remembered only on this device. Existing files will be checked before applying.</small>
            </div>
            {preview.requiredMappings.map((mapping) => {
              const key = `${mapping.projectId}:${mapping.rootIndex}`;
              return (
                <label className="mapping-row" key={key}>
                  <span><strong>{mapping.projectName}</strong><small>From {mapping.sourcePath}</small></span>
                  <div className="path-input">
                    <input
                      value={mappingPaths[key] || ""}
                      onChange={(event) => setMappingPaths((current) => ({ ...current, [key]: event.target.value }))}
                      aria-label={`Destination for ${mapping.projectName}, folder ${mapping.rootIndex + 1}`}
                    />
                    <Button type="button" onClick={() => void chooseMapping(mapping)}>Browse…</Button>
                  </div>
                </label>
              );
            })}
          </div>
        )}
        <div className="change-list">
          {preview.changes.filter((change) => change.action !== "unchanged").map((change) => (
            <div className="change-row" key={change.key} role={change.action === "conflict" ? "group" : undefined} aria-label={change.action === "conflict" ? change.label : undefined}>
              <span className={`change-badge ${change.action}`}>{change.action}</span>
              <div><strong>{change.label}</strong><small>{change.detail}{change.bytes ? ` · ${formatBytes(change.bytes)}` : ""}</small>
                {change.conflict && <details className="conflict-details"><summary>Compare versions</summary><p><b>This device:</b> {change.conflict.localDescription}</p><p><b>Incoming:</b> {change.conflict.incomingDescription}</p></details>}
                {resolutions[change.key] && <small className="conflict-selection">{resolutions[change.key] === "local" ? "Keeping this device’s version" : "Using the incoming version"}</small>}
              </div>
              {change.action === "conflict" && <div className="conflict-choice"><Button aria-pressed={resolutions[change.key] === "local"} className={resolutions[change.key] === "local" ? "active" : ""} onClick={() => setResolutions((current) => ({ ...current, [change.key]: "local" }))}>Keep mine</Button><Button aria-pressed={resolutions[change.key] === "incoming"} className={resolutions[change.key] === "incoming" ? "active" : ""} onClick={() => setResolutions((current) => ({ ...current, [change.key]: "incoming" }))}>Use incoming</Button></div>}
            </div>
          ))}
          {!hasChanges && preview.blockedReasons.length === 0 && preview.requiredMappings.length === 0 && <EmptyState icon={Check} title={replacement ? "Replace the cloud handoff" : "No content changes"} detail={replacement ? "Push to make this device's current selection the visible cloud handoff." : acknowledge ? "Acknowledge this snapshot to record that this device has reviewed it. Your files stay as they are." : "There are no changes to publish."} />}
        </div>
        <footer>
          <span>{preview.requiredMappings.length > 0 ? "Save project destinations to continue." : preview.blockedReasons.length > 0 ? "Resolve the errors above, then refresh this review." : unresolved.length > 0 ? "Choose a version for every conflict to continue." : replacement ? "Push will replace the visible cloud handoff with this selection." : preview.requiresCodexClose ? "Codex will be asked to close before the final check." : "Codex is already closed."}</span>
          <div>
            <Button className="secondary-button" onClick={onCancel}>Cancel</Button>
            {preview.requiredMappings.length > 0 ? (
              <Button className="primary-button" disabled={!mappingsReady} onClick={saveMappings}>Save destinations & refresh</Button>
            ) : (
              <Button className="primary-button" disabled={blocked || (!hasChanges && !acknowledge && !replacement)} onClick={() => onExecute(Object.entries(resolutions).map(([key, choice]) => ({ key, choice })))}>{preview.direction === "push" && <ArrowUpFromLine size={16} aria-hidden="true" />}{executeLabel}</Button>
            )}
          </div>
        </footer>
      </section>
    </div>
  );
}

function CloudCleanupDialog({ preview, onCancel, onConfirm }: {
  preview: CloudCleanupPreview;
  onCancel: () => void;
  onConfirm: (confirmation: string) => void;
}) {
  const dialogRef = useRef<HTMLElement>(null);
  const [confirmation, setConfirmation] = useState("");
  useModalKeyboard(dialogRef, onCancel);
  const empty = preview.snapshotCount === 0 && preview.objectCount === 0;
  return (
    <div className="modal-backdrop" role="presentation">
      <section ref={dialogRef} tabIndex={-1} className="cleanup-dialog" role="dialog" aria-modal="true" aria-labelledby="cleanup-title">
        <header>
          <span className="danger-icon"><Trash2 size={22} /></span>
          <div><p className="eyebrow">Permanent cloud cleanup</p><h2 id="cleanup-title">Reset this sync history?</h2></div>
        </header>
        <p>This removes all Spice Route snapshots and compressed content objects from the selected cloud folder. It does not remove local Codex data or local rollback sets.</p>
        <div className="cleanup-stats">
          <div><strong>{preview.snapshotCount}</strong><span>snapshots</span></div>
          <div><strong>{preview.objectCount}</strong><span>objects</span></div>
          <div><strong>{formatBytes(preview.storedBytes)}</strong><span>stored</span></div>
        </div>
        <Banner kind="warning">Wait for the drive client to finish syncing these deletions before another computer pushes. Cloud-provider recovery may retain deleted files outside Spice Route.</Banner>
        <label className="field cleanup-confirm"><span>Type <code>{preview.confirmationPhrase}</code> to confirm</span><input autoFocus value={confirmation} onChange={(event) => setConfirmation(event.target.value)} /></label>
        <footer>
          <Button className="secondary-button" onClick={onCancel}>Cancel</Button>
          <Button className="danger-button" disabled={empty || confirmation !== preview.confirmationPhrase} onClick={() => onConfirm(confirmation)}>Reset cloud history</Button>
        </footer>
      </section>
    </div>
  );
}

function BusyOverlay({ label, progress, cancellable, onCancel }: { label: string; progress: OperationProgress | null; cancellable: boolean; onCancel?: () => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  useModalKeyboard(dialogRef, undefined, progress?.operationId);
  const percent = progress && progress.totalSteps > 0 ? Math.round((progress.completedSteps / progress.totalSteps) * 100) : null;
  return <div className="busy-overlay"><div ref={dialogRef} tabIndex={-1} role="dialog" aria-modal="true" aria-live="polite"><LoaderCircle className="spin" size={25} /><strong>{label}</strong>{percent !== null && <div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}><span style={{ width: `${percent}%` }} /></div>}<small>{progress?.cancellationRequested ? "Stopping at a safe checkpoint…" : "Do not disconnect the cloud drive while this is running."}</small>{cancellable && !progress?.cancellationRequested && <Button className="text-button" onClick={onCancel}>Cancel safely</Button>}</div></div>;
}

const providerOptions = [{ value: "oneDrive", label: "OneDrive" }, { value: "googleDrive", label: "Google Drive" }, { value: "iCloud", label: "iCloud Drive" }, { value: "custom", label: "Other synced folder" }];
const modeOptions = [{ value: "full", label: "Full project" }, { value: "historyOnly", label: "Chat history only" }, { value: "excluded", label: "Excluded" }];

function ChoiceField({ label, value, options, onChange }: { label: string; value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void }) {
  const id = useId();
  return <div className="field"><span id={id}>{label}</span><Dropdown aria-labelledby={id} inlinePopup value={options.find((item) => item.value === value)?.label ?? ""} selectedOptions={[value]} onOptionSelect={(_, data) => { if (data.optionValue) onChange(data.optionValue); }}>{options.map((item) => <Option key={item.value} value={item.value}>{item.label}</Option>)}</Dropdown></div>;
}

function ModeSelect({ value, onChange, label = "Project sync mode" }: { value: ProjectMode; onChange: (mode: ProjectMode) => void; label?: string }) {
  return <Dropdown size="small" className={`mode-select mode-${value}`} listbox={{ className: "mode-options" }} aria-label={label} value={modeOptions.find((item) => item.value === value)?.label} selectedOptions={[value]} onOptionSelect={(_, data) => { if (data.optionValue) onChange(data.optionValue as ProjectMode); }}>{modeOptions.map((item) => <Option key={item.value} value={item.value}>{item.label}</Option>)}</Dropdown>;
}

function FolderLocation({ label, path, onChoose, onReset }: { label: string; path: string; onChoose: () => void; onReset?: () => void }) {
  const parts = path.replace(/\\/g, "/").replace(/\/$/, "").split("/");
  const name = parts.pop() || path;
  const parent = parts.join(" / ");
  return <div className="folder-location" aria-label={label} title={path}>
    <Folder size={14} aria-hidden="true" /><span className="folder-name">{name}</span><span className="folder-parent">{parent}</span>
    <Menu positioning="below-end"><MenuTrigger disableButtonEnhancement><Button appearance="subtle" size="small" className="folder-menu-trigger" aria-label={`Folder options: ${label}`} icon={<MoreHorizontal size={17} />} /></MenuTrigger>
      <MenuPopover className="folder-menu"><MenuList>
        <MenuItem icon={<Folder size={16} />} onClick={onChoose}>Change folder…</MenuItem>
        {onReset && <MenuItem icon={<RefreshCw size={16} />} onClick={onReset}>Use Codex location</MenuItem>}
      </MenuList></MenuPopover>
    </Menu>
  </div>;
}

function TextField({ label, value, onChange, help, autoFocus }: { label: string; value: string; onChange: (value: string) => void; help?: string; autoFocus?: boolean }) {
  return <label className="field"><span>{label}</span><input value={value} autoFocus={autoFocus} onChange={(event) => onChange(event.target.value)} />{help && <small>{help}</small>}</label>;
}

function PathField({ label, value, onBrowse, onChange, help }: { label: string; value: string; onBrowse: () => void; onChange?: (value: string) => void; help: string }) {
  const id = useId();
  return <div className="field"><label htmlFor={id}>{label}</label><div className="path-input"><input id={id} value={value} readOnly={!onChange} onChange={(event) => onChange?.(event.target.value)} aria-describedby={`${id}-help`} placeholder={onChange ? "Choose or enter a folder" : undefined} title={value} /><Button type="button" onClick={onBrowse} aria-label={`Browse ${label}`}>Browse…</Button></div><small id={`${id}-help`}>{help}</small></div>;
}

function ToggleRow({ title, detail, checked, onChange }: { title: string; detail: string; checked: boolean; onChange: (checked: boolean) => void }) {
  const id = useId();
  return <div className="toggle-row"><span><strong id={id}>{title}</strong><small id={`${id}-detail`}>{detail}</small></span><Switch checked={checked} aria-labelledby={id} aria-describedby={`${id}-detail`} onChange={(_, data) => onChange(data.checked)} /></div>;
}

function Metric({ icon: Icon, label, value, detail }: { icon: typeof Route; label: string; value: string; detail: string }) {
  return <div className="metric-card"><span><Icon size={18} /></span><div><small>{label}</small><strong>{value}</strong><em>{detail}</em></div></div>;
}

function EmptyState({ icon: Icon, title, detail }: { icon: typeof Route; title: string; detail: string }) {
  return <div className="empty-state"><Icon size={23} /><div><strong>{title}</strong><small>{detail}</small></div></div>;
}

function Banner({ kind, children, onClose }: { kind: "error" | "warning" | "success"; children: React.ReactNode; onClose?: () => void }) {
  return <div className={`banner ${kind}`} role={kind === "error" ? "alert" : "status"}>{kind === "success" ? <Check size={17} /> : <CircleAlert size={17} />}<span>{children}</span>{onClose && <Button onClick={onClose} aria-label="Dismiss"><X size={15} /></Button>}</div>;
}

function useModalKeyboard(
  container: React.RefObject<HTMLElement | null>,
  onEscape?: () => void,
  resetKey?: unknown,
  active = true,
) {
  useEffect(() => {
    if (!active) return;
    const modal = container.current;
    if (!modal) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const focusable = () => Array.from(modal.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    )).filter((element) => !element.hasAttribute("hidden"));
    (modal.querySelector<HTMLElement>("[autofocus]") ?? focusable()[0] ?? modal).focus();
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && onEscape) {
        event.preventDefault();
        onEscape();
        return;
      }
      if (event.key !== "Tab") return;
      const items = focusable();
      if (!items.length) {
        event.preventDefault();
        modal.focus();
        return;
      }
      const first = items[0];
      const last = items[items.length - 1];
      if (event.shiftKey && (document.activeElement === first || !modal.contains(document.activeElement))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && (document.activeElement === last || !modal.contains(document.activeElement))) {
        event.preventDefault();
        first.focus();
      }
    };
    document.addEventListener("keydown", handleKey);
    return () => {
      document.removeEventListener("keydown", handleKey);
      if (previous?.isConnected) previous.focus();
    };
  }, [container, onEscape, resetKey, active]);
}

function selectedThreadCount(config: AppConfig, catalog: ContentCatalog | null): number {
  return catalog?.threads.filter((thread) => {
    if (config.selection.excludedThreadIds.includes(thread.id)) return false;
    if (thread.archived && !config.selection.includeArchived) return false;
    if (!thread.projectId) return true;
    return (config.selection.projectModes[thread.projectId] ?? config.selection.defaultProjectMode) !== "excluded";
  }).length ?? 0;
}

function fullProjectCount(config: AppConfig, catalog: ContentCatalog | null): number {
  return catalog?.projects.filter((project) => (config.selection.projectModes[project.id] ?? config.selection.defaultProjectMode) === "full").length ?? 0;
}

function estimatedSelectedBytes(config: AppConfig, catalog: ContentCatalog): number {
  const chats = catalog.threads
    .filter((thread) => {
      if (config.selection.excludedThreadIds.includes(thread.id)) return false;
      if (thread.archived && !config.selection.includeArchived) return false;
      if (!thread.projectId) return true;
      return (config.selection.projectModes[thread.projectId] ?? config.selection.defaultProjectMode) !== "excluded";
    })
    .reduce((total, thread) => total + thread.estimatedBytes, 0);
  const projects = catalog.projects
    .filter((project) => (config.selection.projectModes[project.id] ?? config.selection.defaultProjectMode) === "full")
    .reduce((total, project) => total + project.estimatedBytes, 0);
  return chats + projects;
}

export function estimatedProjectBytes(config: AppConfig, catalog: ContentCatalog, project: ProjectSummary): number {
  const mode = config.selection.projectModes[project.id] ?? config.selection.defaultProjectMode;
  if (mode === "excluded") return 0;
  const history = catalog.threads.filter((thread) => thread.projectId === project.id
    && !config.selection.excludedThreadIds.includes(thread.id)
    && (config.selection.includeArchived || !thread.archived))
    .reduce((bytes, thread) => bytes + thread.estimatedBytes, 0);
  return history + (mode === "full" ? project.estimatedBytes : 0);
}

function toMessage(value: unknown): string {
  if (value instanceof Error) return value.message;
  if (typeof value === "string") return value;
  try { return JSON.stringify(value); } catch { return "An unexpected error occurred."; }
}
