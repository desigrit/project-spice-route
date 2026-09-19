# Architecture

## Boundaries

The interface in `src` contains Overview, What to sync, Recovery, Diagnostics, and Settings for macOS. It communicates through typed Tauri commands in `src-tauri/src/lib.rs`. Windows uses a native WinUI 3 shell under `native/windows` and connects to the same engine over typed JSON-line operations.

The Rust engine is split into narrow modules:

| Module | Responsibility |
|---|---|
| engine.rs | Previews, three-way comparisons, operation lifecycle, cancellation, conflict resolution, and orchestration |
| codex.rs | Codex discovery, exact schema compatibility, selective record export/import, path mapping, and SQLite backup |
| snapshot.rs | File policy, Git capture, compressed content objects, immutable manifests, ancestry, and verification |
| recovery.rs | Local rollback copies, durable journals, integrity checks, restore, and retention |
| settings.rs | Local configuration, shared selection rules, provider folder layout, and validation |
| platform.rs | Windows and macOS cloud-folder discovery, Codex process handling, version detection, and launch integration |

`src-tauri/core` compiles the same modules as a UI-independent crate. Windows x64, Windows ARM64, macOS Apple Silicon, and macOS Intel packages all use this core and the same snapshot format.

`codexHome` stores task/history databases and metadata; `projectlessRoot` bounds workspaces associated with projectless chats. Projects use per-device `sourceRoots` and `destinationRoots` maps keyed by `projectId:rootIndex`. The interface saves the same local path for both capture and restoration. `ProjectSummary.roots` retains Codex-discovered roots, while `localRoots` shows effective capture folders. Source overrides preserve project IDs and original operational roots for import path rewriting, including moved image attachments. Incoming source paths are never implicitly accepted as restoration targets.

`projectsRoot` is an optional suggestion parent, with a generic Documents / Codex Projects fallback. It is absent from required onboarding. History-only projects use managed empty folders; excluded projects need no mapping. Root validation resolves junction ancestors and rejects overlaps with Codex/cloud storage, drive roots, parent traversal, and inconsistent shared-workspace mappings.

## Push data flow

1. Validate local paths, available space, pending recovery, selection rules, Codex build, database migrations, schema fingerprint, and visible cloud heads.
2. Create SQLite backups with the SQLite backup API.
3. Export only selected task, history, project, section, attachment, and sidebar records from the staged databases.
4. Remove destination-local task fields such as sandbox policy, approval mode, and agent path.
5. Capture selected files in bounded chunks. Hash the source a second time to detect content changes even when size and modification time did not change.
6. Capture Git refs and objects in a bundle, write a portable full index, and create a pack for staged objects that are not reachable from a commit.
7. Compress content into a local SHA-256 object store and build a manifest with device, parent, selection, compatibility, record, and object metadata.
8. Copy and verify objects in the cloud folder. Publish the final manifest last through a partial file and rename.

Existing objects are reused only after verification. A corrupt object at a content-addressed cloud path is repaired before its manifest can be published.

When a device has no saved baseline and cloud heads already exist, Push can enter replacement mode after an explicit review. The new manifest contains only the current selection and records every reviewed visible head as a parent, so it becomes the sole visible head without concatenating histories. Cloud heads are checked before capture and again immediately before publication. If another device publishes during that interval, execution stops and requires a fresh review. Older content objects remain immutable until the user runs Reset cloud history.

Preview captures compute content identities without compressing or retaining a second object store. Status refresh reads manifest structure without hydrating cloud objects. Full content verification remains part of execution, and metadata-only workspace estimates use the same file exclusions as capture.

## Pull data flow

1. Inspect published manifests, validate the history graph, and select a visible head for review.
2. Capture current local state and compare incoming and local fingerprints with their common ancestor.
3. Collect destination mappings and reject overlapping roots, case collisions, reserved Windows names, unsupported path components, and links that escape selected roots.
4. Require Codex and known writers to be closed, then verify every required object.
5. Recheck cloud heads and the local preview fingerprint.
6. Create a rollback set for affected databases, the sync baseline, UI state, replaced or deleted rollouts, attachments, workspaces, Git pointer files, Git directories, and common Git directories.
7. Construct database and rollout changes in staging. Keep unrelated rows and local global state. Preserve existing destination task permission fields; new tasks inherit the destination's current local policy with no imported agent path.
8. Apply live changes while repeatedly checking that Codex has not reopened.
9. Rebuild primary and linked Git worktrees, restore the index and staged-only objects, materialize files, and apply eligible deletions.
10. Verify SQLite integrity, imported row fingerprints, file hashes, Git connectivity, refs, index state, and final workspace state before marking the rollback set complete.

Multi-file filesystem replacement cannot be one atomic transaction. The recovery journal records enough information to finish safely or restore the prior state after interruption.

When the completed review keeps every local version and requires no content changes, Pull records the verified snapshot as the reviewed baseline without replacing Codex databases or files.

## Merge rules

Task and project fingerprints exclude operational device paths and local permission fields. Project file fingerprints include relative paths, object hashes, executability, and Git state. Git index stat metadata can still produce conservative conflicts across devices.

- A change on only one side wins automatically.
- Equal changes on both sides coalesce.
- Divergent versions of the same item become an explicit conflict.
- A source deletion applies only when the destination still matches the common baseline.
- Unrelated local files remain untouched.
- Excluding a file or task stops future transfer and does not imply deletion.

When two devices publish from the same parent, both manifests remain visible as heads. After the user reviews each branch, the next Push records all reviewed heads as parents and publishes a single merged head.

## Compatibility policy

The adapter checks the Codex CLI build, successful migration numbers, required tables, and a fingerprint of both database schemas. All must match an allowlisted profile before export or restoration.

Database imports use upserts that update existing rows in place. Whole source databases are never uploaded, and replace-style inserts are avoided because they can delete and recreate a destination row. The importer rewrites known operational paths while leaving historical conversation text unchanged.

Updating the compatibility profile requires:

1. Compare the new database schemas and migrations.
2. Review every selected table and path-bearing field.
3. Re-run disposable export, import, display, continuation, exclusion, and rollback tests.
4. Add the exact build and schema fingerprint to the allowlist only after lossless restoration is demonstrated.
