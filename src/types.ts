export type Page = "overview" | "selection" | "recovery" | "diagnostics" | "settings";
export type ProjectMode = "full" | "historyOnly" | "excluded";
export type ThemeMode = "system" | "light" | "dark";
export type CloudProvider = "oneDrive" | "googleDrive" | "iCloud" | "custom";

export interface SelectionRules {
  revision: string;
  defaultProjectMode: ProjectMode;
  projectModes: Record<string, ProjectMode>;
  excludedThreadIds: string[];
  includeArchived: boolean;
  includeBuildOutputs: boolean;
  includeSensitiveFiles: boolean;
  extraExcludePatterns: string[];
}

export interface AppConfig {
  schemaVersion: number;
  deviceId: string;
  deviceName: string;
  codexHome: string;
  projectlessRoot: string;
  projectsRoot: string;
  cloudRoot: string;
  cloudProvider: CloudProvider;
  theme: ThemeMode;
  onboardingComplete: boolean;
  destinationRoots: Record<string, string>;
  sourceRoots: Record<string, string>;
  selection: SelectionRules;
}

export interface CompatibilityInfo {
  supported: boolean;
  adapter: string;
  stateMigration: number | null;
  historyMigration: number | null;
  schemaFingerprint: string;
  explanation: string;
}

export interface CloudCandidate {
  provider: CloudProvider;
  path: string;
  label: string;
}

export interface EnvironmentDiscovery {
  codexHome: string | null;
  codexHomeResolved: string | null;
  codexExecutable: string | null;
  codexVersion: string | null;
  codexRunning: boolean;
  cloudCandidates: CloudCandidate[];
  compatibility: CompatibilityInfo | null;
  warnings: string[];
}

export interface ThreadSummary {
  id: string;
  title: string;
  preview: string;
  cwd: string;
  projectId: string | null;
  archived: boolean;
  updatedAtMs: number;
  estimatedBytes: number;
  projectless: boolean;
}

export interface ProjectSummary {
  id: string;
  name: string;
  roots: string[];
  localRoots: string[];
  threadCount: number;
  estimatedBytes: number;
  gitRepository: boolean;
  linkedWorktree: boolean;
}

export interface ContentCatalog {
  threads: ThreadSummary[];
  projects: ProjectSummary[];
  totalEstimatedBytes: number;
  warnings: string[];
}

export interface SnapshotSummary {
  id: string;
  shortId: string;
  deviceId: string;
  deviceName: string;
  createdAt: string;
  parentId: string | null;
  logicalBytes: number;
  storedBytes: number;
  objectCount: number;
  verified: boolean;
  clientSyncState: "unknown" | "waiting" | "reportedSynced";
}

export interface SyncStatus {
  latestSnapshot: SnapshotSummary | null;
  visibleHeads: SnapshotSummary[];
  lastAppliedSnapshotId: string | null;
  lastPushedSnapshotId: string | null;
  cloudBytes: number;
  incomingAvailable: boolean;
  mergeReady: boolean;
  pendingRecovery: boolean;
  state: "ready" | "needsPull" | "needsSetup" | "blocked";
  message: string;
}

export interface ChangePreview {
  key: string;
  kind: "thread" | "projectFile" | "project" | "settings";
  action: "add" | "update" | "delete" | "unchanged" | "conflict";
  label: string;
  detail: string;
  bytes: number;
  conflict?: {
    localDescription: string;
    incomingDescription: string;
  };
}

export interface OperationPreview {
  operationId: string;
  direction: "push" | "pull";
  snapshotId: string | null;
  changes: ChangePreview[];
  warnings: string[];
  blockedReasons: string[];
  estimatedBytes: number;
  requiresCodexClose: boolean;
  requiredMappings: RequiredMapping[];
  replacesCloudHistory: boolean;
  replacedSnapshotIds: string[];
}

export interface RequiredMapping {
  projectId: string;
  rootIndex: number;
  projectName: string;
  sourcePath: string;
  suggestedPath: string;
}

export interface ConflictResolution {
  key: string;
  choice: "local" | "incoming";
}

export interface OperationResult {
  snapshot: SnapshotSummary;
  warnings: string[];
  recoveryId: string | null;
  statusMessage: string;
}

export interface OperationProgress {
  operationId: string;
  phase: "ready" | "rechecking" | "capturing" | "verifying" | "publishing" | "backingUp" | "applying" | "finalVerification" | "cancelling" | "complete";
  message: string;
  completedSteps: number;
  totalSteps: number;
  cancellationRequested: boolean;
}

export interface CloudCleanupPreview {
  operationId: string;
  snapshotCount: number;
  objectCount: number;
  storedBytes: number;
  confirmationPhrase: string;
}

export interface CloudCleanupResult {
  snapshotsRemoved: number;
  objectsRemoved: number;
  bytesRemoved: number;
}

export interface RecoverySummary {
  id: string;
  createdAt: string;
  reason: string;
  sourceSnapshotId: string | null;
  status: "available" | "pending" | "restored";
  sizeBytes: number;
}

export interface DiagnosticFinding {
  severity: "info" | "warning" | "error";
  title: string;
  detail: string;
}

export interface DiagnosticsReport {
  schemaVersion: number;
  generatedAt: string;
  summary: string;
  findings: DiagnosticFinding[];
  report: {
    configuredProfile?: {
      path?: string;
      canonicalPath?: string;
      stateDatabase?: { counts?: Record<string, number> };
      historyDatabase?: { counts?: Record<string, number> };
    };
    [key: string]: unknown;
  };
}
