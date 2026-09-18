import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import App, { Onboarding, Overview, PreviewDialog, SelectionScreen, SettingsScreen, estimatedProjectBytes } from "./App";
import { open } from "@tauri-apps/plugin-dialog";
import { AppTheme } from "./fluent-theme";
import { api } from "./api";
import type {
  AppConfig,
  EnvironmentDiscovery,
  SnapshotSummary,
  SyncStatus,
  ContentCatalog,
  OperationPreview,
} from "./types";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const config: AppConfig = {
  schemaVersion: 1,
  deviceId: "device-a",
  deviceName: "Windows A",
  codexHome: "C:\\Users\\test\\.codex",
  projectlessRoot: "C:\\Users\\test\\Documents\\Codex",
  projectsRoot: "C:\\Users\\test\\Documents\\Codex Projects",
  cloudRoot: "C:\\Users\\test\\OneDrive\\Spice Route",
  cloudProvider: "oneDrive",
  theme: "system",
  onboardingComplete: true,
  destinationRoots: {},
  sourceRoots: {},
  selection: {
    revision: "selection-1",
    defaultProjectMode: "full",
    projectModes: {},
    excludedThreadIds: [],
    includeArchived: true,
    includeBuildOutputs: false,
    includeSensitiveFiles: false,
    extraExcludePatterns: [],
  },
};

const environment: EnvironmentDiscovery = {
  codexHome: config.codexHome,
  codexHomeResolved: config.codexHome,
  codexExecutable: "codex.exe",
  codexVersion: "codex-cli 0.153.4",
  codexRunning: false,
  cloudCandidates: [],
  compatibility: {
    supported: true,
    adapter: "test",
    stateMigration: 52,
    historyMigration: 6,
    schemaFingerprint: "test",
    explanation: "supported",
  },
  warnings: [],
};

function snapshot(id: string, deviceName: string): SnapshotSummary {
  return {
    id,
    shortId: id.slice(0, 8),
    deviceId: deviceName,
    deviceName,
    createdAt: "2026-09-10T12:00:00Z",
    parentId: "parent",
    logicalBytes: 10,
    storedBytes: 8,
    objectCount: 1,
    verified: true,
    clientSyncState: "unknown",
  };
}

function status(mergeReady: boolean): SyncStatus {
  return {
    latestSnapshot: null,
    visibleHeads: [snapshot("aaaaaaaa-1", "Windows A"), snapshot("bbbbbbbb-2", "Windows B")],
    lastAppliedSnapshotId: "aaaaaaaa-1",
    lastPushedSnapshotId: null,
    cloudBytes: 16,
    incomingAvailable: true,
    mergeReady,
    pendingRecovery: false,
    state: "needsPull",
    message: "Two cloud branches are visible.",
  };
}

function renderOverview(syncStatus: SyncStatus) {
  render(
    <Overview
      config={config}
      environment={environment}
      catalog={{ threads: [], projects: [], totalEstimatedBytes: 0, warnings: [] }}
      status={syncStatus}
      onPush={vi.fn()}
      onPull={vi.fn()}
    />,
  );
}

describe("concurrent handoff actions", () => {
  it("does not claim visible content is verified before Pull checks it", () => {
    const visible = { ...snapshot("snapshot-visible", "Windows A"), verified: false };
    renderOverview({ ...status(false), latestSnapshot: visible, visibleHeads: [visible] });
    expect(screen.getByText(/Visible in sync folder/)).toBeInTheDocument();
    expect(screen.queryByText("Received and verified")).not.toBeInTheDocument();
  });

  it("blocks a new push until every visible branch is reviewed", () => {
    renderOverview(status(false));

    expect(screen.getByRole("button", { name: /Push this device/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Pull latest/i })).toBeDisabled();
    expect(screen.getAllByRole("button", { name: /Review branch/i })).toHaveLength(2);
  });

  it("enables the merge snapshot after all visible branches are represented locally", () => {
    renderOverview(status(true));

    expect(screen.getByRole("button", { name: /Publish merged history/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /Pull latest/i })).toBeDisabled();
  });

  it("allows a device without a baseline to review replacing visible branches", () => {
    renderOverview({ ...status(false), lastAppliedSnapshotId: null, state: "ready" });

    expect(screen.getByRole("button", { name: /Push this device/i })).toBeEnabled();
    expect(screen.getAllByRole("button", { name: /Review branch/i })).toHaveLength(2);
  });
});

const handoffPreview: OperationPreview = {
  operationId: "review-1", direction: "pull", snapshotId: "snapshot-1",
  changes: ["Atlas project", "Plan a release", "Travel notes"].map((label, index) => ({
    key: `thread:${index}`, kind: "thread", action: "conflict", label,
    detail: "Both versions changed since the common snapshot.", bytes: 1024,
    conflict: { localDescription: "Edited on this device", incomingDescription: "Edited on Travel laptop" },
  })),
  warnings: ["An external workspace was not captured.", "An external workspace was not captured."],
  blockedReasons: [], estimatedBytes: 3072, requiresCodexClose: false, requiredMappings: [],
  replacesCloudHistory: false, replacedSnapshotIds: [],
};

function renderHandoff(preview = handoffPreview, onExecute = vi.fn()) {
  render(<AppTheme mode="light"><PreviewDialog preview={preview} onCancel={vi.fn()} onExecute={onExecute} onSaveMappings={vi.fn()} /></AppTheme>);
  return onExecute;
}

describe("handoff review", () => {
  it("retains quick choices for all conflicts and explains when the handoff can proceed", () => {
    const execute = renderHandoff();
    expect(screen.getByRole("button", { name: "Pull" })).toBeDisabled();
    expect(screen.getByText("Choose a version for 3 remaining conflicts.")).toBeVisible();
    expect(screen.getAllByText("An external workspace was not captured.")).toHaveLength(1);
    act(() => {
      screen.getAllByRole("button", { name: "Use incoming" }).forEach((button) => fireEvent.click(button));
    });
    expect(screen.getByText("All conflicts have a choice. Apply the handoff to finish.")).toBeVisible();
    expect(screen.getAllByRole("button", { name: "Use incoming", pressed: true })).toHaveLength(3);
    const first = within(screen.getByRole("group", { name: "Atlas project" }));
    fireEvent.click(first.getByRole("button", { name: "Keep mine" }));
    expect(first.getByRole("button", { name: "Keep mine", pressed: true })).toBeVisible();
    expect(first.getByRole("button", { name: "Use incoming", pressed: false })).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Pull" }));
    expect(execute).toHaveBeenCalledWith([
      { key: "thread:0", choice: "local" }, { key: "thread:1", choice: "incoming" }, { key: "thread:2", choice: "incoming" },
    ]);
  });

  it("allows acknowledgment when incoming content already matches", () => {
    const execute = renderHandoff({ ...handoffPreview, changes: [{ ...handoffPreview.changes[0], action: "unchanged" }], warnings: [] });
    expect(screen.getByText("No content changes")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Acknowledge snapshot" }));
    expect(execute).toHaveBeenCalledWith([]);
  });

  it("keeps errors and required destinations blocking acknowledgment", () => {
    renderHandoff({ ...handoffPreview, changes: [], warnings: [], blockedReasons: ["Cloud content is missing."] });
    expect(screen.getByRole("alert")).toHaveTextContent("Cloud content is missing.");
    expect(screen.getByRole("button", { name: "Acknowledge snapshot" })).toBeDisabled();
    expect(screen.getByText("Resolve the errors above, then refresh this review.")).toBeVisible();
  });

  it("does not publish a push with no content changes", () => {
    renderHandoff({ ...handoffPreview, direction: "push", changes: [], warnings: [] });
    expect(screen.getByRole("button", { name: "Push" })).toBeDisabled();
  });

  it("allows an explicitly reviewed replacement push with no ordinary diff rows", () => {
    const execute = renderHandoff({
      ...handoffPreview,
      direction: "push",
      changes: [],
      warnings: ["This selection will supersede the visible cloud handoff."],
      replacesCloudHistory: true,
      replacedSnapshotIds: ["snapshot-1"],
    });
    expect(screen.getByRole("heading", { name: "Review cloud replacement" })).toBeVisible();
    expect(screen.getByText("Replace the cloud handoff")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "Push" }));
    expect(execute).toHaveBeenCalledWith([]);
  });

  it("keeps a transfer failure visible after refreshing status and preserves the choices", async () => {
    vi.spyOn(api, "discoverEnvironment").mockResolvedValue(environment);
    vi.spyOn(api, "loadConfig").mockResolvedValue(config);
    vi.spyOn(api, "listContentQuick").mockResolvedValue({ threads: [], projects: [], totalEstimatedBytes: 0, warnings: [] });
    const latest = snapshot("snapshot-1", "Travel laptop");
    vi.spyOn(api, "getSyncStatus").mockResolvedValue({ ...status(false), latestSnapshot: latest, visibleHeads: [latest] });
    vi.spyOn(api, "listRecoveries").mockResolvedValue([]);
    vi.spyOn(api, "getOperationProgress").mockResolvedValue(null);
    vi.spyOn(api, "previewPull").mockResolvedValue(handoffPreview);
    vi.spyOn(api, "executePull").mockRejectedValue(new Error("The local workspace changed. Refresh the review."));
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /Pull latest/ }));
    await screen.findByRole("dialog", { name: "Review this handoff" });
    screen.getAllByRole("button", { name: "Use incoming" }).forEach((button) => fireEvent.click(button));
    fireEvent.click(screen.getByRole("button", { name: "Pull" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("The local workspace changed. Refresh the review."));
    expect(api.listContentQuick).toHaveBeenCalledTimes(2);
    expect(screen.getAllByRole("button", { name: "Use incoming", pressed: true })).toHaveLength(3);
    expect(screen.getByRole("button", { name: "Refresh review" })).toBeVisible();
  });
});

const projectCatalog: ContentCatalog = {
  threads: [], totalEstimatedBytes: 0, warnings: [],
  projects: [
    { id: "product", name: "Product", roots: ["D:\\Code\\product"], localRoots: ["D:\\Code\\product"], threadCount: 2, estimatedBytes: 100, gitRepository: true, linkedWorktree: false },
    { id: "flights", name: "Flights", roots: ["E:\\Work\\Flights", "F:\\Worktrees\\Flights"], localRoots: ["E:\\Work\\Flights", "F:\\Worktrees\\Flights"], threadCount: 3, estimatedBytes: 100, gitRepository: true, linkedWorktree: true },
  ],
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

function mockStartup() {
  vi.spyOn(api, "discoverEnvironment").mockResolvedValue(environment);
  vi.spyOn(api, "loadConfig").mockResolvedValue(config);
  vi.spyOn(api, "listContentQuick").mockResolvedValue({ ...projectCatalog, projects: projectCatalog.projects.map((project) => ({ ...project, estimatedBytes: 0 })) });
  vi.spyOn(api, "getSyncStatus").mockResolvedValue({ ...status(false), visibleHeads: [], pendingRecovery: true });
}

describe("page-specific discovery", () => {
  it("skips workspace and recovery scans on Overview, then loads only the opened page", async () => {
    mockStartup();
    const detail = deferred<ContentCatalog>();
    const recovery = deferred<Awaited<ReturnType<typeof api.listRecoveries>>>();
    const detailedRead = vi.spyOn(api, "listContent").mockReturnValue(detail.promise);
    const recoveryRead = vi.spyOn(api, "listRecoveries").mockReturnValue(recovery.promise);
    render(<App />);
    await screen.findByTitle("Recovery needs attention");
    expect(detailedRead).not.toHaveBeenCalled();
    expect(recoveryRead).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "What to sync" }));
    await waitFor(() => expect(detailedRead).toHaveBeenCalledTimes(1));
    expect(screen.getByLabelText("Estimated sync size for Product")).toHaveTextContent("Calculating size…");
    expect(screen.getByLabelText("Estimated sync size for Product")).not.toHaveTextContent("0 B");
    fireEvent.click(screen.getByRole("combobox", { name: "Sync mode for Product" }));
    fireEvent.click(screen.getByRole("option", { name: "Chat history only" }));
    await act(async () => detail.resolve(projectCatalog));
    expect(screen.getByRole("combobox", { name: "Sync mode for Product" })).toHaveTextContent("Chat history only");
    expect(screen.getByLabelText("Estimated sync size for Flights")).toHaveTextContent("100 B");
    expect(recoveryRead).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    fireEvent.click(screen.getByRole("button", { name: "What to sync" }));
    expect(detailedRead).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Recovery" }));
    await waitFor(() => expect(recoveryRead).toHaveBeenCalledTimes(1));
    expect(screen.getByRole("status")).toHaveTextContent("Loading recovery points…");
    expect(screen.queryByText("No recovery points yet")).not.toBeInTheDocument();
    await act(async () => recovery.resolve([]));
    expect(screen.getByText("No recovery points yet")).toBeVisible();
  });

  it("shares an in-flight size scan across navigation and ignores older refresh results", async () => {
    mockStartup();
    const previous = deferred<ContentCatalog>();
    const current = deferred<ContentCatalog>();
    const detailedRead = vi.spyOn(api, "listContent").mockReturnValueOnce(previous.promise).mockReturnValueOnce(current.promise);
    vi.spyOn(api, "listRecoveries").mockResolvedValue([]);
    render(<App />);
    await screen.findByTitle("Recovery needs attention");
    fireEvent.click(screen.getByRole("button", { name: "What to sync" }));
    await waitFor(() => expect(detailedRead).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    fireEvent.click(screen.getByRole("button", { name: "What to sync" }));
    expect(detailedRead).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(detailedRead).toHaveBeenCalledTimes(2));
    await act(async () => current.resolve(projectCatalog));
    expect(screen.getByLabelText("Estimated sync size for Product")).toHaveTextContent("100 B");
    await act(async () => previous.resolve({ ...projectCatalog, projects: projectCatalog.projects.map((project) => ({ ...project, estimatedBytes: 8000 })) }));
    expect(screen.getByLabelText("Estimated sync size for Product")).toHaveTextContent("100 B");
  });

  it("keeps deferred errors on their page and retries a failed scan on return", async () => {
    mockStartup();
    const detailedRead = vi.spyOn(api, "listContent").mockRejectedValueOnce(new Error("Workspace is unavailable.")).mockResolvedValueOnce(projectCatalog);
    vi.spyOn(api, "listRecoveries").mockRejectedValue(new Error("Recovery folder is unavailable."));
    render(<App />);
    await screen.findByTitle("Recovery needs attention");
    fireEvent.click(screen.getByRole("button", { name: "What to sync" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Workspace is unavailable."));
    expect(screen.getByLabelText("Estimated sync size for Product")).toHaveTextContent("Size unavailable");
    fireEvent.click(screen.getByRole("button", { name: "Overview" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Recovery" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Recovery folder is unavailable."));
    fireEvent.click(screen.getByRole("button", { name: "What to sync" }));
    await waitFor(() => expect(screen.getByLabelText("Estimated sync size for Product")).toHaveTextContent("100 B"));
    expect(detailedRead).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("per-project local folders", () => {
  it("saves independent folders through the overflow menu and native picker", async () => {
    vi.mocked(open).mockResolvedValueOnce("G:\\Repositories\\Product").mockResolvedValueOnce("H:\\Worktrees\\Flights");
    const save = vi.fn();
    render(<AppTheme mode="light"><SelectionScreen config={config} catalog={projectCatalog} onSave={save} /></AppTheme>);
    fireEvent.click(screen.getByRole("button", { name: "Folder options: Local folder for Product" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Change folder…" }));
    await waitFor(() => expect(screen.getByLabelText("Local folder for Product")).toHaveAttribute("title", "G:\\Repositories\\Product"));
    fireEvent.click(screen.getByRole("button", { name: "Folder options: Folder 2 for Flights" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Change folder…" }));
    await waitFor(() => expect(screen.getByLabelText("Folder 2 for Flights")).toHaveAttribute("title", "H:\\Worktrees\\Flights"));
    fireEvent.click(screen.getByRole("button", { name: "Save choices" }));
    const saved = save.mock.calls[0][0] as AppConfig;
    expect(saved.sourceRoots).toEqual({ "product:0": "G:\\Repositories\\Product", "flights:1": "H:\\Worktrees\\Flights" });
    expect(saved.destinationRoots).toEqual(saved.sourceRoots);
    expect(saved.selection).toEqual(config.selection);
    expect(screen.getByLabelText("Folder 1 for Flights")).toHaveAttribute("title", "E:\\Work\\Flights");
    expect(screen.getByLabelText("Local folder for Product").querySelector("input")).toBeNull();
    expect(screen.queryByRole("button", { name: /Browse/ })).not.toBeInTheDocument();
  });

  it("restores the discovered folder and removes its remembered override", () => {
    const save = vi.fn();
    const mapped = { ...config, sourceRoots: { "product:0": "G:\\Product" }, destinationRoots: { "product:0": "G:\\Product" } };
    render(<AppTheme mode="light"><SelectionScreen config={mapped} catalog={projectCatalog} onSave={save} /></AppTheme>);
    fireEvent.click(screen.getByRole("button", { name: "Folder options: Local folder for Product" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Use Codex location" }));
    expect(screen.getByLabelText("Local folder for Product")).toHaveAttribute("title", "D:\\Code\\product");
    fireEvent.click(screen.getByRole("button", { name: "Save choices" }));
    expect(save.mock.calls[0][0].sourceRoots).toEqual({});
    expect(save.mock.calls[0][0].destinationRoots).toEqual({});
  });

  it("requires no code folder for history-only and excluded projects", () => {
    const selection = { ...config.selection, projectModes: { product: "historyOnly" as const, flights: "excluded" as const } };
    render(<AppTheme mode="dark"><SelectionScreen config={{ ...config, selection }} catalog={projectCatalog} onSave={vi.fn()} /></AppTheme>);
    expect(screen.queryByLabelText("Local folder for Product")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Folder 1 for Flights")).not.toBeInTheDocument();
    expect(screen.getByText(/no project files/)).toBeInTheDocument();
  });

  it("changes project modes through the Windows-style dropdown", () => {
    render(<AppTheme mode="light"><SelectionScreen config={config} catalog={projectCatalog} onSave={vi.fn()} /></AppTheme>);
    fireEvent.click(screen.getByRole("combobox", { name: "Sync mode for Product" }));
    fireEvent.click(screen.getByRole("option", { name: "Chat history only" }));
    expect(screen.queryByLabelText("Local folder for Product")).not.toBeInTheDocument();
  });

  it("onboards with no single project parent and keeps cloud providers selectable", async () => {
    vi.spyOn(api, "listContent").mockResolvedValue(projectCatalog);
    const complete = vi.fn();
    render(<AppTheme mode="light"><Onboarding config={{ ...config, onboardingComplete: false, projectsRoot: "" }} environment={environment} onComplete={complete} /></AppTheme>);
    expect(screen.queryByLabelText(/Project code parent/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(screen.getByRole("combobox", { name: "Cloud drive" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Review" }));
    await waitFor(() => expect(screen.getByText("full projects")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: /Finish setup/ }));
    expect(complete.mock.calls[0][0]).toMatchObject({ projectsRoot: "", onboardingComplete: true });
  });

  it("allows clearing the optional default restore suggestion", () => {
    const save = vi.fn();
    render(<AppTheme mode="light"><SettingsScreen config={config} environment={environment} onSave={save} onResetCloudHistory={vi.fn()} /></AppTheme>);
    fireEvent.change(screen.getByLabelText("Default restore location (optional)"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
    expect(save.mock.calls[0][0].projectsRoot).toBe("");
  });
});

describe("selection size estimates", () => {
  const historyCatalog: ContentCatalog = { ...projectCatalog, threads: [
    { id: "one", title: "First", preview: "", cwd: "", projectId: "product", archived: false, updatedAtMs: 0, estimatedBytes: 20, projectless: false },
    { id: "two", title: "Archived", preview: "", cwd: "", projectId: "product", archived: true, updatedAtMs: 0, estimatedBytes: 30, projectless: false },
  ] };
  it("updates history and excluded sizes immediately without saving", () => {
    render(<AppTheme mode="light"><SelectionScreen config={config} catalog={historyCatalog} onSave={vi.fn()} /></AppTheme>);
    const size = () => screen.getByLabelText("Estimated sync size for Product");
    expect(size()).toHaveTextContent("150 B");
    fireEvent.click(screen.getByRole("combobox", { name: "Sync mode for Product" }));
    fireEvent.click(screen.getByRole("option", { name: "Chat history only" }));
    expect(size()).toHaveTextContent("50 B");
    fireEvent.click(screen.getByRole("combobox", { name: "Sync mode for Product" }));
    fireEvent.click(screen.getByRole("option", { name: "Excluded" }));
    expect(size()).toHaveTextContent("0 B");
  });
  it("respects archived and excluded chats and the global mode default", () => {
    const selected = { ...config, selection: { ...config.selection, defaultProjectMode: "historyOnly" as const, includeArchived: false } };
    expect(estimatedProjectBytes(selected, historyCatalog, projectCatalog.projects[0])).toBe(20);
    selected.selection.excludedThreadIds = ["one"];
    expect(estimatedProjectBytes(selected, historyCatalog, projectCatalog.projects[0])).toBe(0);
  });
  it("keeps the existing folder when the picker is cancelled", async () => {
    vi.mocked(open).mockResolvedValueOnce(null);
    render(<AppTheme mode="light"><SelectionScreen config={config} catalog={projectCatalog} onSave={vi.fn()} /></AppTheme>);
    fireEvent.click(screen.getByRole("button", { name: "Folder options: Local folder for Product" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Change folder…" }));
    await waitFor(() => expect(open).toHaveBeenCalled());
    expect(screen.getByLabelText("Local folder for Product")).toHaveAttribute("title", "D:\\Code\\product");
  });
});
