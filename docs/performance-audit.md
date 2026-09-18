# Startup and transfer performance audit

September 18, 2026. This audit records the causes investigated for the 0.3.2 maintenance build. Measurements below are local microbenchmarks, not end-to-end cloud transfer promises. No live personal-data restore or two-device round trip was performed during this investigation.

## What the app was doing

The older desktop scripts mainly transferred Codex history files and database content. Spice Route's Full project mode also captures selected working files, Git history, and project state. Comparing the total duration of these two operations hides a large difference in scope.

One inspected handoff selected approximately 11 GB. A single model-weight file was about 5.14 GB before compression and 4.72 GB in the content store. Other model files and generated packaging content contributed additional bytes. That large object was project content, not an unexplained copy of the Codex database.

The cloud object folders are organized by content hash. A short folder name such as `64` is a hash prefix, not a project name or file type. The manifest links each stored object to its selected source file. Review should expose that relationship and the largest size contributors.

The implementation also added avoidable work:

- Startup requested full project estimates and recovery sizes even when opening Overview.
- JSON manifests were parsed directly from unbuffered files. A parser making many small reads magnified filesystem overhead.
- Writer checks could enumerate system processes for each restored item. Thousands of individually reasonable checks accumulated into minutes.
- Child commands could open console windows during Git capture and other probes.
- New objects were compressed, and unchanged objects could go through redundant capture and verification work.
- Generated `packaging/out` and `packaging/staging` folders were not consistently recognized as build output.

The reported settings error had a separate cause. Equivalent settings containing maps could serialize in a different key order on a later command, producing a different fingerprint without a user change. Configuration and transfer-contract comparisons now canonicalize nested JSON objects before hashing. Real selection and path changes still invalidate a preview.

## Measurements

| Case | Previous path | Revised path | Existing-object reuse |
| --- | ---: | ---: | ---: |
| Parse 5,063,903 bytes of synthetic JSON | 4.905 s | 14.236 ms with buffered input | Not applicable |
| Capture 128 files of 8 KiB each | 1,834 ms | 2,034 ms | 94 ms |
| Capture a 64 MiB sample | 671 ms | 291 ms | 96 ms |

The revised compression produced about 0.5% more stored data in the measured sample. That is a sample result, not a fixed storage penalty for all files.

The small-file fresh-capture result was slightly slower. This change should not be described as making every operation faster. Existing-object reuse was substantially cheaper in both object tests, while the larger fresh capture benefited from the lighter compression path.

A process inventory measured about 16.25 ms. Multiplying that by 18,518 per-item checks gives approximately 301 seconds, or five minutes, before accounting for actual restoration. This is an extrapolation from a measured inventory cost, not an observed complete transfer trace.

These timings depend on storage, cache state, antivirus, machine load, file contents, and the cloud client's activity. They establish where avoidable cost existed; they do not establish a universal transfer time.

A read-only check of an existing installation, with engine-local state isolated in a temporary directory, produced these additional measurements:

| Startup operation | Measured time | Scope |
| --- | ---: | --- |
| Quick catalog | 37.2366 ms | 33 chats and 12 projects |
| Visible cloud status | 518.2242 ms | One visible head; 9,396,126,112 stored bytes represented |

The cloud-status measurement did not hydrate or verify object contents. These are backend operation timings, not the entire application launch duration. No private project names, conversation content, or source paths are included here.

## Changes in the maintenance build

Startup uses a quick content catalog. Full workspace estimates load when What to sync opens. Recovery-folder enumeration loads when Recovery opens; the navigation indicator uses the already available pending-recovery status. Pending estimates say **Calculating size…** instead of displaying an unmeasured zero.

Manifest JSON uses buffered reads. Overview and preview inspect visible manifest structure without downloading and decoding every cloud object. Actual Pull still verifies required content before it can be restored.

Writer monitoring moves repeated process enumeration out of the per-file restoration path. The operation retains its initial closed-Codex check and monitors for a reopened writer during execution. Cancellation and journal recovery remain part of the operation.

Windows child-process creation suppresses console windows for internal commands. This does not suppress a user-facing request to close Codex or hide an actionable command failure.

Content capture uses buffered I/O and lighter compression, and can reuse matching existing objects. Publication writes required content before the completion manifest. Content identity remains SHA-256 based; compressed byte length is not the identity of the source content.

Generated `packaging/out` and `packaging/staging` content now follows the build-output switch. Model weights remain included unless the user excludes them or chooses Chat history only. A model's size alone is not grounds to remove a file needed on another device.

## Why verification still matters

Blindly copying files would remove checks the handoff relies on: SQLite consistency, selected-history filtering, content corruption detection, project mapping, conflict detection, and recoverable restoration. The optimization target is duplicate work and expensive implementation choices, not those guarantees.

Visible snapshot-head checks detect another device publishing since a preview. They inspect the manifest graph rather than choosing a snapshot merely because its timestamp is newer. The readable handoff label now combines a timestamp with a unique suffix. This changes presentation, not immutable snapshot identities or ancestry rules.

Large new files still need to be read, hashed, stored locally, and uploaded by the drive client. Reuse helps subsequent unchanged transfers; it cannot make an initial multi-gigabyte upload instantaneous. **Saved to sync folder** means local publication finished. **Received and verified** means the receiving device has checked the required content. A local timer cannot prove cloud delivery.

## File policy and interface scope

New configurations include project configuration and secrets by default. Existing saved selections remain unchanged. In the native interface, enable **Include project secrets and configuration** in Settings. In the earlier Tauri interface, the corresponding toggle is **Include project configuration and secrets** in What to sync. Custom exclusions still apply. Global Codex sign-in credentials and machine settings remain local.

The 0.3.2 maintenance build still uses Tauri and React. The approved Workspace direction is implemented separately in Spice Route 1.4 for Windows with WinUI 3, connected to the same Rust engine. Its build and headless connection tests pass; interactive and live transfer validation remain outstanding.

See the [maintenance verification record](testing-0.3.2.md) and [Windows verification record](testing-1.4.0.md) for release checks and the remaining two-device validation.
