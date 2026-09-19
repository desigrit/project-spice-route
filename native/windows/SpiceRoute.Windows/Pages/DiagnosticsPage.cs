using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using System.Text.Json;
using System.Text.Json.Nodes;
using Windows.Storage;
using Windows.Storage.Pickers;
using WinRT.Interop;

namespace SpiceRoute.Windows;

public sealed class DiagnosticsPage : Page
{
    private readonly SpiceRouteContext context;
    private readonly Button run = Ui.Button("Run checks", "\uE72C");
    private readonly Button export = Ui.Button("Export log…", "\uE74E", true);
    private readonly ProgressBar progress = new() { IsIndeterminate = true, Height = 3, Visibility = Visibility.Collapsed };
    private readonly TextBlock status = Ui.Muted("", 12);
    private readonly StackPanel findings = new() { Spacing = 0 };
    private readonly InfoBar error = new() { IsOpen = false, IsClosable = true, Severity = InfoBarSeverity.Error };
    private readonly CancellationTokenSource lifetime = new();
    private JsonObject? report;
    private bool running;

    public DiagnosticsPage(SpiceRouteContext context)
    {
        this.context = context;
        var back = Ui.TextButton("Back to Recovery", "\uE72B");
        back.Click += (_, _) => context.Navigate("recovery");
        var page = NativePageUi.PageGrid("Diagnostics", back, out var content);
        content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        content.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        var intro = new StackPanel { Spacing = 12, Margin = new Thickness(0, 0, 0, 16) };
        intro.Children.Add(Ui.Text("Missing chats after a Pull?", 16, true));
        intro.Children.Add(Ui.Muted("Check the restored history, Codex data folders, and recent pull results. You can run these checks while Codex is open.", 13));
        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        actions.Children.Add(run); actions.Children.Add(export); intro.Children.Add(actions);
        intro.Children.Add(Ui.Muted("The log includes versions, folder locations, record counts, and pull events. Conversation text and credentials are excluded.", 12));
        intro.Children.Add(progress); intro.Children.Add(error);
        AutomationProperties.SetLiveSetting(status, Microsoft.UI.Xaml.Automation.Peers.AutomationLiveSetting.Polite);
        intro.Children.Add(status);
        content.Children.Add(intro);
        var scroll = new ScrollViewer { Content = findings, HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
        Grid.SetRow(scroll, 1); content.Children.Add(scroll);
        Content = page;
        export.IsEnabled = false;
        run.Click += async (_, _) => await RunAsync();
        export.Click += async (_, _) => await ExportAsync();
        Loaded += async (_, _) => await RunAsync();
        Unloaded += (_, _) => lifetime.Cancel();
    }

    private async Task RunAsync()
    {
        if (running || lifetime.IsCancellationRequested) return;
        running = true; run.IsEnabled = export.IsEnabled = false;
        report = null;
        error.IsOpen = false; progress.Visibility = Visibility.Visible;
        status.Text = "Checking restored history and Codex folders…";
        try
        {
            var result = await context.Engine.CallAsync("get_diagnostics_report", new() { ["config"] = context.Config.DeepClone() }, lifetime.Token);
            if (lifetime.IsCancellationRequested) return;
            report = result as JsonObject ?? throw new InvalidDataException("The diagnostic report could not be read. Try running the checks again.");
            findings.Children.Clear();
            var profile = Wire.Object(Wire.Object(report, "report"), "configuredProfile");
            if (profile.Count > 0)
            {
                var counts = Wire.Object(Wire.Object(profile, "stateDatabase"), "counts");
                var history = Wire.Object(Wire.Object(profile, "historyDatabase"), "counts");
                var snapshot = new StackPanel { Spacing = 5, Margin = new Thickness(0, 8, 0, 14) };
                snapshot.Children.Add(Ui.Text("Pull destination", 14, true));
                var path = Ui.Muted(Wire.Text(profile, "canonicalPath", Wire.Text(profile, "path")), 13);
                path.IsTextSelectionEnabled = true;
                snapshot.Children.Add(path);
                snapshot.Children.Add(Ui.Muted($"Chats: {CountLabel(counts, "threads")} · Projects: {CountLabel(counts, "projects")} · History items: {CountLabel(history, "thread_items")}", 13));
                if (Wire.Bool(Wire.Object(profile, "stateDatabase"), "countsLimited") || Wire.Bool(Wire.Object(profile, "historyDatabase"), "countsLimited"))
                    snapshot.Children.Add(Ui.Muted("Large record counts are capped to keep these checks quick. See the log for the limits.", 12));
                findings.Children.Add(snapshot);
            }
            foreach (var finding in Wire.Array(report, "findings").OfType<JsonObject>())
            {
                var severity = Wire.Text(finding, "severity");
                var row = Ui.ColumnsWithSpacing(12, new GridLength(22), new GridLength(1, GridUnitType.Star));
                row.Padding = new Thickness(0, 14, 0, 14);
                var icon = Ui.Icon(severity is "warning" or "error" ? "\uE7BA" : "\uE946", 17);
                icon.Style = Ui.Style("SpiceMutedIconStyle"); icon.VerticalAlignment = VerticalAlignment.Top;
                icon.Margin = new Thickness(0, 2, 0, 0); Ui.Add(row, icon);
                var text = new StackPanel { Spacing = 4 };
                text.Children.Add(Ui.Text(Wire.Text(finding, "title"), 14, true));
                text.Children.Add(Ui.Muted(Wire.Text(finding, "detail"), 13));
                Ui.Add(row, text, column: 1);
                if (findings.Children.Count > 0) findings.Children.Add(Ui.Rule());
                findings.Children.Add(row);
            }
            status.Text = Wire.Text(report, "summary", "Checks finished. Export the log to investigate the missing history.");
        }
        catch (OperationCanceledException) { }
        catch (Exception exception)
        {
            if (lifetime.IsCancellationRequested) return;
            error.Message = exception.Message; error.IsOpen = true;
            status.Text = "Checks could not finish. Your Codex data was not changed.";
        }
        finally
        {
            running = false;
            progress.Visibility = Visibility.Collapsed;
            run.IsEnabled = true;
            export.IsEnabled = report is not null;
        }
    }

    private static string CountLabel(JsonObject counts, string key)
        => counts[key] is null ? "Unavailable" : Wire.Number(counts, key).ToString("N0");

    private async Task ExportAsync()
    {
        if (report is null || running) return;
        export.IsEnabled = false;
        error.IsOpen = false;
        try
        {
            var picker = new FileSavePicker
            {
                SuggestedStartLocation = PickerLocationId.DocumentsLibrary,
                SuggestedFileName = $"spice-route-diagnostics-{DateTime.Now:yyyyMMdd-HHmmss}"
            };
            picker.FileTypeChoices.Add("Diagnostic report", new List<string> { ".json" });
            InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(context.Window));
            var file = await picker.PickSaveFileAsync();
            if (file is null) return;
            var exported = (JsonObject)report.DeepClone();
            exported["appVersion"] = typeof(DiagnosticsPage).Assembly.GetName().Version?.ToString();
            await FileIO.WriteTextAsync(file, exported.ToJsonString(new JsonSerializerOptions { WriteIndented = true }));
            status.Text = "Log exported. Attach this JSON file when reporting the missing history.";
        }
        catch (Exception exception) { error.Message = exception.Message; error.IsOpen = true; }
        finally { export.IsEnabled = report is not null; }
    }
}
