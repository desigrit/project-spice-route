using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using System.Text.Json;
using System.Text.Json.Nodes;
using Windows.Graphics;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;

namespace SpiceRoute.Windows;

// An opt-in, offscreen rendering harness. All data is in memory, and every
// unrecognized engine request fails instead of starting a real sync process.
internal sealed class VisualProbeFixture
{
    internal static event Action<string>? RequestObserved;
    internal JsonObject Config { get; } = JsonNode.Parse("""
        {"deviceName":"Workspace PC","onboardingComplete":true,"theme":"light","cloudProvider":"oneDrive","cloudRoot":"C:\\VisualProbe\\OneDrive\\Spice Route","codexHome":"C:\\VisualProbe\\.codex","projectlessRoot":"C:\\VisualProbe\\Chats","projectsRoot":"C:\\VisualProbe\\Projects","sourceRoots":{},"destinationRoots":{},"selection":{"defaultProjectMode":"historyOnly","projectModes":{"spice":"full","archive":"excluded"},"excludedThreadIds":[],"includeArchived":true,"includeSensitiveFiles":true,"includeBuildOutputs":false,"extraExcludePatterns":[],"revision":"visual-probe"}}
        """)!.AsObject();
    internal JsonObject Environment { get; } = JsonNode.Parse("""
        {"codexVersion":"26.908.40834","compatibility":{"supported":true,"explanation":"This Codex format is supported for Push and Pull."}}
        """)!.AsObject();
    internal JsonObject Catalog { get; } = JsonNode.Parse("""
        {"projects":[
          {"id":"spice","name":"Project Spice Route","roots":["D:\\Code\\Project Spice Route"],"localRoots":["D:\\Code\\Project Spice Route"],"estimatedBytes":23500000},
          {"id":"design","name":"Design system","roots":["C:\\Users\\Alex\\Work\\Design system"],"estimatedBytes":17300000},
          {"id":"research","name":"Research and notes","roots":["E:\\Work\\Research","E:\\Work\\References"],"estimatedBytes":28600000},
          {"id":"archive","name":"Archived experiments","roots":["D:\\Code\\Archive"],"estimatedBytes":97000000}],
         "threads":[
          {"id":"chat-1","title":"Improve the Windows app","projectId":"spice","projectless":false,"estimatedBytes":2300000},
          {"id":"chat-2","title":"Review the next handoff","projectId":"spice","projectless":false,"estimatedBytes":920000},
          {"id":"chat-3","title":"Build a reusable color palette","projectId":"design","projectless":false,"estimatedBytes":1460000},
          {"id":"chat-4","title":"Summarize the research","projectId":"research","projectless":false,"estimatedBytes":620000},
          {"id":"chat-5","title":"Earlier experiments","projectId":"archive","projectless":false,"archived":true,"estimatedBytes":180000},
          {"id":"chat-6","title":"Plan the weekend","projectless":true,"estimatedBytes":760000},
          {"id":"chat-7","title":"Compare a few ideas","projectless":true,"estimatedBytes":180000}]}
        """)!.AsObject();
    internal JsonArray Snapshots { get; } = JsonNode.Parse("""
        [{"id":"20260918T173000-workspace","shortId":"20260918.1030-A81B72C3","createdAt":"2026-09-18T17:30:00Z","deviceName":"Workspace PC","logicalBytes":29580000,"storedBytes":18430000,"objectCount":126,"verified":true},
         {"id":"20260917T154000-laptop","shortId":"20260917.0840-D32A90B1","createdAt":"2026-09-17T15:40:00Z","deviceName":"Travel laptop","logicalBytes":26780000,"storedBytes":16120000,"objectCount":121,"verified":true}]
        """)!.AsArray();
    internal JsonObject Status { get; }
    internal JsonArray ReviewChanges { get; } = JsonNode.Parse("""
        [
          {"key":"project:spice","kind":"project","label":"Project Spice Route","detail":"Project configuration and saved folder mapping","action":"update","bytes":0},
          {"key":"project:design","kind":"project","label":"Design system and reusable components","detail":"Project configuration and saved folder mapping","action":"add","bytes":0},
          {"key":"project:research","kind":"project","label":"Research and product discovery notes","detail":"Project configuration and saved folder mapping","action":"unchanged","bytes":0},
          {"key":"file:projects/spice/0/assets/windows-app-preview.png","kind":"file","label":"windows-app-preview.png","detail":"Project Spice Route · assets / windows-app-preview.png","action":"update","bytes":12400000},
          {"key":"file:projects/spice/0/assets/onboarding-walkthrough.mp4","kind":"file","label":"onboarding-walkthrough.mp4","detail":"Project Spice Route · assets / onboarding-walkthrough.mp4","action":"add","bytes":8350000},
          {"key":"file:projects/spice/0/native/windows/Assets/application-icon-source.png","kind":"file","label":"application-icon-source.png","detail":"Project Spice Route · native / windows / Assets / application-icon-source.png","action":"update","bytes":2712300},
          {"key":"file:projects/spice/0/docs/windows-release-checklist.md","kind":"file","label":"windows-release-checklist.md","detail":"Project Spice Route · docs / windows-release-checklist.md","action":"unchanged","bytes":37700},
          {"key":"file:projects/design/0/exports/component-library-typography-and-color-reference.pdf","kind":"file","label":"component-library-typography-and-color-reference.pdf","detail":"Design system and reusable components · exports / component-library-typography-and-color-reference.pdf","action":"add","bytes":15800000},
          {"key":"file:projects/design/0/assets/navigation-states.png","kind":"file","label":"navigation-states.png","detail":"Design system and reusable components · assets / navigation-states.png","action":"update","bytes":1456200},
          {"key":"file:projects/design/0/tokens/legacy-theme.json","kind":"file","label":"legacy-theme.json","detail":"Design system and reusable components · tokens / legacy-theme.json","action":"delete","bytes":43800},
          {"key":"file:projects/research/0/interviews/customer-interview-synthesis-september.pdf","kind":"file","label":"customer-interview-synthesis-september.pdf","detail":"Research and product discovery notes · interviews / customer-interview-synthesis-september.pdf","action":"update","bytes":26800000},
          {"key":"file:projects/research/0/notes/competitive-analysis.xlsx","kind":"file","label":"competitive-analysis.xlsx","detail":"Research and product discovery notes · notes / competitive-analysis.xlsx","action":"unchanged","bytes":1755000},
          {"key":"file:projects/research/0/notes/product-discovery-plan.md","kind":"file","label":"product-discovery-plan.md","detail":"Research and product discovery notes · notes / product-discovery-plan.md","action":"add","bytes":45000},
          {"key":"thread:chat-1","kind":"thread","label":"Improve the Windows app and review its navigation states","detail":"Project Spice Route · Conversation history","action":"update","bytes":2300000},
          {"key":"thread:chat-2","kind":"thread","label":"Review the next handoff","detail":"Project Spice Route · Conversation history","action":"unchanged","bytes":920000},
          {"key":"thread:chat-3","kind":"thread","label":"Build a reusable color palette with accessible contrast","detail":"Design system and reusable components · Conversation history","action":"add","bytes":1460000},
          {"key":"thread:chat-4","kind":"thread","label":"Summarize the research","detail":"Research and product discovery notes · Conversation history","action":"update","bytes":620000},
          {"key":"thread:chat-6","kind":"thread","label":"Plan the weekend","detail":"Chats without a project · Conversation history","action":"update","bytes":760000},
          {"key":"thread:chat-7","kind":"thread","label":"Compare a few ideas for the next personal project","detail":"Chats without a project · Conversation history","action":"unchanged","bytes":180000}
        ]
        """)!.AsArray();
    internal List<string> Calls { get; } = new();
    internal VisualProbeFixture()
    {
        Status = new JsonObject
        {
            ["message"] = "This device is aligned with the latest visible snapshot.",
            ["cloudBytes"] = 48900000d,
            ["pendingRecovery"] = false,
            ["lastAppliedSnapshotId"] = "20260918T173000-workspace",
            ["latestSnapshot"] = Snapshots[0]!.DeepClone(),
            ["visibleHeads"] = new JsonArray(Snapshots[0]!.DeepClone())
        };
    }
    internal static Task? ContentReadGate;
    internal static bool FailContentRead;
    internal async Task<JsonNode?> RespondAsync(string method, JsonObject? parameters)
    {
        if (method == "list_content")
        {
            if (ContentReadGate is not null) await ContentReadGate;
            if (FailContentRead) throw new IOException("The sample folder is unavailable.");
        }
        return Respond(method, parameters);
    }
    private JsonNode? Respond(string method, JsonObject? parameters)
    {
        Calls.Add(method);
        RequestObserved?.Invoke(method);
        return method switch
        {
            "list_snapshots" => Snapshots,
            "list_content" or "list_content_quick" => Catalog,
            "load_config" => Config,
            "discover_environment" => Environment,
            "get_sync_status" => Status,
            "preview_push" => Preview("push"),
            "preview_pull" => Preview("pull"),
            "cancel_operation" when Wire.Text(parameters, "operationId") is "visual-probe-push" or "visual-probe-pull" => null,
            "get_diagnostics_report" => JsonNode.Parse("""
                {"schemaVersion":1,"generatedAt":"2026-09-18T17:30:00Z","summary":"The selected profile is readable. One path needs review.","findings":[{"severity":"warning","title":"Profile path differs from the discovered profile","detail":"The saved path points to C:\\VisualProbe\\.codex. Confirm the intended profile before changing it."},{"severity":"info","title":"Conversation databases are readable","detail":"Found 7 threads, 4 projects, and 128 history items."}],"report":{"configuredProfile":{"path":"C:\\VisualProbe\\.codex","canonicalPath":"C:\\VisualProbe\\.codex","stateDatabase":{"counts":{"threads":7,"projects":4}},"historyDatabase":{"counts":{"thread_items":128}}}}}
                """),
            _ => throw new InvalidOperationException($"Visual probe blocked engine method: {method}")
        };
    }

    private JsonObject Preview(string direction) => new()
    {
        ["operationId"] = "visual-probe-" + direction,
        ["direction"] = direction,
        ["snapshotId"] = "20260918T173000-workspace",
        ["changes"] = ReviewChanges.DeepClone(),
        // Match the engine's transfer estimate: unchanged and deleted rows do not transfer bytes.
        ["estimatedBytes"] = ReviewChanges.OfType<JsonObject>().Where(change => Wire.Text(change, "action") is not ("unchanged" or "delete")).Sum(change => Wire.Number(change, "bytes")),
        ["warnings"] = new JsonArray("Full projects include complete Git history and selected working files."),
        ["blockedReasons"] = new JsonArray(),
        ["requiredMappings"] = new JsonArray(),
        ["requiresCodexClose"] = false,
        ["replacesCloudHistory"] = false
    };
}

internal static class VisualProbe
{
    internal static async Task<bool> RunAsync(MainWindow window, string outputDirectory)
    {
        var captures = new JsonArray();
        var checks = new JsonArray();
        var methodCalls = new List<string>();
        VisualProbeFixture.RequestObserved += methodCalls.Add;
        var root = window.VisualProbeRoot;
        window.AppWindow.IsShownInSwitchers = false;
        window.AppWindow.MoveAndResize(new RectInt32(-32000, -32000, 1180, 820));
        window.ShowVisualProbePage("overview", ElementTheme.Light);
        window.AppWindow.Show(false);
        await SettleAsync(root);

        foreach (var theme in new[] { ElementTheme.Light, ElementTheme.Dark })
        {
            var themeName = theme.ToString().ToLowerInvariant();
            foreach (var size in new[] { (Name: "wide", Width: 1180), (Name: "narrow", Width: 760) })
            {
                window.AppWindow.MoveAndResize(new RectInt32(-32000, -32000, size.Width, 820));
                foreach (var page in new[] { "overview", "selection", "settings", "review", "diagnostics" })
                {
                    if (page == "diagnostics" && size.Name != "wide") continue;
                    var firstRequest = methodCalls.Count;
                    window.ShowVisualProbePage(page, theme);
                    await SettleAsync(root);
                    var name = $"{themeName}-{size.Name}-{page}.png";
                    var image = await CaptureAsync(root, outputDirectory, name);
                    captures.Add(new JsonObject { ["file"] = name, ["width"] = image.Width, ["height"] = image.Height });
                    if (page == "overview" && size.Name == "wide")
                        await CheckButtonStatesAsync(root, themeName, outputDirectory, checks);
                    if (page == "selection")
                        await CheckSelectionAsync(root, themeName, size.Name, outputDirectory, checks);
                    if (page == "review")
                        await CheckReviewAsync(window, themeName, size.Name, outputDirectory, checks);
                    if (page == "diagnostics")
                    {
                        var countText = Descendants(root).OfType<TextBlock>().FirstOrDefault(text => text.Text == "Chats: 7 · Projects: 4 · History items: 128");
                        var diagnosticCalls = methodCalls.Skip(firstRequest).ToList();
                        checks.Add(new JsonObject
                        {
                            ["check"] = $"{themeName} Diagnostics shows the configured profile database counts",
                            ["passed"] = countText is { Visibility: Visibility.Visible } && countText.ActualWidth > 0,
                            ["renderedCounts"] = countText?.Text
                        });
                        checks.Add(new JsonObject
                        {
                            ["check"] = $"{themeName} Diagnostics requests only the read-only report",
                            ["passed"] = diagnosticCalls.Count > 0 && diagnosticCalls.All(method => method == "get_diagnostics_report"),
                            ["methods"] = new JsonArray(diagnosticCalls.Select(method => (JsonNode?)JsonValue.Create(method)).ToArray())
                        });
                    }
                }
            }
        }

        var navigation = window.VisualProbeNavigation;
        var hamburger = Descendants(root).OfType<Button>().FirstOrDefault(button => button.Name is "TogglePaneButton" or "PaneToggleButton");
        checks.Add(new JsonObject
        {
            ["check"] = "Navigation hamburger is present",
            ["passed"] = navigation.IsPaneToggleButtonVisible && hamburger is { Visibility: Visibility.Visible } && hamburger.ActualWidth > 0,
            ["controlName"] = hamburger?.Name ?? "not found"
        });
        var navigationItems = navigation.MenuItems.OfType<NavigationViewItem>().ToList();
        checks.Add(new JsonObject
        {
            ["check"] = "Navigation opens with visible labels in an expanded left pane",
            ["passed"] = navigation.PaneDisplayMode == NavigationViewPaneDisplayMode.Left && navigation.IsPaneOpen && navigationItems.Count > 0 && navigationItems.All(item => item.Visibility == Visibility.Visible && item.ActualWidth > 100 && !string.IsNullOrWhiteSpace(item.Content?.ToString()))
        });
        checks.Add(new JsonObject { ["check"] = "No sync process started", ["passed"] = !window.VisualProbeContext.Engine.HasStartedProcess });
        var passed = checks.OfType<JsonObject>().All(check => Wire.Bool(check, "passed"));
        var report = new JsonObject
        {
            ["passed"] = passed,
            ["mode"] = "Actual WinUI rendering in an offscreen, non-activating window with in-memory fixture data",
            ["captures"] = captures,
            ["checks"] = checks
        };
        await File.WriteAllTextAsync(Path.Combine(outputDirectory, "visual-probe-report.json"), report.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
        VisualProbeFixture.RequestObserved -= methodCalls.Add;
        window.AppWindow.Hide();
        return passed;
    }

    private static async Task CheckButtonStatesAsync(FrameworkElement root, string theme, string directory, JsonArray checks)
    {
        var button = Descendants(root).OfType<Button>().FirstOrDefault(candidate => AutomationProperties.GetName(candidate) == "Push")
            ?? throw new InvalidOperationException("The rendered Overview did not contain a Push button.");
        byte[]? normal = null;
        foreach (var state in new[] { "Normal", "PointerOver", "Pressed", "Disabled" })
        {
            button.IsEnabled = state != "Disabled";
            var transitioned = VisualStateManager.GoToState(button, state, false);
            await SettleAsync(root);
            var image = await CaptureAsync(button, directory, $"{theme}-push-{state.ToLowerInvariant()}.png");
            var color = image.ColorAt(7, image.Height / 2);
            if (state == "Normal") normal = color;
            var preservesAccent = normal is not null && (state == "Disabled" || (color[3] > 240 && ColorDistance(normal, color) < 115));
            checks.Add(new JsonObject
            {
                ["check"] = $"{theme} Push {state}",
                ["passed"] = transitioned && preservesAccent,
                ["sampledColor"] = $"#{color[2]:X2}{color[1]:X2}{color[0]:X2}{color[3]:X2}",
                ["visualStateApplied"] = transitioned
            });
        }
        button.IsEnabled = true;
        VisualStateManager.GoToState(button, "Normal", false);
    }

    private static double ColorDistance(byte[] first, byte[] second)
        => Math.Sqrt(Enumerable.Range(0, 3).Sum(index => Math.Pow(first[index] - second[index], 2)));

    private static async Task CheckSelectionAsync(FrameworkElement root, string theme, string size, string directory, JsonArray checks)
    {
        var modes = Descendants(root).OfType<ComboBox>()
            .Where(control => AutomationProperties.GetName(control).StartsWith("Sync mode for ", StringComparison.Ordinal)).ToList();
        var details = new JsonArray();
        var enabled = modes.Count > 0;
        foreach (var mode in modes)
        {
            var ancestors = new JsonArray();
            var opacity = 1d;
            var chainEnabled = true;
            for (DependencyObject? current = mode; current is not null; current = VisualTreeHelper.GetParent(current))
            {
                if (current is not FrameworkElement element) continue;
                opacity *= element.Opacity;
                if (element is Control control) chainEnabled &= control.IsEnabled;
                var states = new JsonArray();
                foreach (var group in VisualStateManager.GetVisualStateGroups(element))
                    states.Add($"{group.Name}:{group.CurrentState?.Name}");
                ancestors.Add(new JsonObject
                {
                    ["type"] = element.GetType().Name, ["name"] = element.Name,
                    ["opacity"] = element.Opacity,
                    ["isEnabled"] = element is Control item ? item.IsEnabled : null,
                    ["states"] = states
                });
            }
            enabled &= chainEnabled && opacity >= .98;
            details.Add(new JsonObject { ["mode"] = AutomationProperties.GetName(mode), ["effectiveOpacity"] = opacity, ["ancestors"] = ancestors });
        }
        checks.Add(new JsonObject { ["check"] = $"{theme} {size} project controls are enabled and fully opaque", ["passed"] = enabled, ["controls"] = details });
        if (size != "wide") return;

        var projectMode = modes.FirstOrDefault(mode => AutomationProperties.GetName(mode) == "Sync mode for Project Spice Route")
            ?? throw new InvalidOperationException("The project mode fixture was not rendered.");
        var metadata = Descendants(root).OfType<TextBlock>().FirstOrDefault(text => text.Name == "InspectorSelectedSize")
            ?? throw new InvalidOperationException("The inspector size was not rendered.");
        var before = metadata.Text;
        var list = Descendants(root).OfType<ListView>().First(control => AutomationProperties.GetName(control) == "Content to sync");
        var selected = list.SelectedItem;
        projectMode.SelectedIndex = 1;
        await SettleAsync(root);
        var save = Descendants(root).OfType<Button>().FirstOrDefault(button => AutomationProperties.GetName(button) == "Save choices");
        var after = metadata.Text;
        var filename = $"{theme}-selection-history-only.png";
        await CaptureAsync(root, directory, filename);
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} history-only mode updates size and preserves project selection",
            ["passed"] = before != after && after == NativePageUi.Bytes(3220000d) && save?.IsEnabled == true && ReferenceEquals(selected, list.SelectedItem),
            ["before"] = before, ["after"] = after, ["screenshot"] = filename
        });
        var search = Descendants(root).OfType<TextBox>().First(control => AutomationProperties.GetName(control) == "Search sync choices");
        search.Text = "Research";
        await SettleAsync(root);
        checks.Add(new JsonObject { ["check"] = $"{theme} search selects a visible project and keeps all its folder controls", ["passed"] = list.Items.Count == 1 && list.SelectedItem is SyncChoiceRow { Id: "research" } && Descendants(root).OfType<Button>().Count(button => AutomationProperties.GetName(button).Contains("for Research", StringComparison.Ordinal)) == 2 });
        search.Text = "no matching project";
        await SettleAsync(root);
        checks.Add(new JsonObject { ["check"] = $"{theme} empty search clears stale inspector", ["passed"] = list.Items.Count == 0 && !Descendants(root).OfType<ComboBox>().Any(control => AutomationProperties.GetName(control).StartsWith("Sync mode for", StringComparison.Ordinal)) });
        search.Text = "";
        await SettleAsync(root);
        var fullMode = Descendants(root).OfType<ComboBox>().First(control => AutomationProperties.GetName(control) == "Sync mode for Project Spice Route");
        fullMode.SelectedIndex = 0;
        var page = Descendants(root).OfType<SelectionPage>().First();
        var sizeLabel = Descendants(root).OfType<TextBlock>().First(text => text.Name == "InspectorSelectedSize");
        var chosen = list.SelectedItem;
        var gate = new TaskCompletionSource<bool>();
        VisualProbeFixture.ContentReadGate = gate.Task;
        VisualProbeFixture.FailContentRead = true;
        var refresh = (Task)typeof(SelectionPage).GetMethod("LoadSizesAsync", System.Reflection.BindingFlags.Instance | System.Reflection.BindingFlags.NonPublic)!.Invoke(page, null)!;
        await SettleAsync(root);
        await CaptureAsync(root, directory, $"{theme}-selection-size-pending.png");
        checks.Add(new JsonObject { ["check"] = $"{theme} delayed scan clears old table and inspector sizes", ["passed"] = sizeLabel.Text == "Calculating…" && list.SelectedItem is SyncChoiceRow { Size: "Calculating…" } && ReferenceEquals(chosen, list.SelectedItem) });
        gate.SetResult(true);
        await refresh;
        await SettleAsync(root);
        await CaptureAsync(root, directory, $"{theme}-selection-size-unavailable.png");
        checks.Add(new JsonObject { ["check"] = $"{theme} failed scan shows unavailable in table and inspector", ["passed"] = sizeLabel.Text == "Size unavailable" && list.SelectedItem is SyncChoiceRow { Size: "Size unavailable" } && ReferenceEquals(chosen, list.SelectedItem) });
        VisualProbeFixture.ContentReadGate = null;
        VisualProbeFixture.FailContentRead = false;
    }

    private static async Task CheckReviewAsync(MainWindow window, string theme, string size, string directory, JsonArray checks)
    {
        var root = window.VisualProbeRoot;
        var elements = Descendants(root).OfType<FrameworkElement>().ToList();
        var fileList = elements.OfType<ListView>().First(control => AutomationProperties.GetName(control) == "Files and conversations in this handoff");
        var projects = elements.OfType<ListView>().First(control => AutomationProperties.GetName(control) == "Project filter");
        var compactProjects = elements.OfType<ComboBox>().FirstOrDefault(control => control.Name == "ReviewProjectPicker" && control.Visibility == Visibility.Visible);
        var search = elements.OfType<TextBox>().First(control => AutomationProperties.GetName(control) == "Find a file or project");
        var sort = elements.OfType<ComboBox>().First(control => AutomationProperties.GetName(control) == "Sort files");
        var filter = elements.OfType<ComboBox>().First(control => AutomationProperties.GetName(control) == "Filter changes");
        var allRows = fileList.Items.OfType<ReviewChange>().ToList();
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} {size} Review renders mixed fixture content",
            ["passed"] = allRows.Count == 19 && allRows.Select(row => row.ProjectId).Distinct().Count() == 4 && allRows.Any(row => row.Key.StartsWith("thread:", StringComparison.Ordinal)) && allRows.Any(row => row.Key.StartsWith("project:", StringComparison.Ordinal)),
            ["rowCount"] = allRows.Count
        });

        var toolbarControls = new FrameworkElement[] { search, sort, filter };
        var rectangles = toolbarControls.Select(control => Bounds(control, root)).ToList();
        var toolbarFits = rectangles.All(rect => rect.Width >= 80 && rect.Left >= 0 && rect.Right <= root.ActualWidth + 1);
        for (var first = 0; first < rectangles.Count; first++)
            for (var second = first + 1; second < rectangles.Count; second++)
                toolbarFits &= !Overlaps(rectangles[first], rectangles[second]);
        checks.Add(new JsonObject { ["check"] = $"{theme} {size} Review search and filters fit without overlap", ["passed"] = toolbarFits });

        var alignment = new JsonArray();
        var aligned = true;
        foreach (var (headerName, rowName, rightEdge) in new[]
        {
            ("ReviewNameHeader", "ReviewFileName", false),
            ("ReviewChangeHeader", "ReviewFileChange", false),
            ("ReviewSizeHeader", "ReviewFileSize", true)
        })
        {
            var header = elements.FirstOrDefault(element => element.Name == headerName);
            var cells = elements.Where(element => element.Name == rowName && element.ActualWidth > 0).ToList();
            var differences = header is null ? new List<double>() : cells.Select(cell =>
            {
                var headerRect = Bounds(header, root);
                var cellRect = Bounds(cell, root);
                return Math.Abs(rightEdge ? headerRect.Right - cellRect.Right : headerRect.Left - cellRect.Left);
            }).ToList();
            var passed = differences.Count > 0 && differences.All(difference => difference <= 2);
            aligned &= passed;
            alignment.Add(new JsonObject { ["column"] = headerName, ["passed"] = passed, ["maximumOffset"] = differences.Count > 0 ? differences.Max() : null });
        }
        checks.Add(new JsonObject { ["check"] = $"{theme} {size} Review file columns align with their headers", ["passed"] = aligned, ["columns"] = alignment });

        if (compactProjects is not null) compactProjects.SelectedIndex = 1;
        else projects.SelectedIndex = 1;
        await SettleAsync(root);
        var projectRows = fileList.Items.OfType<ReviewChange>().ToList();
        var projectScreenshot = $"{theme}-{size}-review-project.png";
        await CaptureAsync(root, directory, projectScreenshot);
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} {size} selecting a project filters its files and conversations",
            ["passed"] = projectRows.Count > 0 && projectRows.Count < allRows.Count && projectRows.Select(row => row.ProjectId).Distinct().Count() == 1,
            ["rowCount"] = projectRows.Count, ["screenshot"] = projectScreenshot
        });

        search.Text = "visual-probe-no-matching-file-9481";
        await SettleAsync(root);
        var empty = Descendants(root).OfType<FrameworkElement>().FirstOrDefault(element => element.Name == "ReviewEmptyResults");
        var emptyScreenshot = $"{theme}-{size}-review-empty.png";
        await CaptureAsync(root, directory, emptyScreenshot);
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} {size} unmatched search displays the empty result state",
            ["passed"] = fileList.Items.Count == 0 && empty is { Visibility: Visibility.Visible } && empty.ActualHeight > 0,
            ["screenshot"] = emptyScreenshot
        });

        search.Text = "";
        if (compactProjects is not null) compactProjects.SelectedIndex = 0;
        else projects.SelectedIndex = 0;
        fileList.SelectedIndex = -1;
        await SettleAsync(root);
        if (fileList.ContainerFromIndex(0) is not ListViewItem selected)
            throw new InvalidOperationException("Review did not realize its first file row for the native state probe.");
        var presenter = Descendants(selected).OfType<ListViewItemPresenter>().FirstOrDefault();
        var rowBounds = Bounds(selected, root);
        var unselectedImage = await CaptureAsync(root, directory, $"{theme}-{size}-review-unselected.png");
        fileList.SelectedIndex = 0;
        await SettleAsync(root);
        var selectedScreenshot = $"{theme}-{size}-review-selected.png";
        var selectedImage = await CaptureAsync(root, directory, selectedScreenshot);
        var selectedDifference = RowBackgroundDifference(unselectedImage, selectedImage, rowBounds, root);
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} {size} Review native row Selected state",
            ["passed"] = presenter is not null && selected.IsSelected && fileList.SelectedIndex == 0 && selectedDifference > 1,
            ["selectionApplied"] = selected.IsSelected,
            ["backgroundColorDifference"] = selectedDifference,
            ["screenshot"] = selectedScreenshot
        });

        // The SDK's DefaultListViewItemStyle consists of a ListViewItemPresenter
        // without XAML VisualStateGroups. Its GoToElementStateCore updates native
        // chrome, then the base implementation can return false. Verify the actual
        // rendered background, not that return value. No pointer input is injected.
        var hoverStateReturn = VisualStateManager.GoToState(selected, "PointerOverSelected", false);
        await SettleAsync(root);
        var hoverScreenshot = $"{theme}-{size}-review-pointeroverselected.png";
        var hoverImage = await CaptureAsync(root, directory, hoverScreenshot);
        var hoverDifference = RowBackgroundDifference(selectedImage, hoverImage, rowBounds, root);
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} {size} Review native row PointerOverSelected state",
            ["passed"] = presenter is not null && selected.IsSelected && hoverDifference > 1,
            ["selectionPreserved"] = selected.IsSelected,
            ["backgroundColorDifference"] = hoverDifference,
            ["visualStateManagerReturn"] = hoverStateReturn,
            ["screenshot"] = hoverScreenshot
        });
        VisualStateManager.GoToState(selected, "Selected", false);
        fileList.SelectedIndex = -1;

        if (size != "wide") return;
        var navigation = window.VisualProbeNavigation;
        var menuItems = navigation.MenuItems.OfType<NavigationViewItem>().ToList();
        if (navigation.SettingsItem is NavigationViewItem settings) menuItems.Add(settings);
        var review = elements.OfType<ReviewPage>().FirstOrDefault();
        window.VisualProbeContext.IsBusy = true;
        var disabledWhileBusy = menuItems.Count > 0 && menuItems.All(item => !item.IsEnabled);
        window.VisualProbeContext.Navigate("settings");
        var stayedOnReview = review is not null && Descendants(root).Contains(review);
        var notice = Descendants(root).OfType<InfoBar>().FirstOrDefault(bar => bar.Name == "Notice");
        var noNavigationError = notice is not { IsOpen: true, Severity: InfoBarSeverity.Error };
        window.VisualProbeContext.IsBusy = false;
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} Review busy navigation stays on the page without a persistent error",
            ["passed"] = disabledWhileBusy && stayedOnReview && noNavigationError && menuItems.All(item => item.IsEnabled),
            ["navigationDisabledWhileBusy"] = disabledWhileBusy, ["stayedOnReview"] = stayedOnReview, ["noNavigationError"] = noNavigationError
        });
    }

    private static global::Windows.Foundation.Rect Bounds(FrameworkElement element, FrameworkElement root)
        => element.TransformToVisual(root).TransformBounds(new global::Windows.Foundation.Rect(0, 0, element.ActualWidth, element.ActualHeight));

    private static bool Overlaps(global::Windows.Foundation.Rect first, global::Windows.Foundation.Rect second)
        => first.Left < second.Right - 1 && first.Right > second.Left + 1 && first.Top < second.Bottom - 1 && first.Bottom > second.Top + 1;

    private static double RowBackgroundDifference(Capture before, Capture after, global::Windows.Foundation.Rect row, FrameworkElement root)
    {
        if (before.Width != after.Width || before.Height != after.Height) return 0;
        // Sample the blank top band at three interior positions, avoiding text,
        // rounded corners, the selection indicator, and animated neighboring UI.
        return new[] { .25, .5, .75 }.Average(fraction =>
        {
            var x = (int)Math.Round((row.Left + row.Width * fraction) * after.Width / root.ActualWidth);
            var y = (int)Math.Round((row.Top + 6) * after.Height / root.ActualHeight);
            return ColorDistance(before.ColorAt(x, y), after.ColorAt(x, y));
        });
    }

    private static IEnumerable<DependencyObject> Descendants(DependencyObject parent)
    {
        for (var index = 0; index < VisualTreeHelper.GetChildrenCount(parent); index++)
        {
            var child = VisualTreeHelper.GetChild(parent, index);
            yield return child;
            foreach (var descendant in Descendants(child)) yield return descendant;
        }
    }

    private static async Task SettleAsync(FrameworkElement root)
    {
        root.UpdateLayout();
        // Native list entrance and navigation indicator animations outlast a frame.
        await Task.Delay(450);
        root.UpdateLayout();
        if (root.ActualWidth < 1 || root.ActualHeight < 1)
            throw new InvalidOperationException("Offscreen WinUI layout is unavailable. The probe will not activate a visible window.");
    }

    private sealed record Capture(int Width, int Height, byte[] Pixels)
    {
        internal byte[] ColorAt(int x, int y)
        {
            var offset = (Math.Clamp(y, 0, Height - 1) * Width + Math.Clamp(x, 0, Width - 1)) * 4;
            return Pixels[offset..(offset + 4)];
        }
    }

    private static async Task<Capture> CaptureAsync(FrameworkElement visual, string directory, string name)
    {
        var bitmap = new RenderTargetBitmap();
        await bitmap.RenderAsync(visual);
        if (bitmap.PixelWidth < 1 || bitmap.PixelHeight < 1) throw new InvalidOperationException("The offscreen renderer returned an empty image.");
        var buffer = await bitmap.GetPixelsAsync();
        var pixels = new byte[(int)buffer.Length];
        using (var reader = DataReader.FromBuffer(buffer)) reader.ReadBytes(pixels);
        if (!pixels.Where((_, index) => index % 4 == 3).Any(alpha => alpha > 0))
            throw new InvalidOperationException("The offscreen renderer returned a transparent image. No visible activation was attempted.");
        var folder = await StorageFolder.GetFolderFromPathAsync(Path.GetFullPath(directory));
        var file = await folder.CreateFileAsync(name, CreationCollisionOption.ReplaceExisting);
        using var stream = await file.OpenAsync(FileAccessMode.ReadWrite);
        var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, stream);
        encoder.SetPixelData(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied, (uint)bitmap.PixelWidth, (uint)bitmap.PixelHeight, 96, 96, pixels);
        await encoder.FlushAsync();
        return new Capture(bitmap.PixelWidth, bitmap.PixelHeight, pixels);
    }
}
