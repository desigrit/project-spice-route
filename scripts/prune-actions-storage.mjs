import { spawnSync } from 'node:child_process';
import { appendFileSync, writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

const diagnostics = new Set([
  'board-evidence', 'board-layers-evidence', 'boards-file-recovery',
  'slack-board-evidence', 'zoom-reader-evidence', 'netflix-reader-evidence',
  'twitch-reader-evidence', 'public-update-verification',
  'desktop-visual-review', 'local-engine-qa-windows', 'local-engine-qa-macos',
  'mac-distribution-qa', 'credits-renderer-qa-windows', 'credits-renderer-qa-macos',
  'native-windows-failure', 'msix-installed-x64-validation',
]);

// Only known build outputs are eligible. New artifact names are kept by default.
export function artifactFamily(name) {
  if (diagnostics.has(name) || name === 'store-submission-bundle') return name;
  if (/^(?:zebby|pm-application-tracker)-(windows-x64|windows-arm64|macos-arm64)$/.test(name)) return name.replace(/^pm-application-tracker-/, 'zebby-');
  if (/^zebby-private(?:-macos-arm64|-macos-qa|-qa)?-[a-f0-9]{40}$/.test(name)) return name.replace(/[a-f0-9]{40}$/, 'build');
  if (/^private-published-update-(windows-latest|macos-\d+)$/.test(name)) return name;
  if (/^zebby-settings-directions-\d+$/.test(name)) return 'zebby-settings-directions';
  if (/^spice-route-\d+\.\d+\.\d+-(windows-(x64|arm64)-unsigned|macos-(apple-silicon|intel)-adhoc|winui-sample-screens)$/.test(name)) return name.replace(/\d+\.\d+\.\d+/, 'version');
  if (/^Shambu-((Host-)?Windows-(x64|ARM64)|Mac-(Apple-Silicon|Intel|Updater-unsigned)|Android-arm64-(unsigned|preview))$/.test(name)) return name;
  return null;
}

export function planCleanup(artifacts, runs, pulls = []) {
  const runById = new Map(runs.map(run => [run.id, run]));
  const openBranches = new Set(pulls.filter(pr => pr.state === 'open').map(pr => pr.head.ref));
  const ordered = artifacts.filter(a => !a.expired).toSorted((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at) || b.id - a.id);
  const newest = new Map();
  const successful = new Map();
  for (const artifact of ordered) {
    const family = artifactFamily(artifact.name);
    const run = runById.get(artifact.workflow_run?.id);
    // A PR, active build or unknown run must not displace the last stable output.
    if (!family || !run || run.status !== 'completed' || openBranches.has(artifact.workflow_run?.head_branch)) continue;
    if (!newest.has(family)) newest.set(family, artifact.id);
    if (run.conclusion === 'success' && !successful.has(family)) successful.set(family, artifact.id);
  }
  const keep = [], remove = [];
  for (const artifact of ordered) {
    const family = artifactFamily(artifact.name);
    const run = runById.get(artifact.workflow_run?.id);
    let reason;
    if (!family) reason = 'Unrecognized artifact';
    else if (!run || run.status !== 'completed') reason = 'Active or unknown workflow';
    else if (openBranches.has(artifact.workflow_run?.head_branch)) reason = 'Open pull request';
    else if (newest.get(family) === artifact.id) reason = 'Latest completed output';
    else if (successful.get(family) === artifact.id) reason = 'Latest successful output';
    (reason ? keep : remove).push({ ...artifact, reason: reason || 'Superseded completed output' });
  }
  return { keep, remove };
}

export function github(args) {
  const result = spawnSync('gh', ['api', ...args], { encoding: 'utf8', windowsHide: true, maxBuffer: 8 * 1024 * 1024, timeout: 120_000 });
  if (result.error || result.status !== 0) throw new Error(result.stderr?.trim() || result.error?.message || 'GitHub request failed');
  return result.stdout ? JSON.parse(result.stdout) : null;
}

export function listPages(endpoint, field, request = github) {
  const all = [];
  for (let page = 1; ; page++) {
    const response = request([`${endpoint}${endpoint.includes('?') ? '&' : '?'}per_page=100&page=${page}`]);
    const items = field ? response[field] : response;
    if (!Array.isArray(items)) throw new Error('Incomplete GitHub inventory');
    all.push(...items);
    if (items.length < 100) return all;
  }
}

export function validatedRepository(repository) {
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository || '')) throw new Error('Set GITHUB_REPOSITORY to owner/name');
  return repository;
}

export function validateArtifact(repository, artifact) {
  if (!Number.isSafeInteger(artifact.id) || artifact.id <= 0 || artifact.url !== `https://api.github.com/repos/${repository}/actions/artifacts/${artifact.id}`) throw new Error('Artifact does not belong to this repository');
}

export function main() {
  const repository = validatedRepository(process.env.GITHUB_REPOSITORY);
  const prefix = `repos/${repository}`;
  const artifacts = listPages(`${prefix}/actions/artifacts`, 'artifacts');
  const pulls = listPages(`${prefix}/pulls?state=open`, null);
  const runs = [...new Set(artifacts.filter(a => !a.expired).map(a => a.workflow_run?.id))].map(id => {
    if (!Number.isSafeInteger(id)) throw new Error('Artifact is missing its workflow run');
    return github([`${prefix}/actions/runs/${id}`]);
  });
  const plan = planCleanup(artifacts, runs, pulls);
  const apply = process.argv.includes('--apply');
  const report = { repository, apply, kept: plan.keep.map(({ id, name, size_in_bytes, reason }) => ({ id, name, size_in_bytes, reason })), deleted: [], skipped: [] };
  const reportIndex = process.argv.indexOf('--report');
  const reportPath = reportIndex < 0 ? null : process.argv[reportIndex + 1];
  if (reportIndex >= 0 && (!reportPath || reportPath.startsWith('--'))) throw new Error('A report path is required');
  const save = () => { if (reportPath) writeFileSync(reportPath, JSON.stringify(report, null, 2) + '\n'); };
  save();
  console.log(`${apply ? 'Removing' : 'Would remove'} ${plan.remove.length} superseded artifacts; preserving ${plan.keep.length}.`);
  if (apply && plan.remove.length) {
    // Refresh PR state immediately before mutation, including PRs opened mid-run.
    const protectedBranches = new Set(listPages(`${prefix}/pulls?state=open`, null).map(pr => pr.head.ref));
    for (const artifact of plan.remove) {
      validateArtifact(repository, artifact);
      // Re-check each run, since a completed run can be retried during cleanup.
      const run = github([`${prefix}/actions/runs/${artifact.workflow_run.id}`]);
      if (run.status !== 'completed' || protectedBranches.has(artifact.workflow_run.head_branch)) {
        report.skipped.push({ id: artifact.id, reason: 'Workflow restarted or pull request opened' }); save(); continue;
      }
      github(['--method', 'DELETE', `${prefix}/actions/artifacts/${artifact.id}`]);
      report.deleted.push({ id: artifact.id, name: artifact.name, bytes: artifact.size_in_bytes });
      save();
    }
  }
  const bytes = report.deleted.reduce((sum, a) => sum + a.bytes, 0);
  const message = `Removed ${report.deleted.length} superseded artifacts (${(bytes / 1e9).toFixed(2)} GB). Preserved ${report.kept.length} artifacts. Release assets were not deleted.\n`;
  console.log(message);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, message);
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
