# UX concept design notes

September 22, 2026. Extracted from `prototype.css`, `prototype.js`, and the concept README. These notes describe the comparison prototype, not a new production design system.

## Overview

The user selected **B, Side by side for Overview** and **C, Workbench for What to sync** on September 22, 2026. The selected renders are the production layout contract. Source version 1.6.0 implements that combination; Review, Recovery, and Settings retain their existing behavior. The root [DESIGN.md](../../DESIGN.md) and [.impeccable/design.json](../../.impeccable/design.json) record the actual production system. The values in these notes describe the comparison prototype and should not override implemented tokens.

All three directions are standalone HTML samples bundled in `preview.html`. They use fabricated data and offer Windows and macOS styling, light and dark themes, and example transfer states. Interactions update page memory and reset on refresh. Folder selection, diagnostics, recovery, and transfer completion are demonstrations with no live engine or file access.

| Concept | Layout and behavior | Tradeoff |
| --- | --- | --- |
| A: Focus | One prominent next action, a compact selection summary, project-grouped review, and expandable settings. | Activity and comparisons require another step. |
| B: Side by side | Parallel device and cloud summaries, aligned project rows, and a review summary alongside changes. | More information appears on the opening screen. |
| C: Workbench | Dense project tables with a selected-item inspector, project filters in review, and category navigation in Settings. | More efficient for repeated adjustments, but busier for a quick handoff. |

## Colors

Shared semantic CSS tokens keep navy accents, warm light surfaces, and restrained status colors consistent across concepts.

| Token and role | Light | Dark |
| --- | --- | --- |
| `--bg`, page | `#fcfbf8` | `#1d2328` |
| `--rail`, navigation and window chrome | `#eeefec` | `#171d22` |
| `--panel`, secondary surface | `#f1f2ee` | `#262f35` |
| `--field`, controls | `#fff` | `#29333a` |
| `--ink`, primary text | `#20323e` | `#edf0ed` |
| `--muted`, supporting text | `#5b6871` | `#aab9c2` |
| `--line`, separators | `#dfe3df` | `#36434b` |
| `--accent`, primary action | `#163d55` | `#b5d4e5` |
| `--accent-hover` / `--accent-pressed` | `#254f67` / `#0e2d43` | `#c5e1ef` / `#99bdcf` |
| `--on-accent`, primary action text | `#fff` | `#102f41` |
| `--selection` / `--hover` | `#dde8ec` / `#e8eceb` | `#304652` / `#303c44` |
| `--success` / `--warning` / `--danger` | `#21664f` / `#855116` / `#a83e34` | `#99d1b7` / `#edc48c` / `#f0aba0` |

## Typography

Windows uses Segoe UI Variable Text with Segoe UI and sans-serif fallbacks. macOS requests the system font through `-apple-system` and `BlinkMacSystemFont`. Body text is 14px, supporting text 12px, and small labels 11px. Page titles use 25px at weight 600; section headings use 16px. Numeric summaries use tabular figures.

## Layout

The default navigation rail is 196px wide, page side padding is 30px, buttons are at least 34px tall, and navigation rows are 38px tall. Common gaps are 8px, 12px, and 16px. Focus constrains content to 850px. Side by side uses equal overview columns and 65px project rows. Workbench reserves 238px for its inspector and uses 49px table rows. At 1100px and 900px, padding and columns compress; auxiliary inspectors hide and settings controls move below labels.

The macOS sample changes window treatment, title-bar height from 36px to 45px, navigation rows to 35px, and control rounding. Its toggle changes styling only.

## Elevation & Depth

Most content uses flat surfaces and separators. Flyouts and dialogs use `--shadow`: `0 8px 24px #15293826` in light mode and `0 9px 28px #0006` in dark mode.

## Shapes

Windows controls use 5px corners; macOS buttons use 7px. Flyouts use 8px and dialogs 9px. Focus uses a 2px accent outline with a 3px offset. Button color transitions last 140ms; reduced-motion preferences disable transitions and progress animation.

## Components

| Prototype pattern | Production mapping |
| --- | --- |
| Labelled sidebar and selected page | WinUI 3 `NavigationView`; Mac sidebar in Tauri/WebKit |
| Push, Pull, secondary actions | Native Windows `Button` templates and theme resources; Mac system styling |
| Mode control, folder ellipsis | `ComboBox` or `MenuFlyout`; native folder picker for actual selection |
| Project and change lists | Virtualized `ListView` with appropriate columns and filters |
| Grouped settings and disclosures | Native inputs, selectors, toggles, and expanders |
| Conflict choices, progress, dialogs | Accessible native controls, named phases, and explicit decisions |

Use the selected B/C renders as concrete layout references. The finish review accepted the implementation after comparing all 16 Windows and Mac-styled captures. Automated checks cover the implemented layout and selection behavior, including native Windows virtualization and size states. These checks do not establish VoiceOver, real Mac runtime behavior, or live transfer correctness. The macOS application is not WinUI. See the [implementation record](README.md#production-implementation-record) for evidence and verification limits.

## Provenance and metadata

The sail artwork is the existing user-approved sweeping-sail icon. Interface icons come from the repository's Lucide dependency. Concept images are browser screenshots of the authored samples with fabricated data, not generated illustrations or live transfer evidence.

`PRODUCT.md` already records the platform as `adaptive desktop`. The Impeccable launcher does not recognize that value. This is pre-existing metadata drift, retained without repair in this concept-selection task.
