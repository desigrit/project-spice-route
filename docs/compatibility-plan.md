# Compatibility plan: Codex database migrations 52, 54, 55, and 57

## Scope and evidence

The original schema fixtures come from desktop 26.903.71938 with runtime 0.153.4 and state/history migrations 52/6, and desktop 26.908.40834 with runtime 0.154.0-alpha.6.2 and migrations 54/6. These definitions differ only by nullable `threads.originator TEXT` and `threads.daybreak_enabled BOOLEAN`.

Runtime 0.155.0-alpha.9.2 uses state/history migrations 55/6. Read-only inspection found a successfully completed migration 55. Compared with schema 54, `thread_artifacts` becomes `thread_attachments`, `artifact_type` becomes `attachment_type`, and the associated index is renamed. The other state objects and the history schema match. Sanitized fixtures contain structure only, not user records or paths.

Codex desktop 26.924.2738 uses state/history migrations 57/7 on the inspected Windows PC. Relative to 55/6, `threads` adds nullable `creator_user_id` and `creator_account_id`; `thread_items` adds nullable `started_at_ms` and `completed_at_ms`. Migration 57 is labeled as a cleanup of guardian thread metadata and adds no further schema objects. The inspected profile had non-null values in both new tables, so ignoring these columns would lose data. The fixture records only schema definitions.

Schema fingerprints normalize CRLF and LF line endings before hashing. Codex databases created by equivalent Windows and macOS builds can otherwise contain identical SQL with platform-specific newlines. Previously published Windows fingerprints remain accepted when validating existing snapshot manifests.

## Contract

Spice Route does not upgrade or downgrade Codex databases or edit migration ledgers. It restores selected records into a staged copy of the destination's own databases. Supported storage profiles and transfer directions are explicit. Unknown migrations, layouts, and unsupported non-null columns block writes; a migration number alone never establishes compatibility. Trigger definitions are checked as well as tables and indexes. The detected Codex runtime remains diagnostic once the exact storage profile is established, because patch releases can retain the same database contract. The test build uses these same gates without a test-only bypass.

Null/missing extension fields compare equally, keeping pre-existing migration-52 snapshots stable. The 52→54 path uses the destination's native nullable columns and preserves existing destination-only extension values. The 54→52 path is supported only when incoming extension values are null. If `originator` or `daybreak_enabled` has a value, the preview names the affected chat and fields and requires upgrading the older Codex or excluding the chat. No value is silently discarded. Opaque sidecar retention would not establish how older Codex should behave with those fields, or how to reconcile a deliberate null after a later Codex upgrade; this release therefore blocks that case rather than guessing.

Snapshot format 2 marks this stricter transfer contract so older Spice Route versions cannot silently discard extension values. New readers accept format 1 and 2, including the already-published source snapshot. Every device should update Spice Route before publishing format 2.

For schema 55, the adapter translates attachment table and column names at the database boundary. Snapshots retain the existing canonical `thread_artifacts` and `artifact_type` representation. Payloads retain their values, with recognized operational file paths mapped during restoration. Equivalent chats keep the same fingerprints across the table rename, and existing snapshots remain readable. Imports preserve the destination's native schema and migration ledger. Install Spice Route 1.5.2 or newer on every computer before exchanging schema-55 snapshots across differing Codex patch runtimes.

For schema 57/7, the adapter keeps the newer creator and lifecycle values in selected snapshots. Null extension fields compare as absent so unchanged chats retain their identity across supported profiles. A destination on 55/6 or earlier rejects a chat with non-null 57/7 fields before database mutation. A 57/7 destination accepts older chats and preserves its own database format. Install Spice Route 1.6.2 on every participating device before exchanging 57/7 snapshots.

Additive columns can eventually be handled with a constrained capability check: preserve unknown nullable columns in known tables when both sides support them, and reject a downgrade that would drop a non-null value. New tables, triggers, constraints, path semantics, or migration behavior still need testing. A migration number alone cannot establish safety, and an unchanged SQL layout does not prove unchanged chat behavior.

## Why this is more than a folder copy

Selective handoffs must leave excluded and unrelated destination chats intact. They also preserve destination credentials, permissions, and device settings while mapping operational paths to that computer. Replacing whole databases cannot provide those guarantees. SQLite backup captures consistent staged databases, including committed changes still in the write-ahead log; file copying while a writer is active would not establish that consistency.

A full-profile replacement would be a different transfer mode. It would overwrite destination history and local state, still require compatible Codex versions, and still need a way to handle machine-specific paths. Codex's own forward migrations do not establish that an older runtime can read a newer database. The current selective mode therefore keeps explicit, tested schema adapters.

## Validation and release limits

Use exact schema fixtures with disposable records to exercise 52→54, 54→52→54 with null fields, refusal of non-representable non-null fields before mutations, 52→54→52, repeated import, same-identity edits, exclusions, local-only field preservation, null equivalence, deletion, rollback, unknown columns, unsupported triggers, and runtime variation over an unchanged profile. Verify schema and migration-ledger preservation and SQLite integrity. Keep existing snapshot, Git, ancestry, interruption, and recovery tests.

Schema-55 coverage adds 52→55, 54→55, 55→54, 55→55, and 55→52 with representable values. It exercises attachment objects, path mapping, replacement, deletion, exclusions, repeated imports, canonical fingerprints, and refusal of malformed attachment rows before any database mutation. Non-null newer thread fields remain blocked when the destination is schema 52.

This proves the tested storage contract. A real cross-device Pull followed by history display and session continuation is still the manual acceptance gate. Future Codex features can change payload semantics without changing SQL; supporting every future build automatically would be an unsupported guarantee. New profiles require evidence and regression tests.
