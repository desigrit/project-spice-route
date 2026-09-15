# Spice Route 0.3.1 — preparation and handoff fixes

Use `artifacts/Spice-Route-0.3.1-x64-setup.exe` on both Windows PCs. The installer is built for a normal per-user installation with no Node.js or scripts required. This build was prepared without launching the installed app, closing Codex, or running a live transfer.

## What caused the reported problems

- Preparing a handoff previously compressed, wrote, and flushed every selected object before the user approved anything. Status refresh also decompressed and hashed every cloud object. Project size discovery recursively visited dependency and build folders, including a second scan for mapped projects. These were unnecessary costs.
- A reported case had no local `state.json` and a current device ID different from its earlier snapshot's source ID. The computer name alone cannot establish a common ancestor. The app correctly required reconciliation, but its message was unclear and the review flow was broken. The available evidence does not establish why that local state was lost.
- The transfer error handler refreshed status after setting the error; refresh cleared that error. Failed Pulls therefore appeared unexplained. Selecting conflict versions also lacked clear feedback. An unchanged Pull could not be acknowledged, trapping devices that needed only a baseline update.
- The three repeated workspace warnings came from helper agents whose explicit parent belongs to a project. They were incorrectly classified as independent projectless chats. Warnings already embedded in the earlier snapshot remain historical capture notes; this release deduplicates them and labels their origin.

## Changes

- Preview captures hash content without compression or object-file writes. File mutation checks remain. Actual Push still captures, verifies, and publishes content.
- Status and Pull preview inspect manifest structure without hydrating cloud blobs. Overview says **Visible in sync folder**; actual Pull verifies required objects before applying or acknowledging the snapshot. No cloud-upload completion is inferred.
- Catalog estimates use selected portable files and prune excluded dependency/build trees. They exclude the Git archive estimate; the handoff preview supplies actual captured content totals.
- Writer detection requests only process names and executable paths. Verification and restoration avoid duplicate decompression where possible. Full Git bundle/index capture and repeated content checks remain, so this release does not promise two-second preparation for large repositories.
- Conflicts show the selected version and remaining choices. Errors stay inside the review after refresh; the same review retains its choices. **Refresh review** deliberately obtains a fresh comparison and resets stale decisions.
- A Pull with no file/record changes, including a review where every conflict keeps the local version, records the reviewed baseline without replacing Codex databases. Subsequent Push can publish the kept local changes as a descendant of that snapshot.
- Pending rollback blocks both execution paths. A Pull rollback set now includes its sync baseline and deleted transcripts. Live mutations check for reopened Codex writers individually.
- Explicit helper-agent ancestry controls project membership and exclusions. Excluding a parent stops transfer of its descendants without treating older uploaded children as deletions. Windows extended and UNC paths map without requiring the source drive on the receiver.
- Short handoff identifiers use the final eight identifier characters. Existing snapshots keep their full immutable IDs. The displayed suffix distinguishes snapshots created on the same day.

## Test the earlier snapshot

1. Install this version on both PCs when convenient. Leave the cloud history intact.
2. Finish active Codex work and fully close Codex before making the review, so closing it afterward cannot invalidate the comparison.
3. Choose **Pull latest** and confirm the source device, time, and handoff ID match the snapshot you want. Review local folder mappings.
4. For each conflict choose **Keep mine** for the current local version or **Use incoming** for the version in the earlier snapshot. Choosing incoming is a replacement decision, not a way to dismiss a warning.
5. Apply the review, or acknowledge it if no changes are needed. The completed review establishes the baseline; Push becomes available. If anything fails, the actual error remains visible. Use Recovery if an interrupted application is reported.
6. Push the desired current state, wait for the drive client to sync, then match its new handoff ID on the other PC before Pull. Confirm history and continuation there.

## Validation and remaining limits

47 Rust tests and 19 frontend tests cover the compatibility matrix, Git/rollback fixtures, hash-only previews, missing/corrupt objects, filtered estimates, helper ancestry and exclusions, missing-baseline reconciliation, pending recovery, conflict selection retention, failure visibility, and honest cloud verification labels. Light/dark handoff layouts were inspected in headless renders at two window sizes. Release Clippy, TypeScript, and the MSVC/NSIS package build are release checks.

The compatibility policy remains the tested 52/6 and 54/6 profiles described in [0.3.0](testing-0.3.0.md). No live A-to-B round trip or physical cloud-client timing was performed for this patch. Git index stat metadata and Codex runtime metadata can still produce conservative conflicts; these fingerprints were retained to avoid silently dropping meaningful changes in older snapshots.
