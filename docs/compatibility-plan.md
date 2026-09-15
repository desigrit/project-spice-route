# Compatibility plan: Codex database migrations 52 and 54

## Scope and evidence

The two validated installations use desktop 26.903.71938 with runtime 0.153.4 and state/history migrations 52/6, and desktop 26.908.40834 with runtime 0.154.0-alpha.6.2 and migrations 54/6. The supplied database definitions differ only by nullable `threads.originator TEXT` and `threads.daybreak_enabled BOOLEAN`. History tables, indexes, and triggers match. Sanitized schema fixtures contain structure only, not user records or paths.

## Contract

Spice Route does not upgrade or downgrade Codex databases or edit migration ledgers. It restores selected records into a staged copy of the destination's own databases. Supported runtime/schema profiles and transfer directions are explicit. Unknown builds, migrations, layouts, and unsupported non-null columns block writes; a migration number alone never establishes compatibility. Trigger definitions are checked as well as tables and indexes. The test build uses these same gates without a test-only bypass.

Null/missing extension fields compare equally, keeping pre-existing migration-52 snapshots stable. The 52→54 path uses the destination's native nullable columns and preserves existing destination-only extension values. The 54→52 path is supported only when incoming extension values are null. If `originator` or `daybreak_enabled` has a value, the preview names the affected chat and fields and requires upgrading the older Codex or excluding the chat. No value is silently discarded. Opaque sidecar retention would not establish how older Codex should behave with those fields, or how to reconcile a deliberate null after a later Codex upgrade; this release therefore blocks that case rather than guessing.

Snapshot format 2 marks this stricter transfer contract so older Spice Route versions cannot silently discard extension values. New readers accept format 1 and 2, including the already-published source snapshot. Every device should update Spice Route before publishing format 2.

## Validation and release limits

Use exact schema fixtures with disposable records to exercise 52→54, 54→52→54 with null fields, refusal of non-representable non-null fields before mutations, 52→54→52, repeated import, same-identity edits, exclusions, local-only field preservation, null equivalence, deletion, rollback, unknown columns, unsupported triggers, and runtime/schema mismatches. Verify schema and migration-ledger preservation and SQLite integrity. Keep existing snapshot, Git, ancestry, interruption, and recovery tests.

This proves the tested storage contract. A real cross-device Pull followed by history display and session continuation is still the manual acceptance gate. Future Codex features can change payload semantics without changing SQL; supporting every future build automatically would be an unsupported guarantee. New profiles require evidence and regression tests.
