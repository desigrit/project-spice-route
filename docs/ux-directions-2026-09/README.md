# Spice Route UX directions

September 22, 2026. The user approved **B, Side by side for Overview** and **C, Workbench for What to sync**. Source version 1.6.0 implements that combination in native Windows WinUI 3 and the React/Tauri Mac app. Review, Recovery, and Settings keep their existing behavior. The comparison preview remains an interactive concept with invented sample data.

Open [the comparison preview](preview.html). It works offline. Choose A, B, or C at the top, then use the app navigation. Theme, platform styling, and example state are controlled outside the app frame.

## Three approaches

| Direction | Everyday experience | Review and selection | Tradeoff |
| --- | --- | --- | --- |
| A: Focus | A clear next action, followed by a compact summary of what travels. | Changes grouped by project; advanced settings disclosed when needed. | Recent activity and detailed comparisons take another click. |
| B: Side by side | This device and the visible cloud handoff are equally easy to understand. | Changes and a small handoff summary; compact project rows. | More information is present on the opening screen. |
| C: Workbench | Projects stay in view, with details for the selected item alongside. | Explorer-style filtering and dense lists; settings grouped by category. | Best for frequent adjustments, but busier for a quick handoff. |

All three preserve labelled navigation, the approved sail icon, navy accents, light and dark themes, and the short Push and Pull labels. macOS styling is an adaptation of the same hierarchy.

## What changes

The previous interface often gave operational detail the same emphasis as the user's next action. Some key facts appeared more than once, and large warning areas competed with the file list.

The concepts separate these concerns:

- Overview answers where work is, what will travel, and what to do next.
- What to sync keeps project mode, selected size, and local folder together. Folder controls open a short menu. No path text box or Browse button is needed in a project row.
- Review has Changes, Needs attention, and Details. Only decisions block the main action.
- Each conflict has explicit local and incoming choices. Resolved items update the remaining count.
- Progress names its current phase. It does not invent a completion percentage.
- A completed Push says Saved to sync folder. A successful Pull says Received and verified. Neither proves the external Codex interface displayed the restored chats.
- Settings uses short labels and supporting text. Its saved confirmation clears automatically and stays on that page.
- Recovery distinguishes local restore points from cloud history and gives missing-history diagnostics a direct entry.

## Try the prototype

1. Switch among the three directions on the same page.
2. Open What to sync and change Spice Route from Chat history only to Full project. The selected amount changes from 207 MB to 995 MB.
3. Open a folder menu using its ellipsis.
4. Exclude a projectless or project chat and watch the count and estimate change.
5. Open Review and switch the example state to Needs attention. Choose a chat version and a folder.
6. Switch between light and dark themes or Windows and macOS styling.

Only sample values in the open page change. Refreshing resets them. Folder picking, diagnostics, and restore actions demonstrate the proposed flow; they do not access real folders or start the sync engine.

## Approved implementation contract

Use B Overview and C What to sync renders as the layout contract. The unselected Review and Settings concepts are not implementation requirements. Map the controls to WinUI 3 NavigationView, Button, MenuFlyout, ComboBox, ListView, native folder pickers, and theme resources. Use the macOS system font and window treatment for the Mac implementation.

Keep native control states, accessible names, keyboard navigation, text scaling, high contrast, and virtualization. A browser preview cannot establish native UI Automation behavior, Windows DPI handling, VoiceOver support, or real transfer correctness. Those need checks in the final app.

The approved production tokens and layout rules are recorded in [DESIGN.md](../../DESIGN.md) and [.impeccable/design.json](../../.impeccable/design.json). The prototype values below remain historical concept evidence.

## Reproduce the concept

From the repository root:

```powershell
node docs/ux-directions-2026-09/build.mjs
node docs/ux-directions-2026-09/render.mjs
```

The build writes a self-contained preview.html. The renderer uses the existing local Playwright installation and headless Chrome. It opens only the sample prototype and writes screenshots and a check report to docs/design/ux-2026-09.

The icon is the existing user-approved sail artwork. Interface icons are rendered from the repository's Lucide dependency. All concept images are screenshots of the authored prototype; no generated illustration or personal task content is used.
## Verification

The final concept pass checked 60 combinations of page, window size, and theme, plus 15 interaction checks. There were no JavaScript errors, horizontal layout overflows, unnamed visible controls, or failing interaction checks. The accent hover check waits for the 140ms color transition to finish.

The independent finish review marked its three requested corrections resolved: Pull progress/completion, incoming-handoff status, and selection-dependent review content. That verdict covers those corrections and this concept deliverable, not the production native app.

The Mac release script passed a Bash syntax check. The obsolete Intel target and artifact label were both rejected before any build could start. No app build, app installation, real transfer, or visible application launch was performed during that concept exploration.
## Production implementation record

The independent finish review accepted the B/C implementation after inspecting 16 captures: eight offscreen native Windows captures and eight headless Mac-styled React captures, across light, dark, wide, and narrow layouts. No material visual divergence remained. A stale inspector size during delayed or failed estimation was corrected. Evidence is summarized in the [test record](../testing-1.6.0.md) and [image index](../images/README.md).

Verification passed: 59 offscreen native checks, 27 React tests, 13 native-client checks, the Windows build, TypeScript/Vite, and desktop Cargo checks. This evidence does not establish live sync or real Mac runtime UI behavior. New Mac builds target Apple Silicon only; Intel 1.5.2 is archived. Sparse packaging is not implemented.
