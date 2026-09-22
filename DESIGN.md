---
name: "Spice Route desktop"
description: "A calm native workspace for reviewing and handing off selected Codex work."
colors:
  accent-light: "#163d55"
  accent-hover-light: "#254f67"
  accent-pressed-light: "#0e2d43"
  on-accent-light: "#ffffff"
  selection-light: "#dde8ec"
  accent-dark: "#b5d4e5"
  accent-hover-dark: "#c5e1ef"
  accent-pressed-dark: "#99bdcf"
  on-accent-dark: "#102f41"
  selection-dark: "#304652"
  canvas-light: "#eeefec"
  surface-light: "#fcfbf8"
  raised-light: "#ffffff"
  subtle-light: "#f1f2ee"
  text-light: "#20323e"
  muted-light: "#5b6871"
  line-light: "#dfe3df"
  canvas-dark: "#171d22"
  surface-dark: "#1d2328"
  raised-dark: "#29333a"
  subtle-dark: "#262f35"
  text-dark: "#edf0ed"
  muted-dark: "#aab9c2"
  line-dark: "#36434b"
typography:
  windows-page-title:
    fontFamily: "Segoe UI Variable Display"
    fontSize: "24px"
    fontWeight: 600
  mac-page-title:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, \"Segoe UI Variable\", \"Segoe UI\", sans-serif"
    fontSize: "25px"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.015em"
  pane-title:
    fontSize: "23px"
    fontWeight: 600
  inspector-title:
    fontSize: "16px"
    fontWeight: 600
  section-title:
    fontSize: "14px"
    fontWeight: 600
  body:
    fontSize: "14px"
  mac-body:
    fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, \"Segoe UI Variable\", \"Segoe UI\", sans-serif"
    fontSize: "14px"
    lineHeight: 1.45
  row:
    fontSize: "13px"
  supporting:
    fontSize: "12px"
  metadata:
    fontSize: "11px"
rounded:
  windows-control: "4px"
  search: "6px"
  mac-control: "7px"
  shared-control: "8px"
  mac-sheet: "12px"
spacing:
  small: "8px"
  control: "12px"
  medium: "16px"
  inspector-gutter: "22px"
  large: "24px"
components:
  button-primary-windows:
    backgroundColor: "{colors.accent-light}"
    textColor: "{colors.on-accent-light}"
    rounded: "{rounded.windows-control}"
    padding: "5px 12px"
  button-primary-macos:
    backgroundColor: "{colors.accent-light}"
    textColor: "{colors.on-accent-light}"
    rounded: "{rounded.mac-control}"
    padding: "6px 12px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover-light}"
  button-primary-pressed:
    backgroundColor: "{colors.accent-pressed-light}"
  button-secondary-macos:
    backgroundColor: "{colors.raised-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.mac-control}"
    padding: "6px 12px"
  button-text-macos:
    backgroundColor: "transparent"
    textColor: "{colors.accent-light}"
    rounded: "{rounded.shared-control}"
    padding: "6px 12px"
  search:
    backgroundColor: "{colors.raised-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.search}"
  navigation-selected-macos:
    textColor: "{colors.text-light}"
    rounded: "{rounded.mac-control}"
    padding: "7px 10px"
  project-row-selected:
    backgroundColor: "{colors.selection-light}"
    textColor: "{colors.text-light}"
    height: "49px"
  project-inspector:
    textColor: "{colors.text-light}"
    width: "238px"
    padding: "14px 0 18px 22px"
---

# Design System: Spice Route desktop

## Overview

**Creative North Star: "Workspace"**

Spice Route is a calm, compact desktop workspace in navy and warm ivory. Flat surfaces, fine dividers, labelled navigation, and platform controls keep the next handoff action and its supporting facts readable.

Approved September 22, 2026: B, Side by side for Overview and C, Workbench for What to sync, from the comparison preview. Source version 1.6.0 implements that selection. Review, Recovery, and Settings keep their existing behavior while sharing the updated colors.

Windows uses actual WinUI 3 controls through Windows App SDK. macOS uses React in Tauri with the system WebKit view and Mac window treatment. Both use the same Rust sync engine.

**Key Characteristics:**

- Expanded navigation with visible labels and the approved C, sweeping sail icon.
- Parallel handoff facts and a compact project table with one selected-project inspector.
- Light and dark palettes, platform focus behavior, and Windows system high-contrast resources.

The C, sweeping sail icon was selected on September 18, 2026. Keep its flat navy and warm ivory artwork in the app and installer. The [concept notes](docs/ux-directions-2026-09/DESIGN-NOTES.md) preserve the selection contract; their sample data is illustrative.

## Colors

Navy accents sit on warm light surfaces; dark mode uses blue-grey surfaces and a pale blue accent. The frontmatter records the shared foundation from [App.xaml](native/windows/SpiceRoute.Windows/App.xaml) and [workspace.css](src/workspace.css). Light component tokens describe the default variant; use the corresponding dark roles when the appearance changes.

Primary roles are accent, hover, pressed, on-accent, and selection. Neutral roles are canvas and sidebar, page surface, raised Mac fields, subtle surface, primary text, supporting text, and separators. Existing success and warning resources continue to carry operational meaning.

**The Platform State Rule.** Resolve surfaces and text through theme resources. Keep accent fill during hover and pressed states, visible keyboard focus, and system high-contrast fallback.

Windows high contrast maps surfaces to SystemColorWindowColor, text and separators to SystemColorWindowTextColor, accent states to SystemColorHighlightColor, and action text to SystemColorHighlightTextColor. Do not substitute fixed palette colors in that mode.

## Typography

WinUI uses native text typography, with Segoe UI Variable Display explicitly assigned to its page titles. The Mac Fluent theme requests the system font stack. Preserve platform text rendering rather than loading a decorative face.

| Role | Windows | Mac/WebKit |
| --- | --- | --- |
| Page title | 24px, semibold | 25px, semibold |
| Overview pane title | 23px, semibold | 23px, semibold |
| Inspector title | 16px, semibold | 16px, semibold |
| Section title | 14px, semibold | 14px, semibold |
| Body / project row | 14px / 13px | 14px / 13px |
| Supporting facts / metadata | 12px / 11px | 12px / 11px |

Mac body line height is 1.45. Sizes and summary facts use tabular figures where the web component specifies them. Long names trim within tables and wrap in the inspector; folder controls expose the full path through their accessible name or tooltip.

## Layout

Measurements are Windows effective pixels or CSS pixels. The approved page composition is a surface contract, not a requirement to use the same column layout on every future screen.

| Pattern | Implemented dimensions and adaptation |
| --- | --- |
| Shell | Windows navigation opens at 208px, with 24px page side padding. Mac navigation is 212px, changing to 184px at viewport widths of 1050px or less; page side padding changes from 34px to 24px. |
| Overview | Content is capped at 1120px. Device and visible cloud handoff use equal columns with a divider, facts, and their respective Push and Pull actions. Facts are at least 35px tall; recent-handoff rows are at least 62px. |
| Overview narrow | Windows stacks the panes below 660px of page width. Mac stacks them at viewport widths of 970px or less, replacing the vertical divider with a horizontal one. |
| Project workbench | Project, Sync, and Size columns use a flexible name column, then 137px and 70px. Rows are 49px tall and the header is 36px. The inspector is 238px wide with a 22px gutter. |
| Workbench narrow | Windows stacks the inspector below the table below 720px of page width, in a 230px row. Mac reduces the inspector to 210px at 1120px, then stacks it at 970px in a 150px to 230px row. Mode and folder groups sit side by side in the stacked inspector. |
| Search and footer | Search changes from 260px to 205px in the narrow workbench. The Save choices footer stays outside the scrolling list. Mac supports a minimum viewport width of 760px. |

Use the compact spacing rhythm in the frontmatter. Keep native navigation-item spacing without extra margins. The Windows project and chat lists retain ListView virtualization; the Mac implementation uses a scrollable HTML table and chat list.

## Elevation & Depth

Page sections are flat, separated by space and fine rules. Windows flyouts and dialogs retain native elevation. Mac menus and sheets use soft shadows, and the sidebar uses restrained translucency. Exact Mac shadow values and the Windows primary-button brush transition are recorded in the sidecar. Reduced-motion CSS disables web transitions and repeating animations.

## Shapes

Controls use small rounded corners: native Windows buttons and mode pickers use the Windows control token; Mac buttons use the Mac control token. Search fields, Fluent controls, and Mac sheets use their recorded tokens. Tables and page sections remain square and flat. Keep the sail artwork's silhouette unchanged.

## Components

### Navigation and actions

Use the expanded Windows NavigationView, native toggle, and title-bar sail. Mac keeps labelled navigation, the native traffic-light area, and an inset translucent sidebar. Preserve the established page locations across platforms.

Windows primary buttons retain the accent-button states, disabled system resources, and system focus visuals. Primary actions are at least 32px tall. Secondary and text actions remain subordinate. The review action is **Push** with its up-arrow icon; Codex closure remains an execution check. There is no Open Codex control.

### Overview

Show selected chat and project counts once beside the device action. Show the latest visible handoff, its content amount, and its state on this device beside Pull. Use readable dates with a unique suffix and keep the full identifier selectable in Details. Snapshot ancestry remains independent of display dates.

Draw Overview from the quick catalog and known status. Do not scan working trees just to render the page. When a full-project estimate is unavailable, show **Calculated in review**. Keep **Saved to sync folder**, **Received and verified**, and **Not yet pulled** distinct; the drive application still controls delivery.

### What to sync

Use visible Projects, Project chats, and Projectless chats tabs, search, the compact project table, and one selected-project inspector. The inspector contains the mode, selected amount, and every local folder mapping. Folder actions open the platform picker; changing a mapping does not move files.

Full project estimates include selected chats and workspace files. Chat history only counts included chats; Excluded is zero. Recalculate when folder mappings or file-inclusion policy changes. Pending and failed estimates remain visibly pending or unavailable in both the table and inspector.

**The Stable Selection Rule.** Changing a project mode keeps that project selected and updates its table size, inspector size, and total together.

The Defaults popup contains the new-project mode, archived-chat inclusion, project secrets, build and dependency folders, and additional file exclusions. Individual chat exclusions remain available in both chat scopes. Project or archive settings disable ineligible chats. Keep large files, including model files, visible unless the user changes the selection.

Keep Save choices in the footer, with the note that project folders are specific to this device. Enable it after a change and keep saved feedback brief and local to the page. The Windows confirmation clears after four seconds.

### Review and progress

Preserve the existing review organization, project filter, file search, largest-first sort, conflict decisions, and native Windows virtualized file list. Routine information belongs in Notes; Attention contains decisions or concrete problems, including folder mappings and conflicts. Keep errors with their review.

Show a named operation phase, cancel control, and real totals when known. Disable page navigation during a handoff while leaving Review available for progress and cancellation. An unavailable navigation action must not leave a persistent error banner. Engine work must not block the UI thread or open child console windows.

When no saved baseline exists, Push may replace visible cloud heads after explicit review stating that only the current selection will appear in the new handoff. Older immutable objects remain until the separate Reset cloud history action is chosen.

### Recovery and Settings

Preserve grouped, icon-led settings rows with controls on the right and below labels at narrower widths. Keep advanced exclusions and cloud cleanup in expanders, and save feedback local to Settings.

Recovery retains Diagnostics for missing history after Pull, readable findings, and Export log. Reports contain versions, folder metadata, record counts, and bounded pull events, without conversation text or credentials. A successful restore does not establish that the external Codex UI displayed the restored sessions.

## Do's and Don'ts

### Do:

- Do start with the navigation pane expanded, visible menu labels, and native menu-item spacing.
- Do retain the native Windows hamburger toggle and the sweeping-sail icon in the title bar.
- Do use platform controls, keyboard behavior, automation names, and native Windows list virtualization.
- Do show named operation phases and real step totals when known.

### Don't:

- Don't add Open Codex controls.
- Don't expand the Push label into an implementation explanation; keep its up-arrow icon.
- Don't present a locally saved or merely visible handoff as proof of cloud delivery.
- Don't show a fake percentage during indeterminate preparation.
- Don't apply unselected Review or Settings concept variants.
- Don't use em dashes in public content.
