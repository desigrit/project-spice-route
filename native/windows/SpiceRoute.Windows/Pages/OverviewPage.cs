using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Text.Json.Nodes;

namespace SpiceRoute.Windows;

public sealed class OverviewPage : Page
{
    private readonly SpiceRouteContext context;
    private readonly StackPanel body = Ui.Stack(20);
    private readonly InfoBar error = new() { IsClosable = true, Severity = InfoBarSeverity.Error };
    private readonly ProgressBar progress = new() { IsIndeterminate = true, Visibility = Visibility.Collapsed };
    private bool refreshing;
    private bool active;
    private bool readingHistory;
    private JsonArray? recentSnapshots;
    private string recentKey = "";
    private string? historyError;

    public OverviewPage(SpiceRouteContext context)
    {
        this.context = context;
        Content = new ScrollViewer { Content = body, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
        Loaded += OnLoaded;
        Unloaded += (_, _) => { active = false; context.StateChanged -= Render; };
        Render();
    }
    private async void OnLoaded(object sender, RoutedEventArgs args) { active = true; context.StateChanged += Render; await LoadRecentAsync(); }
    private void Render()
    {
        body.Children.Clear();
        var heading = Ui.Columns(new GridLength(1, GridUnitType.Star), GridLength.Auto);
        Ui.Add(heading, Ui.Text("Overview", 28, true));
        var refresh = Ui.Button("Refresh", "\uE72C");
        refresh.IsEnabled = !refreshing;
        refresh.Click += async (_, _) =>
        {
            refreshing = true; recentKey = ""; refresh.IsEnabled = false; progress.Visibility = Visibility.Visible; error.IsOpen = false;
            try { await context.RefreshAsync(); }
            catch (Exception exception) { error.Message = exception.Message; error.IsOpen = true; }
            finally { refreshing = false; progress.Visibility = Visibility.Collapsed; refresh.IsEnabled = true; }
        };
        Ui.Add(heading, refresh, column: 1);
        body.Children.Add(heading);
        body.Children.Add(error);
        body.Children.Add(progress);
        var commands = new CommandBar { DefaultLabelPosition = CommandBarDefaultLabelPosition.Right, HorizontalAlignment = HorizontalAlignment.Left, IsOpen = false };
        var push = new AppBarButton { Label = "Push", Icon = new SymbolIcon(Symbol.Upload) };
        var pull = new AppBarButton { Label = "Pull latest", Icon = new SymbolIcon(Symbol.Download) };
        var ready = Wire.Bool(Wire.Object(context.Environment, "compatibility"), "supported") && Wire.Bool(context.Config, "onboardingComplete") && !Wire.Bool(context.Status, "pendingRecovery");
        var heads = Wire.Array(context.Status, "visibleHeads");
        push.IsEnabled = ready && (heads.Count <= 1 || Wire.Bool(context.Status, "mergeReady"));
        pull.IsEnabled = ready && context.Status["latestSnapshot"] is JsonObject;
        push.Click += (_, _) => BeginReview("push");
        pull.Click += (_, _) => BeginReview("pull");
        commands.PrimaryCommands.Add(push); commands.PrimaryCommands.Add(pull);
        body.Children.Add(commands);
        body.Children.Add(Ui.Rule());

        var columns = Ui.Columns(new GridLength(1, GridUnitType.Star), new GridLength(280));
        var device = Ui.Stack(18);
        device.Children.Add(Ui.Text("This computer", 20, true));
        var identity = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 14 };
        identity.Children.Add(Ui.Icon("\uE7F4", 30));
        identity.Children.Add(Ui.Text(Wire.Text(context.Config, "deviceName", "This PC"), 20, true));
        device.Children.Add(identity);
        var state = Wire.Text(context.Status, "message", "Connect a cloud folder to begin.");
        device.Children.Add(Ui.Text(state));
        if (!ready)
        {
            var explanation = Wire.Text(Wire.Object(context.Environment, "compatibility"), "explanation");
            if (explanation.Length > 0) device.Children.Add(Ui.Text(explanation, 13));
            var settings = Ui.Button(Wire.Bool(context.Status, "pendingRecovery") ? "Review recovery" : "Check settings");
            settings.Click += (_, _) => context.Navigate(Wire.Bool(context.Status, "pendingRecovery") ? "recovery" : "settings");
            device.Children.Add(settings);
        }
        device.Children.Add(Ui.Rule());
        device.Children.Add(Ui.Text("Included in the next handoff", 15, true));
        var selection = Wire.Object(context.Config, "selection");
        var modes = Wire.Object(selection, "projectModes");
        var excluded = Wire.Array(selection, "excludedThreadIds").Select(node => node?.ToString()).ToHashSet();
        var threads = Wire.Array(context.Catalog, "threads").OfType<JsonObject>().Count(thread => !excluded.Contains(Wire.Text(thread, "id")) && (Wire.Bool(selection, "includeArchived", true) || !Wire.Bool(thread, "archived")) && (Wire.Text(thread, "projectId") == "" || (modes[Wire.Text(thread, "projectId")]?.ToString() ?? Wire.Text(selection, "defaultProjectMode", "full")) != "excluded"));
        var projects = Wire.Array(context.Catalog, "projects").OfType<JsonObject>();
        var full = projects.Count(project => (modes[Wire.Text(project, "id")]?.ToString() ?? Wire.Text(selection, "defaultProjectMode", "full")) == "full");
        var history = projects.Count(project => (modes[Wire.Text(project, "id")]?.ToString() ?? Wire.Text(selection, "defaultProjectMode", "full")) == "historyOnly");
        device.Children.Add(Ui.Text($"{threads} chats", 16, true));
        device.Children.Add(Ui.Text($"{full} full projects" + (history > 0 ? $" · {history} with chat history only" : ""), 16, true));
        device.Children.Add(Ui.Text("Selected files, conversations, and working changes.", 13));
        var change = Ui.Button("Change selection"); change.Click += (_, _) => context.Navigate("selection"); device.Children.Add(change);
        Ui.Add(columns, device);
        var cloud = Ui.Stack(14);
        cloud.Children.Add(Ui.Text("Cloud folder", 16, true));
        cloud.Children.Add(Ui.Text(Wire.Bytes(Wire.Number(context.Status, "cloudBytes")) + " stored"));
        cloud.Children.Add(Ui.Rule());
        cloud.Children.Add(Ui.Text("Latest visible handoff", 15, true));
        if (context.Status["latestSnapshot"] is JsonObject latest)
        {
            cloud.Children.Add(Ui.Text(Wire.Time(Wire.Text(latest, "createdAt")), 16, true));
            cloud.Children.Add(Ui.Text(Wire.Text(latest, "deviceName")));
            cloud.Children.Add(Ui.Text(Wire.Text(latest, "shortId"), 12));
            cloud.Children.Add(Ui.Text(Wire.Bool(latest, "verified") ? "Received and verified" : "Visible in the sync folder. Contents are checked during Pull.", 13));
        }
        else cloud.Children.Add(Ui.Text(heads.Count > 1 ? "Several branches need review. Choose one below." : "Nothing published yet. Push from this PC to create the first handoff."));
        var panel = (Border)Microsoft.UI.Xaml.Markup.XamlReader.Load("<Border xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' Background='{ThemeResource SpiceSubtle}' Padding='22' CornerRadius='8'/>");
        panel.Child = cloud;
        Ui.Add(columns, panel, column: 1); body.Children.Add(columns);
        body.Children.Add(Ui.Text("Your drive client handles delivery. Match the handoff on your next computer.", 12));
        body.Children.Add(Ui.Rule());
        body.Children.Add(Ui.Text("Recent handoffs", 17, true));
        if (recentSnapshots is null) body.Children.Add(Ui.Text(historyError ?? "Loading visible handoffs…", 13));
        else if (recentSnapshots.Count == 0) body.Children.Add(Ui.Text("Published handoffs will appear here.", 13));
        else foreach (var handoff in recentSnapshots.OfType<JsonObject>().OrderByDescending(item => Wire.Text(item, "createdAt")).Take(3))
        {
            var line = Ui.Columns(new GridLength(1, GridUnitType.Star), new GridLength(1, GridUnitType.Star), GridLength.Auto);
            Ui.Add(line, Ui.Text(Wire.Time(Wire.Text(handoff, "createdAt")), 12));
            Ui.Add(line, Ui.Text(Wire.Text(handoff, "deviceName"), 12), column: 1);
            Ui.Add(line, Ui.Text(Wire.Text(handoff, "id") == Wire.Text(context.Status, "lastAppliedSnapshotId") ? "Current baseline" : "Visible in cloud folder", 12), column: 2);
            body.Children.Add(line);
        }
        if (heads.Count > 1)
        {
            body.Children.Add(Ui.Text("Visible branches", 18, true));
            foreach (var head in heads.OfType<JsonObject>())
            {
                var branch = Ui.Button($"Review {Wire.Text(head, "deviceName")} · {Wire.Time(Wire.Text(head, "createdAt"))}");
                branch.IsEnabled = ready;
                branch.Click += (_, _) => BeginReview("pull", Wire.Text(head, "id"));
                body.Children.Add(branch);
            }
        }
        body.Children.Add(new Border { Height = 24 });
        if (active) _ = LoadRecentAsync();
    }
    private async Task LoadRecentAsync()
    {
        var key = Wire.Text(context.Config, "cloudRoot") + "|" + Wire.Text(Wire.Object(context.Status, "latestSnapshot"), "id");
        if (!active || readingHistory || key == recentKey || !Wire.Bool(context.Config, "onboardingComplete")) return;
        readingHistory = true;
        try
        {
            var result = await context.Engine.CallAsync("list_snapshots", new() { ["config"] = context.Config.DeepClone() });
            if (!active) return;
            var currentKey = Wire.Text(context.Config, "cloudRoot") + "|" + Wire.Text(Wire.Object(context.Status, "latestSnapshot"), "id");
            if (key != currentKey) return;
            recentSnapshots = result as JsonArray ?? new JsonArray(); recentKey = key; historyError = null;
        }
        catch (Exception exception) { if (active) { historyError = "Recent handoffs could not load. " + exception.Message; recentKey = key; } }
        finally { readingHistory = false; if (active) Render(); }
    }
    private void BeginReview(string direction, string? snapshotId = null)
    {
        context.CurrentPreview = null; context.ReviewDirection = direction; context.ReviewSnapshotId = snapshotId; context.Navigate("review");
    }
}
