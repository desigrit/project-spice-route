# Spice Route 1.6.5 verification

Codex stores a stable database project ID and a separate legacy sidebar ID. The sidebar ID can differ across computers. Before 1.6.5, Pull added the source sidebar ID to the destination even when that destination already had another sidebar ID for the same database project. The result could be two project listings, one with no visible chats.

Version 1.6.5 uses the destination's existing sidebar ID for that database project. It moves chat assignments and sidebar order references from proven aliases to the retained listing. If an earlier Pull already created duplicate aliases for one database project, a repeated Pull offers a project-listing repair. The change is covered by the normal Pull recovery point. It does not delete or move code folders, and it never combines projects solely because they have the same name or folder path.

Disposable-data checks:

- Pull into a profile with an existing sidebar ID for the same database project, then repeat the Pull. The project has one listing and its incoming and local chats point to it.
- Add a stale sidebar alias for that exact database project. A repeated Pull offers a listing update and consolidates the aliases without dropping chat order or unrelated UI state.
- Reuse an existing exact sidebar ID even if its old host mapping is missing, without creating a new folder.
- Keep a separate project with a different database ID, the same name, and the same local root. It remains untouched.
- Compare the same project across devices with different sidebar IDs. Its project fingerprint remains equal, so the difference alone does not create a conflict.
- Run the full Rust engine suite and strict Clippy, plus frontend and native build checks.

Local verification passed without launching Spice Route: 117 Rust engine tests, strict Clippy with warnings denied, 28 frontend tests, and a production frontend build.

The [1.6.5 desktop build run](https://github.com/desigrit/project-spice-route/actions/runs/36295552890) passed on Apple Silicon, Windows x64, and Windows ARM64. CI ran engine and frontend tests and linting on the target platforms. Both Windows packages also passed installed-app startup and engine contract checks. The Apple Silicon app was ad-hoc signed and packaged as a DMG. Downloads were checked against their recorded SHA-256 values before publication:

| Package | SHA-256 |
| --- | --- |
| Apple Silicon DMG | `ab1efd6a52e8861d94667f268b5abf4cbb37ba813bade6276359a4aef6f50a73` |
| Apple Silicon app ZIP | `549c171f4a77624305d4b9a831c9d3d2ddbaabb975c32e1541de2517256c81b0` |
| Windows x64 installer | `c974278eb356bae81714f3e780929b958cd7a5692a9a4368ddb234759e149a04` |
| Windows ARM64 installer | `e5936b5bcdd826ba7ab120001047f679b87727108f201920487cceca7dcf00c2` |

Real-device acceptance remains: install 1.6.5 on both computers, close Codex, review and apply the latest Pull again on the affected computer, then reopen Codex. Check that the duplicate listings for the same project are gone and that local and incoming chats are still visible. If two listings remain, export a diagnostics report. They may represent two distinct Codex database project IDs, which require an explicit mapping choice rather than automatic merging by name.
