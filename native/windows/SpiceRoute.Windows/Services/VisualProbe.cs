using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
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
        [{"id":"20260918T173000-workspace","shortId":"20260918.1030-A81B72C3","createdAt":"2026-09-18T17:30:00Z","deviceName":"Workspace PC","verified":true},
         {"id":"20260917T154000-laptop","shortId":"20260917.0840-D32A90B1","createdAt":"2026-09-17T15:40:00Z","deviceName":"Travel laptop","verified":true}]
        """)!.AsArray();
    internal JsonObject Status { get; }
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
    internal JsonNode? Respond(string method, JsonObject? parameters)
    {
        Calls.Add(method);
        return method switch
        {
            "list_snapshots" => Snapshots,
            "list_content" or "list_content_quick" => Catalog,
            "load_config" => Config,
            "discover_environment" => Environment,
            "get_sync_status" => Status,
            _ => throw new InvalidOperationException($"Visual probe blocked engine method: {method}")
        };
    }
}

internal static class VisualProbe
{
    internal static async Task<bool> RunAsync(MainWindow window, string outputDirectory)
    {
        var captures = new JsonArray();
        var checks = new JsonArray();
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
                foreach (var page in new[] { "overview", "selection", "settings" })
                {
                    window.ShowVisualProbePage(page, theme);
                    await SettleAsync(root);
                    var name = $"{themeName}-{size.Name}-{page}.png";
                    var image = await CaptureAsync(root, outputDirectory, name);
                    captures.Add(new JsonObject { ["file"] = name, ["width"] = image.Width, ["height"] = image.Height });
                    if (page == "overview" && size.Name == "wide")
                        await CheckButtonStatesAsync(root, themeName, outputDirectory, checks);
                    if (page == "selection")
                        await CheckSelectionAsync(root, themeName, size.Name, outputDirectory, checks);
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
        var row = projectMode.Parent as Grid ?? throw new InvalidOperationException("The project mode has no row.");
        var metadata = Descendants(row).OfType<TextBlock>().FirstOrDefault(text => text.Text.Contains("chats · ", StringComparison.Ordinal))
            ?? throw new InvalidOperationException("The project size fixture was not rendered.");
        var before = metadata.Text;
        projectMode.SelectedIndex = 1;
        await SettleAsync(root);
        var save = Descendants(root).OfType<Button>().FirstOrDefault(button => AutomationProperties.GetName(button) == "Save choices");
        var after = metadata.Text;
        var filename = $"{theme}-selection-history-only.png";
        await CaptureAsync(root, directory, filename);
        checks.Add(new JsonObject
        {
            ["check"] = $"{theme} changing Full project to Chat history only updates size and enables Save",
            ["passed"] = before != after && after.EndsWith(NativePageUi.Bytes(3220000d), StringComparison.Ordinal) && save?.IsEnabled == true,
            ["before"] = before, ["after"] = after, ["saveEnabled"] = save?.IsEnabled == true, ["screenshot"] = filename
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
