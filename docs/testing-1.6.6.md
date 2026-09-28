# Spice Route 1.6.6 verification

Version 1.6.6 refines Overview on Windows and macOS. The device name leads the local pane, the latest visible handoff reports its own chat and project counts, and the delivery note sits below recent handoffs. The short handoff ID, local date, and selected size appear together. The counts come from the completed snapshot manifest, so they do not require scanning project folders when Overview opens.

The sample Windows Overview capture comes from the WinUI visual probe in the desktop build workflow. That probe uses in-memory data, keeps the window offscreen, and does not connect to a personal Codex profile or start a sync operation. The Mac interface can be rendered separately with the headless preview script described in [the screenshot guide](images/README.md).

Local verification passed: 29 frontend tests, a production frontend build, 117 Rust engine tests, strict Clippy, and a Windows x64 native compile. The [final 1.6.6 desktop build](https://github.com/desigrit/project-spice-route/actions/runs/36464918951) passed on Windows x64, Windows ARM64, and Apple Silicon. CI tested the engine and frontend on all three targets, checked both installed Windows packages and engine contracts, and built the ad-hoc signed Mac DMG. The offscreen WinUI probe passed all 59 checks, including wide and narrow layouts in light and dark themes.

The downloaded packages match their CI SHA-256 values:

| Package | SHA-256 |
| --- | --- |
| Windows x64 installer | `b452b4828b528499bf7899e78b2f1880016aeb0c55b0984aa40df1b9c29b7b90` |
| Windows ARM64 installer | `638f5626177c2bd25015ede770bf4460cb9da06799d9e8ef693431fae46bc904` |
| Apple Silicon DMG | `51f378712ea20cdf152103878eefbbb40cb5ab43b4d385b7980f4e8034dc3ad5` |
| Apple Silicon app ZIP | `99718f055f0b669e36d60b98ee725b997f774a81923f0d900683ed5bd70d4916` |

Real-device Push and Pull through the chosen cloud provider remain a separate acceptance check. A sample screenshot or successful local publication cannot prove that the drive client completed delivery.
