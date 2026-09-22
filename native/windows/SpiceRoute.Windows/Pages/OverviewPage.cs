using System.Globalization;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class OverviewPage : Page
{
    private readonly SpiceRouteContext context;
    private readonly StackPanel body = new() { Spacing = 0, MaxWidth = 1120, HorizontalAlignment = HorizontalAlignment.Stretch };
    private readonly InfoBar error = new() { IsOpen = false, IsClosable = true, Severity = InfoBarSeverity.Error };
    private readonly ProgressBar progress = new() { IsIndeterminate = true, Visibility = Visibility.Collapsed };
    private JsonArray? recentSnapshots;
    private string recentKey = "";
    private string? historyError;
    private bool active, narrow, refreshing, readingHistory;

    public OverviewPage(SpiceRouteContext context)
    {
        this.context = context;
        Content = new ScrollViewer { Content = body, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
        Loaded += async (_, _) => { active = true; context.StateChanged += Render; await LoadRecentAsync(); };
        Unloaded += (_, _) => { active = false; context.StateChanged -= Render; };
        SizeChanged += (_, args) => { var next = args.NewSize.Width < 660; if (next != narrow) { narrow = next; Render(); } };
        Render();
    }

    private void Render()
    {
        body.Children.Clear();
        var heads = Wire.Array(context.Status, "visibleHeads");
        var ready = Wire.Bool(Wire.Object(context.Environment, "compatibility"), "supported")
            && Wire.Bool(context.Config, "onboardingComplete") && !Wire.Bool(context.Status, "pendingRecovery");
        body.Children.Add(BuildHeader());
        body.Children.Add(error);
        body.Children.Add(progress);
        body.Children.Add(BuildStatus(ready, heads));
        body.Children.Add(BuildPair(ready, heads));
        body.Children.Add(BuildActivity());
        if (heads.Count > 1) body.Children.Add(BuildBranches(heads, ready));
        var note = Ui.ColumnsWithSpacing(8, new GridLength(16), new GridLength(1, GridUnitType.Star));
        note.Margin = new Thickness(0, 22, 0, 24);
        var icon = Ui.Icon("\uE946", 14); icon.Style = Ui.Style("SpiceMutedIconStyle"); Ui.Add(note, icon);
        Ui.Add(note, Ui.Muted("Your drive app handles delivery. A visible handoff may still be downloading.", 12), column: 1);
        body.Children.Add(note);
        if (active) _ = LoadRecentAsync();
    }

    private Grid BuildHeader()
    {
        var header = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto);
        Ui.Add(header, Ui.PageTitle("Overview"));
        var refresh = Ui.IconButton("Refresh", "\uE72C"); refresh.IsEnabled = !refreshing;
        refresh.Click += async (_, _) =>
        {
            refreshing = true; recentKey = ""; refresh.IsEnabled = false; progress.Visibility = Visibility.Visible; error.IsOpen = false;
            try { await context.RefreshAsync(); }
            catch (Exception exception) { error.Message = exception.Message; error.IsOpen = true; }
            finally { refreshing = false; progress.Visibility = Visibility.Collapsed; refresh.IsEnabled = true; }
        };
        Ui.Add(header, refresh, column: 1);
        return header;
    }

    private FrameworkElement BuildStatus(bool ready, JsonArray heads)
    {
        var row = Ui.ColumnsWithSpacing(10, new GridLength(16), new GridLength(1, GridUnitType.Star), GridLength.Auto);
        row.Margin = new Thickness(0, 20, 0, 0); row.Padding = new Thickness(0, 0, 0, 18);
        var latest = context.Status["latestSnapshot"] as JsonObject;
        var applied = latest is not null && Wire.Text(latest, "id") == Wire.Text(context.Status, "lastAppliedSnapshotId");
        var message = !ready ? Wire.Bool(context.Status, "pendingRecovery") ? "Finish recovery before your next handoff."
            : !Wire.Bool(context.Config, "onboardingComplete") ? "Connect your folders to begin." : "Codex compatibility needs attention."
            : heads.Count > 1 ? Wire.Bool(context.Status, "mergeReady") ? "The reviewed branches are ready to publish." : "Several handoffs need review. Choose a branch below."
            : latest is null ? "Ready for your first handoff." : applied ? "This device has the latest visible handoff."
            : "A handoff is visible in your sync folder. Pull to review it.";
        var icon = Ui.Icon(ready && heads.Count <= 1 ? "\uE73E" : "\uE946", 14); icon.Style = Ui.Style("SpiceMutedIconStyle");
        Ui.Add(row, icon); Ui.Add(row, Ui.Muted(message, 12), column: 1);
        if (!ready)
        {
            var recovery = Wire.Bool(context.Status, "pendingRecovery");
            var action = Ui.TextButton(recovery ? "Review recovery" : "Check settings");
            action.Click += (_, _) => context.Navigate(recovery ? "recovery" : "settings"); Ui.Add(row, action, column: 2);
            ToolTipService.SetToolTip(row, Wire.Text(Wire.Object(context.Environment, "compatibility"), "explanation"));
        }
        return row;
    }

    private FrameworkElement BuildPair(bool ready, JsonArray heads)
    {
        var grid = new Grid { RowSpacing = narrow ? 24 : 0, ColumnSpacing = narrow ? 0 : 28 };
        grid.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new() { Width = narrow ? new GridLength(0) : new GridLength(1, GridUnitType.Star) });
        grid.RowDefinitions.Add(new() { Height = GridLength.Auto }); grid.RowDefinitions.Add(new() { Height = GridLength.Auto });
        Ui.Add(grid, BuildDevice(ready, heads));
        var cloud = new Border { Style = Ui.Style("SpiceLineBottomBorderStyle"), BorderThickness = narrow ? new Thickness(0, 1, 0, 0) : new Thickness(1, 0, 0, 0), Padding = narrow ? new Thickness(0, 22, 0, 0) : new Thickness(28, 0, 0, 0), Child = BuildCloud(ready, heads) };
        Ui.Add(grid, cloud, row: narrow ? 1 : 0, column: narrow ? 0 : 1);
        return grid;
    }

    private static StackPanel Pane(string glyph, string label, string title, string caption)
    {
        var pane = new StackPanel { Spacing = 0 };
        var heading = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        var icon = Ui.Icon(glyph, 16); icon.Style = Ui.Style("SpiceAccentIconStyle"); heading.Children.Add(icon); heading.Children.Add(Ui.Muted(label, 13));
        pane.Children.Add(heading);
        pane.Children.Add(Ui.WithMargin(Ui.Text(title, 23, true), new Thickness(0, 13, 0, 5)));
        pane.Children.Add(Ui.Muted(caption, 12));
        return pane;
    }

    private static Grid Fact(string label, string value, bool copy = false)
    {
        var row = Ui.ColumnsWithSpacing(14, new GridLength(1, GridUnitType.Star), new GridLength(1.45, GridUnitType.Star)); row.MinHeight = 35;
        Ui.Add(row, Ui.WithAlignment(Ui.Muted(label, 12), VerticalAlignment.Center));
        var text = Ui.Text(value, 12, true); text.IsTextSelectionEnabled = copy;
        Ui.Add(row, Ui.WithAlignment(text, VerticalAlignment.Center, HorizontalAlignment.Right), column: 1);
        return row;
    }

    private FrameworkElement BuildDevice(bool ready, JsonArray heads)
    {
        var pane = Pane("\uE7F4", "This device", Wire.Text(context.Config, "deviceName", "This PC"), "Choose what goes to your next computer.");
        var summary = SelectionSummary.Count(context.Config, context.Catalog);
        var modes = summary.Full > 0 && summary.History > 0 ? $"{summary.Full} full · {summary.History} history only" : summary.Full > 0 ? "full projects" : "chat history only";
        var facts = new StackPanel { Margin = new Thickness(0, 22, 0, 20) };
        facts.Children.Add(Fact("Selected chats", summary.Chats.ToString()));
        facts.Children.Add(Fact("Projects", $"{summary.Full + summary.History} · {modes}"));
        // Startup uses the quick catalog. Never scan working trees just to draw Overview.
        facts.Children.Add(Fact("Selected content", summary.Full > 0 ? "Calculated in review" : Wire.Bytes(summary.Bytes)));
        pane.Children.Add(facts);
        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        var push = Ui.Button("Push", "\uE74A", true);
        var replacement = heads.Count > 0 && Wire.Text(context.Status, "lastAppliedSnapshotId").Length == 0;
        push.IsEnabled = ready && (heads.Count <= 1 || Wire.Bool(context.Status, "mergeReady") || replacement);
        push.Click += (_, _) => BeginReview("push"); actions.Children.Add(push);
        var edit = Ui.TextButton("Edit selection"); edit.Click += (_, _) => context.Navigate("selection"); actions.Children.Add(edit);
        pane.Children.Add(actions);
        return pane;
    }

    private FrameworkElement BuildCloud(bool ready, JsonArray heads)
    {
        var latest = context.Status["latestSnapshot"] as JsonObject;
        var pane = Pane("\uE753", ProviderName(Wire.Text(context.Config, "cloudProvider")) + " sync folder",
            latest is null ? "No handoff yet" : HandoffTime(Wire.Text(latest, "createdAt")),
            latest is null ? heads.Count > 1 ? "Choose a visible branch below." : "Push to create your first handoff." : $"From {Wire.Text(latest, "deviceName")} · latest visible handoff");
        var facts = new StackPanel { Margin = new Thickness(0, 22, 0, 20) };
        facts.Children.Add(Fact("Handoff", latest is null ? "None published" : FriendlyHandoffId(latest), true));
        facts.Children.Add(Fact("Contents", latest is null ? "No selected content" : Wire.Bytes(Wire.Number(latest, "logicalBytes")) + " selected"));
        facts.Children.Add(Fact("On this device", latest is null ? "Ready to Push" : LocalState(latest)));
        pane.Children.Add(facts);
        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 12 };
        var pull = Ui.Button("Pull", "\uE74B"); pull.IsEnabled = ready && latest is not null && heads.Count <= 1;
        pull.Click += (_, _) => BeginReview("pull"); actions.Children.Add(pull);
        if (latest is not null)
        {
            var details = Ui.TextButton("Details");
            var info = new StackPanel { Spacing = 10, MaxWidth = 350 };
            info.Children.Add(Ui.SectionTitle("Handoff details"));
            var id = Ui.Text(Wire.Text(latest, "id"), 12); id.IsTextSelectionEnabled = true; info.Children.Add(id);
            info.Children.Add(Ui.Muted($"{Wire.Number(latest, "objectCount"):N0} content objects · {Wire.Bytes(Wire.Number(latest, "storedBytes"))} stored", 12));
            info.Children.Add(Ui.Muted("Match this identifier on your other device. Cloud delivery is not confirmed by a successful Push.", 12));
            details.Flyout = new Flyout { Content = info }; actions.Children.Add(details);
        }
        pane.Children.Add(actions);
        return pane;
    }

    private string LocalState(JsonObject handoff)
    {
        var id = Wire.Text(handoff, "id");
        if (id == Wire.Text(context.Status, "lastPushedSnapshotId")) return "Saved to sync folder";
        if (id == Wire.Text(context.Status, "lastAppliedSnapshotId")) return "Received and verified";
        return "Not yet pulled";
    }

    private FrameworkElement BuildActivity()
    {
        var section = new StackPanel { Margin = new Thickness(0, 32, 0, 0) };
        var heading = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto);
        Ui.Add(heading, Ui.SectionTitle("Recent handoffs"));
        var recovery = Ui.TextButton("View recovery"); recovery.Click += (_, _) => context.Navigate("recovery"); Ui.Add(heading, recovery, column: 1);
        section.Children.Add(heading); section.Children.Add(Ui.Rule(10));
        if (recentSnapshots is null || recentSnapshots.Count == 0)
            section.Children.Add(Ui.WithMargin(Ui.Muted(recentSnapshots is null ? historyError ?? "Loading visible handoffs…" : "Published handoffs will appear here.", 12), new Thickness(0, 18, 0, 0)));
        else foreach (var handoff in recentSnapshots.OfType<JsonObject>().OrderByDescending(item => Wire.Text(item, "createdAt")).Take(3))
        {
            var row = Ui.ColumnsWithSpacing(12, new GridLength(18), new GridLength(1, GridUnitType.Star), GridLength.Auto);
            row.MinHeight = 62; row.Padding = new Thickness(0, 10, 0, 10);
            var icon = Ui.Icon("\uE753", 15); icon.Style = Ui.Style("SpiceMutedIconStyle"); Ui.Add(row, icon);
            var copy = new StackPanel { Spacing = 3 }; copy.Children.Add(Ui.Text(HandoffTime(Wire.Text(handoff, "createdAt")), 12, true)); copy.Children.Add(Ui.Muted("From " + Wire.Text(handoff, "deviceName"), 11)); Ui.Add(row, copy, column: 1);
            Ui.Add(row, Ui.WithAlignment(Ui.Muted(Wire.Text(handoff, "id") == Wire.Text(context.Status, "lastAppliedSnapshotId") || Wire.Text(handoff, "id") == Wire.Text(context.Status, "lastPushedSnapshotId") ? LocalState(handoff) : "Visible in sync folder", 12), VerticalAlignment.Center), column: 2);
            section.Children.Add(new Border { Style = Ui.Style("SpiceLineBottomBorderStyle"), BorderThickness = new Thickness(0, 0, 0, 1), Child = row });
        }
        return section;
    }

    private FrameworkElement BuildBranches(JsonArray heads, bool ready)
    {
        var section = new StackPanel { Spacing = 8, Margin = new Thickness(0, 24, 0, 0) };
        section.Children.Add(Ui.SectionTitle("Visible branches"));
        section.Children.Add(Ui.Muted("Choose the device history you want to review.", 12));
        foreach (var head in heads.OfType<JsonObject>())
        {
            var branch = Ui.Button($"Review {Wire.Text(head, "deviceName")} · {HandoffTime(Wire.Text(head, "createdAt"))}"); branch.IsEnabled = ready;
            branch.Click += (_, _) => BeginReview("pull", Wire.Text(head, "id")); section.Children.Add(branch);
        }
        return section;
    }
    private static string ProviderName(string provider) => provider switch { "oneDrive" => "OneDrive", "googleDrive" => "Google Drive", "iCloud" => "iCloud Drive", _ => "Cloud" };
    private static string HandoffTime(string value) => DateTimeOffset.TryParse(value, out var date) ? date.ToLocalTime().ToString("MMM d, h:mm tt", CultureInfo.CurrentCulture) : value;
    private static string FriendlyHandoffId(JsonObject latest)
    {
        var raw = Wire.Text(latest, "shortId", Wire.Text(latest, "id"));
        var suffix = raw.Split('-', StringSplitOptions.RemoveEmptyEntries).LastOrDefault() ?? raw;
        if (suffix.Length > 8) suffix = suffix[^8..];
        return DateTimeOffset.TryParse(Wire.Text(latest, "createdAt"), out var date) ? $"{date.ToLocalTime():MMM d} · {suffix.ToUpperInvariant()}" : raw;
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
