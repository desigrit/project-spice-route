# Compatibility plan: Codex database migrations 52, 54, and 55

## Scope and evidence

The original schema fixtures come from desktop 26.903.71938 with runtime 0.153.4 and state/history migrations 52/6, and desktop 26.908.40834 with runtime 0.154.0-alpha.6.2 and migrations 54/6. These definitions differ only by nullable `threads.originator TEXT` and `threads.daybreak_enabled BOOLEAN`.

Runtime 0.155.0-alpha.9.2 uses state/history migrations 55/6. Read-only inspection found a successfully completed migration 55. Compared with schema 54, `thread_artifacts` becomes `thread_attachments`, `artifact_type` becomes `attachment_type`, and the associated index is renamed. The other state objects and the history schema match. Sanitized fixtures contain structure only, not user records or paths.

## Contract

Spice Route does not upgrade or downgrade Codex databases or edit migration ledgers. It restores selected records into a staged copy of the destination's own databases. Supported runtime/schema profiles and transfer directions are explicit. Unknown builds, migrations, layouts, and unsupported non-null columns block writes; a migration number alone never establishes compatibility. Trigger definitions are checked as well as tables and indexes. The test build uses these same gates without a test-only bypass.

Null/missing extension fields compare equally, keeping pre-existing migration-52 snapshots stable. The 52→54 path uses the destination's native nullable columns and preserves existing destination-only extension values. The 54→52 path is supported only when incoming extension values are null. If `originator` or `daybreak_enabled` has a value, the preview names the affected chat and fields and requires upgrading the older Codex or excluding the chat. No value is silently discarded. Opaque sidecar retention would not establish how older Codex should behave with those fields, or how to reconcile a deliberate null after a later Codex upgrade; this release therefore blocks that case rather than guessing.

Snapshot format 2 marks this stricter transfer contract so older Spice Route versions cannot silently discard extension values. New readers accept format 1 and 2, including the already-published source snapshot. Every device should update Spice Route before publishing format 2.

For schema 55, the adapter translates attachment table and column names at the database boundary. Snapshots retain the existing canonical `thread_artifacts` and `artifact_type` representation. Payloads retain their values, with recognized operational file paths mapped during restoration. Equivalent chats keep the same fingerprints across the table rename, and existing snapshots remain readable. Imports preserve the destination's native schema and migration ledger. Install Spice Route 1.5.0 on every computer before exchanging schema-55 snapshots; older releases do not recognize that source profile.

## Why this is more than a folder copy

Selective handoffs must leave excluded and unrelated destination chats intact. They also preserve destination credentials, permissions, and device settings while mapping operational paths to that computer. Replacing whole databases cannot provide those guarantees. SQLite backup captures consistent staged databases, including committed changes still in the write-ahead log; file copying while a writer is active would not establish that consistency.

A full-profile replacement would be a different transfer mode. It would overwrite destination history and local state, still require compatible Codex versions, and still need a way to handle machine-specific paths. Codex's own forward migrations do not establish that an older runtime can read a newer database. The current selective mode therefore keeps explicit, tested schema adapters.

## Validation and release limits

Use exact schema fixtures with disposable records to exercise 52→54, 54→52→54 with null fields, refusal of non-representable non-null fields before mutations, 52→54→52, repeated import, same-identity edits, exclusions, local-only field preservation, null equivalence, deletion, rollback, unknown columns, unsupported triggers, and runtime/schema mismatches. Verify schema and migration-ledger preservation and SQLite integrity. Keep existing snapshot, Git, ancestry, interruption, and recovery tests.

Schema-55 coverage adds 52→55, 54→55, 55→54, 55→55, and 55→52 with representable values. It exercises attachment objects, path mapping, replacement, deletion, exclusions, repeated imports, canonical fingerprints, and refusal of malformed attachment rows before any database mutation. Non-null newer thread fields remain blocked when the destination is schema 52.

This proves the tested storage contract. A real cross-device Pull followed by history display and session continuation is still the manual acceptance gate. Future Codex features can change payload semantics without changing SQL; supporting every future build automatically would be an unsupported guarantee. New profiles require evidence and regression tests.
