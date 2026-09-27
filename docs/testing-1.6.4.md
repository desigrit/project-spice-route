# Spice Route 1.6.4 verification

This update addresses a Pull from Codex state/history 57/7 into a 57/6 Mac profile when incoming chat items contain non-null `started_at_ms` or `completed_at_ms`. The Mac history database has no columns for these values. Spice Route keeps them in a local retained-history file covered by the Pull recovery journal, imports the chat into Codex, and restores the values to later outgoing snapshots. It does not alter Codex's migration ledger or database schema.

Checks completed without launching Spice Route or accessing a personal Codex profile:

- Rust engine: 115 disposable-data tests passed.
- A 57/7 handoff imported into 57/6 retains its item timing values. An unchanged restored chat keeps its fingerprint, including its transcript and attachment content. A later Push after editing that chat restores both values on a 57/7 destination while leaving unrelated local chats intact.
- A direct import without a retained-history file still fails before mutating either database. A missing retained item blocks Push, while Pull can preview the difference for recovery.
- Existing schema profiles, exclusion rules, recovery paths, and transfer cases passed the full engine suite.
- A version 1 local settings file upgrades to version 2 on load. Older Spice Route releases reject that format, preventing a downgrade from publishing without retained values.
- Rust Clippy passed with warnings denied. Frontend: 28 tests passed and the production bundle built.

Real-device acceptance remains: Pull the identified snapshot on the Mac, verify the chats appear and continue in Codex, Push from the Mac, then Pull back on a 57/7 device and confirm both content and timing fields remain intact. The Mac's actual full schema fingerprint has not been supplied; its successful 1.6.3 preview strongly suggests that the exact 57/6 profile matched, but this must still be confirmed on the device.
