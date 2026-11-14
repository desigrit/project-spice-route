import test from 'node:test';
import assert from 'node:assert/strict';
import { artifactFamily, listPages, planCleanup, validateArtifact } from './prune-actions-storage.mjs';

const artifact = (id, name = 'store-submission-bundle', branch = 'main') => ({ id, name, expired: false, created_at: new Date(2026, 0, id).toISOString(), workflow_run: { id, head_branch: branch }, url: `https://api.github.com/repos/example/app/actions/artifacts/${id}` });
const run = (id, conclusion = 'success', status = 'completed') => ({ id, conclusion, status });
const ids = items => items.map(a => a.id).sort((a, b) => a - b);

test('keeps the latest Store bundle and removes only older successful copies', () => {
  const p = planCleanup([artifact(1), artifact(2)], [run(1), run(2)]);
  assert.deepEqual(ids(p.keep), [2]); assert.deepEqual(ids(p.remove), [1]);
});
test('preserves latest failed evidence and the last successful installer', () => {
  const p = planCleanup([1, 2, 3].map(id => artifact(id)), [run(1), run(2), run(3, 'failure')]);
  assert.deepEqual(ids(p.keep), [2, 3]); assert.deepEqual(ids(p.remove), [1]);
});
test('active builds never displace the latest completed installer', () => {
  const p = planCleanup([1, 2, 3].map(id => artifact(id)), [run(1), run(2), run(3, null, 'in_progress')]);
  assert.deepEqual(ids(p.keep), [2, 3]); assert.deepEqual(ids(p.remove), [1]);
});
test('keeps every open PR output and latest stable output separately', () => {
  const p = planCleanup([artifact(1), artifact(2, undefined, 'feature'), artifact(3, undefined, 'feature')], [run(1), run(2), run(3)], [{ state: 'open', head: { ref: 'feature' } }]);
  assert.deepEqual(ids(p.keep), [1, 2, 3]); assert.equal(p.remove.length, 0);
});
test('fails closed for unknown workflow state or artifact family', () => {
  const p = planCleanup([artifact(1, 'backup-database'), artifact(2), artifact(3)], [run(3)]);
  assert.deepEqual(ids(p.keep), [1, 2, 3]); assert.equal(p.remove.length, 0);
});
test('groups commit and version names without mixing platforms or QA', () => {
  assert.equal(artifactFamily(`zebby-private-${'a'.repeat(40)}`), artifactFamily(`zebby-private-${'b'.repeat(40)}`));
  assert.notEqual(artifactFamily(`zebby-private-${'a'.repeat(40)}`), artifactFamily(`zebby-private-qa-${'a'.repeat(40)}`));
  assert.notEqual(artifactFamily(`zebby-private-${'a'.repeat(40)}`), artifactFamily(`zebby-private-macos-arm64-${'a'.repeat(40)}`));
  assert.equal(artifactFamily('spice-route-1.5.2-macos-intel-adhoc'), artifactFamily('spice-route-1.6.9-macos-intel-adhoc'));
  assert.notEqual(artifactFamily('spice-route-1.6.9-macos-intel-adhoc'), artifactFamily('spice-route-1.6.9-macos-apple-silicon-adhoc'));
  assert.equal(artifactFamily('someones-important-file'), null);
});
test('preserves an older platform until that platform gets a replacement', () => {
  const p = planCleanup([artifact(1, 'spice-route-1.5.2-macos-intel-adhoc'), artifact(2, 'spice-route-1.6.8-macos-apple-silicon-adhoc'), artifact(3, 'spice-route-1.6.9-macos-apple-silicon-adhoc')], [run(1), run(2), run(3)]);
  assert.deepEqual(ids(p.keep), [1, 3]); assert.deepEqual(ids(p.remove), [2]);
});
test('expired outputs cannot displace downloadable packages', () => {
  const p = planCleanup([artifact(1), { ...artifact(2), expired: true }], [run(1), run(2)]);
  assert.deepEqual(ids(p.keep), [1]); assert.equal(p.remove.length, 0);
});
test('paginates the full inventory and preserves existing query parameters', () => {
  const calls = [];
  const items = listPages('repos/example/app/pulls?state=open', null, ([url]) => { calls.push(url); return calls.length === 1 ? Array.from({ length: 100 }, (_, id) => ({ id })) : [{ id: 100 }]; });
  assert.equal(items.length, 101); assert.equal(calls[1], 'repos/example/app/pulls?state=open&per_page=100&page=2');
});
test('rejects deletion targets from a different repository', () => {
  assert.doesNotThrow(() => validateArtifact('example/app', artifact(1)));
  assert.throws(() => validateArtifact('another/app', artifact(1)));
});
