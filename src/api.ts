import { invoke } from "@tauri-apps/api/core";
import type {
  AppConfig,
  CloudCleanupPreview,
  CloudCleanupResult,
  ConflictResolution,
  ContentCatalog,
  DiagnosticsReport,
  EnvironmentDiscovery,
  OperationPreview,
  OperationProgress,
  OperationResult,
  RecoverySummary,
  SyncStatus,
} from "./types";

export const api = {
  discoverEnvironment: () => invoke<EnvironmentDiscovery>("discover_environment"),
  loadConfig: () => invoke<AppConfig>("load_config"),
  saveConfig: (config: AppConfig) => invoke<AppConfig>("save_config", { config }),
  listContent: (config: AppConfig) => invoke<ContentCatalog>("list_content", { config }),
  listContentQuick: (config: AppConfig) => invoke<ContentCatalog>("list_content_quick", { config }),
  getSyncStatus: (config: AppConfig) => invoke<SyncStatus>("get_sync_status", { config }),
  getDiagnosticsReport: (config: AppConfig) => invoke<DiagnosticsReport>("get_diagnostics_report", { config }),
  exportDiagnostics: (config: AppConfig, path: string) => invoke<void>("export_diagnostics", { config, path }),
  previewPush: (config: AppConfig) => invoke<OperationPreview>("preview_push", { config }),
  executePush: (config: AppConfig, operationId: string) =>
    invoke<OperationResult>("execute_push", { config, operationId }),
  previewPull: (config: AppConfig, snapshotId?: string) => invoke<OperationPreview>("preview_pull", { config, snapshotId: snapshotId ?? null }),
  executePull: (config: AppConfig, operationId: string, resolutions: ConflictResolution[]) =>
    invoke<OperationResult>("execute_pull", { config, operationId, resolutions }),
  requestCodexClose: () => invoke<boolean>("request_codex_close"),
  openCodex: () => invoke<void>("open_codex"),
  listRecoveries: () => invoke<RecoverySummary[]>("list_recoveries"),
  restoreRecovery: (recoveryId: string) => invoke<void>("restore_recovery", { recoveryId }),
  cancelOperation: (operationId: string) => invoke<void>("cancel_operation", { operationId }),
  getOperationProgress: (operationId: string) => invoke<OperationProgress | null>("get_operation_progress", { operationId }),
  previewCloudCleanup: (config: AppConfig) => invoke<CloudCleanupPreview>("preview_cloud_cleanup", { config }),
  executeCloudCleanup: (config: AppConfig, operationId: string, confirmation: string) =>
    invoke<CloudCleanupResult>("execute_cloud_cleanup", { config, operationId, confirmation }),
};

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value.toFixed(index === 0 || value >= 10 ? 0 : 1)} ${units[index]}`;
}

export function formatTime(value?: string | null): string {
  if (!value) return "Never";
  const date = new Date(value);
  return Number.isNaN(date.valueOf()) ? value : new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(date);
}
