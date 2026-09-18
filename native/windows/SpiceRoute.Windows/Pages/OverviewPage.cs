using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using System.Globalization;
using System.Text.Json.Nodes;

namespace SpiceRoute.Windows;

public sealed class OverviewPage : Page
{
    private readonly SpiceRouteContext context;
    private readonly StackPanel body = new() { Spacing = 0, MaxWidth = 1120, HorizontalAlignment = HorizontalAlignment.Stretch };
    private readonly InfoBar error = new() { IsClosable = true, Severity = InfoBarSeverity.Error, Margin = new Thickness(0, 0, 0, 10) };
    private readonly ProgressBar progress = new() { IsIndeterminate = true, Visibility = Visibility.Collapsed, Height = 2, Margin = new Thickness(0, 0, 0, 10) };
    private bool refreshing;
    private bool active;
    private bool readingHistory;
    private bool narrow;
    private JsonArray? recentSnapshots;
    private string recentKey = "";
    private string? historyError;

    public OverviewPage(SpiceRouteContext context)
    {
        this.context = context;
        Content = new ScrollViewer
        {
            Content = body,
            VerticalScrollBarVisibility = ScrollBarVisibility.Auto,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled
        };
        Loaded += OnLoaded;
        Unloaded += (_, _) => { active = false; context.StateChanged -= Render; };
        SizeChanged += (_, args) =>
        {
            var next = args.NewSize.Width < 760;
            if (next == narrow) return;
            narrow = next;
            Render();
        };
        Render();
    }

    private async void OnLoaded(object sender, RoutedEventArgs args)
    {
        active = true;
        context.StateChanged += Render;
        await LoadRecentAsync();
    }

    private void Render()
    {
        body.Children.Clear();
        var compatibility = Wire.Object(context.Environment, "compatibility");
        var heads = Wire.Array(context.Status, "visibleHeads");
        var ready = Wire.Bool(compatibility, "supported")
            && Wire.Bool(context.Config, "onboardingComplete")
            && !Wire.Bool(context.Status, "pendingRecovery");

        body.Children.Add(BuildHeader());
        body.Children.Add(error);
        body.Children.Add(progress);
        body.Children.Add(BuildCommands(ready, heads));
        body.Children.Add(Ui.Rule(0, 18));
        body.Children.Add(BuildWorkspace(ready, heads));
        body.Children.Add(BuildActivity());

        if (heads.Count > 1) body.Children.Add(BuildBranches(heads, ready));
        body.Children.Add(new Border { Height = 20 });
        if (active) _ = LoadRecentAsync();
    }

    private Grid BuildHeader()
    {
        var header = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto);
        var title = Ui.PageTitle("Overview");
        AutomationProperties.SetName(title, "Overview");
        Ui.Add(header, title);
        var refresh = Ui.IconButton("Refresh", "\uE72C");
        refresh.IsEnabled = !refreshing;
        refresh.Click += async (_, _) =>
        {
            refreshing = true;
            recentKey = "";
            refresh.IsEnabled = false;
            progress.Visibility = Visibility.Visible;
            error.IsOpen = false;
            try { await context.RefreshAsync(); }
            catch (Exception exception) { error.Message = exception.Message; error.IsOpen = true; }
            finally
            {
                refreshing = false;
                progress.Visibility = Visibility.Collapsed;
                refresh.IsEnabled = true;
            }
        };
        Ui.Add(header, refresh, column: 1);
        return header;
    }

    private FrameworkElement BuildCommands(bool ready, JsonArray heads)
    {
        var commands = new StackPanel
        {
            Orientation = Orientation.Horizontal,
            Spacing = 8,
            Margin = new Thickness(0, 9, 0, 14)
        };
        var push = Ui.Button("Push", "\uE74A", true);
        var pull = Ui.Button("Pull latest", "\uE74B");
        var canReplaceCloudHistory = heads.Count > 0 && Wire.Text(context.Status, "lastAppliedSnapshotId").Length == 0;
        push.IsEnabled = ready && (heads.Count <= 1 || Wire.Bool(context.Status, "mergeReady") || canReplaceCloudHistory);
        pull.IsEnabled = ready && context.Status["latestSnapshot"] is JsonObject;
        push.Click += (_, _) => BeginReview("push");
        pull.Click += (_, _) => BeginReview("pull");
        commands.Children.Add(push);
        commands.Children.Add(pull);
        return commands;
    }

    private FrameworkElement BuildWorkspace(bool ready, JsonArray heads)
    {
        var device = BuildDevice(ready, heads);
        var cloud = BuildCloud(ready);
        if (!narrow)
        {
            var columns = Ui.ColumnsWithSpacing(32, new GridLength(1, GridUnitType.Star), new GridLength(258));
            Ui.Add(columns, device);
            Ui.Add(columns, cloud, column: 1);
            return columns;
        }

        var rows = new Grid { RowSpacing = 18 };
        rows.RowDefinitions.Add(new() { Height = GridLength.Auto });
        rows.RowDefinitions.Add(new() { Height = GridLength.Auto });
        Ui.Add(rows, device);
        Ui.Add(rows, cloud, row: 1);
        return rows;
    }

    private FrameworkElement BuildDevice(bool ready, JsonArray heads)
    {
        var section = new StackPanel { Spacing = 0 };
        var heading = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto);
        Ui.Add(heading, Ui.SectionTitle("This computer"));
        var status = StatusLabel(ready, heads);
        Ui.Add(heading, Ui.StatusPill(status.Text, status.Positive), column: 1);
        section.Children.Add(heading);

        var identity = Ui.ColumnsWithSpacing(13, new GridLength(28), new GridLength(1, GridUnitType.Star));
        identity.Margin = new Thickness(0, 13, 0, 0);
        var icon = Ui.Icon("\uE7F4", 24);
        icon.Style = Ui.Style("SpiceAccentIconStyle");
        icon.VerticalAlignment = VerticalAlignment.Top;
        Ui.Add(identity, icon);
        var identityText = new StackPanel { Spacing = 2 };
        identityText.Children.Add(Ui.Text(Wire.Text(context.Config, "deviceName", "This PC"), 17, true));
        var deviceMessage = ready && context.Status["latestSnapshot"] is not JsonObject
            ? "Choose what to sync, then Push to save your first handoff."
            : Wire.Text(context.Status, "message", "Connect a cloud folder to begin.");
        identityText.Children.Add(Ui.Muted(deviceMessage, 11));
        Ui.Add(identity, identityText, column: 1);
        section.Children.Add(identity);

        if (!ready)
        {
            var explanation = Wire.Text(Wire.Object(context.Environment, "compatibility"), "explanation");
            if (explanation.Length > 0)
            {
                var callout = new Border
                {
                    Style = Ui.Style("SpiceSubtleBorderStyle"),
                    CornerRadius = new CornerRadius(5),
                    Padding = new Thickness(10, 8, 10, 8),
                    Margin = new Thickness(0, 12, 0, 0)
                };
                var calloutRow = Ui.ColumnsWithSpacing(10, new GridLength(1, GridUnitType.Star), GridLength.Auto);
                Ui.Add(calloutRow, Ui.Muted(explanation, 11));
                var settings = Ui.TextButton(Wire.Bool(context.Status, "pendingRecovery") ? "Review recovery" : "Check settings");
                settings.Click += (_, _) => context.Navigate(Wire.Bool(context.Status, "pendingRecovery") ? "recovery" : "settings");
                Ui.Add(calloutRow, settings, column: 1);
                callout.Child = calloutRow;
                section.Children.Add(callout);
            }
        }

        section.Children.Add(Ui.WithMargin(Ui.Text("Included in the next handoff", 12, true), new Thickness(0, 18, 0, 4)));
        var selection = Wire.Object(context.Config, "selection");
        var modes = Wire.Object(selection, "projectModes");
        var excluded = Wire.Array(selection, "excludedThreadIds").Select(node => node?.ToString()).ToHashSet();
        var threads = Wire.Array(context.Catalog, "threads").OfType<JsonObject>().Count(thread =>
            !excluded.Contains(Wire.Text(thread, "id"))
            && (Wire.Bool(selection, "includeArchived", true) || !Wire.Bool(thread, "archived"))
            && (Wire.Text(thread, "projectId") == ""
                || (modes[Wire.Text(thread, "projectId")]?.ToString()
                    ?? Wire.Text(selection, "defaultProjectMode", "full")) != "excluded"));
        var projects = Wire.Array(context.Catalog, "projects").OfType<JsonObject>();
        var full = projects.Count(project => ProjectMode(project, modes, selection) == "full");
        var history = projects.Count(project => ProjectMode(project, modes, selection) == "historyOnly");
        section.Children.Add(SummaryRow("\uE8BD", $"{threads} chats", "Project and projectless conversations"));
        section.Children.Add(SummaryRow("\uE8B7", $"{full + history} projects", ProjectCaption(full, history)));
        section.Children.Add(SummaryRow("\uE72E", "Local recovery", "A restore point is kept before files change"));

        var selectionRow = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto);
        Ui.Add(selectionRow, Ui.Muted("Project files and history", 11));
        var change = Ui.TextButton("Change selection");
        change.Click += (_, _) => context.Navigate("selection");
        Ui.Add(selectionRow, change, column: 1);
        section.Children.Add(new Border
        {
            Margin = new Thickness(0, 7, 0, 0),
            Style = Ui.Style("SpiceLineBottomBorderStyle"),
            BorderThickness = new Thickness(0, 1, 0, 0),
            Padding = new Thickness(0, 10, 0, 0),
            Child = selectionRow
        });
        return section;
    }

    private static string ProjectMode(JsonObject project, JsonObject modes, JsonObject selection)
        => modes[Wire.Text(project, "id")]?.ToString() ?? Wire.Text(selection, "defaultProjectMode", "full");

    private static string ProjectCaption(int full, int history)
    {
        if (full == 0 && history == 0) return "No project folders selected";
        if (full == 0) return "Chat history only";
        if (history == 0) return "Files, work changes, and Git history";
        return $"{full} full, {history} chat history only";
    }

    private static FrameworkElement SummaryRow(string glyph, string title, string caption)
    {
        var row = Ui.ColumnsWithSpacing(11, new GridLength(18), new GridLength(1, GridUnitType.Star));
        row.MinHeight = 43;
        row.Padding = new Thickness(0, 6, 0, 5);
        var icon = Ui.Icon(glyph, 15);
        icon.Style = Ui.Style("SpiceAccentIconStyle");
        icon.VerticalAlignment = VerticalAlignment.Top;
        icon.Margin = new Thickness(0, 2, 0, 0);
        Ui.Add(row, icon);
        var copy = new StackPanel { Spacing = 1 };
        copy.Children.Add(Ui.Text(title, 12, true));
        copy.Children.Add(Ui.Muted(caption, 10));
        Ui.Add(row, copy, column: 1);
        return row;
    }

    private FrameworkElement BuildCloud(bool ready)
    {
        var cloud = new StackPanel { Spacing = 0 };
        cloud.Children.Add(Ui.SectionTitle("Cloud folder"));
        var provider = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8, Margin = new Thickness(0, 12, 0, 0) };
        var cloudIcon = Ui.Icon("\uE753", 15);
        cloudIcon.Style = Ui.Style("SpiceAccentIconStyle");
        provider.Children.Add(cloudIcon);
        provider.Children.Add(Ui.Text(ProviderName(Wire.Text(context.Config, "cloudProvider")), 12, true));
        cloud.Children.Add(provider);
        cloud.Children.Add(Ui.WithMargin(Ui.Muted($"{Wire.Bytes(Wire.Number(context.Status, "cloudBytes"))} stored", 10), new Thickness(23, 2, 0, 0)));
        cloud.Children.Add(Ui.Rule(14, 14));
        cloud.Children.Add(Ui.Text("Latest visible handoff", 12, true));

        if (context.Status["latestSnapshot"] is JsonObject latest)
        {
            cloud.Children.Add(Ui.WithMargin(Ui.Text(Wire.Time(Wire.Text(latest, "createdAt")), 14, true), new Thickness(0, 10, 0, 0)));
            cloud.Children.Add(Ui.WithMargin(Ui.Muted(Wire.Text(latest, "deviceName"), 11), new Thickness(0, 3, 0, 0)));
            var identifier = Ui.Muted(FriendlyHandoffId(latest), 10);
            identifier.IsTextSelectionEnabled = true;
            identifier.Margin = new Thickness(0, 9, 0, 0);
            cloud.Children.Add(identifier);
            cloud.Children.Add(Ui.WithMargin(Ui.Muted(
                Wire.Bool(latest, "verified")
                    ? "Received and verified"
                    : "Visible in the sync folder. Contents are checked during Pull.",
                10), new Thickness(0, 8, 0, 0)));
            var review = Ui.Button("Review handoff", "\uE72A");
            review.Margin = new Thickness(0, 12, 0, 0);
            review.IsEnabled = ready;
            review.Click += (_, _) => BeginReview("pull");
            cloud.Children.Add(review);
        }
        else
        {
            var heads = Wire.Array(context.Status, "visibleHeads");
            cloud.Children.Add(Ui.WithMargin(Ui.Muted(
                heads.Count > 1
                    ? "Several branches need review. Choose one below."
                    : "Nothing published yet. Push from this PC to create the first handoff.",
                11), new Thickness(0, 10, 0, 0)));
        }

        return new Border
        {
            Style = Ui.Style("SpiceSubtleBorderStyle"),
            Padding = new Thickness(19, 17, 19, 17),
            CornerRadius = new CornerRadius(6),
            Child = cloud,
            VerticalAlignment = VerticalAlignment.Top,
            HorizontalAlignment = HorizontalAlignment.Stretch
        };
    }

    private FrameworkElement BuildActivity()
    {
        var section = new StackPanel { Spacing = 0, Margin = new Thickness(0, 20, 0, 0) };
        var heading = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto);
        Ui.Add(heading, Ui.SectionTitle("Recent handoffs"));
        var recovery = Ui.TextButton("View recovery");
        recovery.Click += (_, _) => context.Navigate("recovery");
        Ui.Add(heading, recovery, column: 1);
        section.Children.Add(heading);
        section.Children.Add(Ui.Rule(9, 0));

        if (recentSnapshots is null)
        {
            section.Children.Add(Ui.WithMargin(Ui.Muted(historyError ?? "Loading visible handoffs…", 11), new Thickness(0, 12, 0, 0)));
            return section;
        }
        if (recentSnapshots.Count == 0)
        {
            section.Children.Add(Ui.WithMargin(Ui.Muted("Published handoffs will appear here.", 11), new Thickness(0, 12, 0, 0)));
            return section;
        }

        foreach (var handoff in recentSnapshots.OfType<JsonObject>().OrderByDescending(item => Wire.Text(item, "createdAt")).Take(3))
            section.Children.Add(ActivityRow(handoff));
        return section;
    }

    private FrameworkElement ActivityRow(JsonObject handoff)
    {
        var row = Ui.ColumnsWithSpacing(12, new GridLength(18), new GridLength(155), new GridLength(1, GridUnitType.Star), GridLength.Auto);
        row.MinHeight = 40;
        row.Padding = new Thickness(4, 7, 4, 7);
        var icon = Ui.Icon(Wire.Text(handoff, "id") == Wire.Text(context.Status, "lastAppliedSnapshotId") ? "\uE73E" : "\uE753", 13);
        icon.Style = Ui.Style("SpiceAccentIconStyle");
        icon.VerticalAlignment = VerticalAlignment.Center;
        Ui.Add(row, icon);
        Ui.Add(row, Ui.WithAlignment(Ui.Muted(Wire.Time(Wire.Text(handoff, "createdAt")), 10), VerticalAlignment.Center), column: 1);
        Ui.Add(row, Ui.WithAlignment(Ui.Muted(Wire.Text(handoff, "deviceName"), 10), VerticalAlignment.Center), column: 2);
        Ui.Add(row, Ui.WithAlignment(Ui.Muted(
            Wire.Text(handoff, "id") == Wire.Text(context.Status, "lastAppliedSnapshotId")
                ? "Current baseline"
                : "Visible in cloud folder",
            10), VerticalAlignment.Center, HorizontalAlignment.Right), column: 3);
        return new Border
        {
            Style = Ui.Style("SpiceLineBottomBorderStyle"),
            BorderThickness = new Thickness(0, 0, 0, 1),
            Child = row
        };
    }

    private FrameworkElement BuildBranches(JsonArray heads, bool ready)
    {
        var section = new StackPanel { Spacing = 8, Margin = new Thickness(0, 18, 0, 0) };
        section.Children.Add(Ui.SectionTitle("Visible branches"));
        section.Children.Add(Ui.Muted("Choose the device history you want to review.", 11));
        foreach (var head in heads.OfType<JsonObject>())
        {
            var branch = Ui.Button($"Review {Wire.Text(head, "deviceName")} · {Wire.Time(Wire.Text(head, "createdAt"))}");
            branch.IsEnabled = ready;
            branch.Click += (_, _) => BeginReview("pull", Wire.Text(head, "id"));
            section.Children.Add(branch);
        }
        return section;
    }

    private (string Text, bool Positive) StatusLabel(bool ready, JsonArray heads)
    {
        if (Wire.Bool(context.Status, "pendingRecovery")) return ("Recovery needed", false);
        if (!Wire.Bool(context.Config, "onboardingComplete")) return ("Setup needed", false);
        if (!Wire.Bool(Wire.Object(context.Environment, "compatibility"), "supported")) return ("Compatibility issue", false);
        if (heads.Count > 1) return ("Review branches", false);
        if (Wire.Text(context.Status, "lastAppliedSnapshotId").Length == 0 && context.Status["latestSnapshot"] is JsonObject) return ("Cloud handoff visible", true);
        return ready ? ("Ready", true) : ("Action needed", false);
    }

    private static string ProviderName(string provider) => provider switch
    {
        "oneDrive" => "OneDrive",
        "googleDrive" => "Google Drive",
        "iCloud" => "iCloud Drive",
        _ => "Cloud folder"
    };

    private static string FriendlyHandoffId(JsonObject latest)
    {
        var raw = Wire.Text(latest, "shortId");
        if (raw.Length == 0) raw = Wire.Text(latest, "id");
        var suffix = raw.Split('-', StringSplitOptions.RemoveEmptyEntries).LastOrDefault() ?? raw;
        if (suffix.Length > 8) suffix = suffix[^8..];
        if (DateTimeOffset.TryParse(Wire.Text(latest, "createdAt"), CultureInfo.InvariantCulture, DateTimeStyles.AssumeUniversal, out var created))
            return $"{created.ToLocalTime():yyyyMMdd.HHmm}-{suffix.ToUpperInvariant()}";
        return raw;
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
            recentSnapshots = result as JsonArray ?? new JsonArray();
            recentKey = key;
            historyError = null;
        }
        catch (Exception exception)
        {
            if (active)
            {
                historyError = "Recent handoffs could not load. " + exception.Message;
                recentKey = key;
            }
        }
        finally
        {
            readingHistory = false;
            if (active) Render();
        }
    }

    private void BeginReview(string direction, string? snapshotId = null)
    {
        context.CurrentPreview = null;
        context.ReviewDirection = direction;
        context.ReviewSnapshotId = snapshotId;
        context.Navigate("review");
    }
}
