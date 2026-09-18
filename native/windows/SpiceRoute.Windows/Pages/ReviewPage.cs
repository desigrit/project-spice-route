using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Markup;
using System.ComponentModel;
using System.Text.Json.Nodes;

namespace SpiceRoute.Windows;

[Microsoft.UI.Xaml.Data.Bindable]
public sealed class ReviewChange : INotifyPropertyChanged
{
    public string Key { get; set; } = "";
    public string Label { get; set; } = "";
    public string Detail { get; set; } = "";
    public string Action { get; set; } = "";
    public string ProjectId { get; set; } = "";
    public string ProjectName { get; set; } = "";
    public double Bytes { get; set; }
    public string Size => Wire.Bytes(Bytes);
    public string ActionLabel => Action switch { "add" => "New", "update" => "Updated", "delete" => "Deleted", "conflict" => "Conflict", _ => "Unchanged" };
    private int choiceIndex;
    public int ChoiceIndex { get => choiceIndex; set { if (choiceIndex == value) return; choiceIndex = value; PropertyChanged?.Invoke(this, new(nameof(ChoiceIndex))); } }
    public event PropertyChangedEventHandler? PropertyChanged;
}
public sealed class ReviewPage : Page
{
    private readonly SpiceRouteContext context;
    private readonly Grid root = new() { RowSpacing = 0 };
    private readonly TextBlock heading = Ui.PageTitle("Review");
    private readonly TextBlock summary = Ui.Muted("Preparing the comparison…", 11);
    private readonly TextBlock state = Ui.Muted("", 11);
    private readonly InfoBar error = new() { Severity = InfoBarSeverity.Error, IsClosable = true };
    private readonly ProgressBar progress = new() { IsIndeterminate = true, Visibility = Visibility.Collapsed };
    private readonly Button execute;
    private readonly Button cancel = Ui.Button("Cancel");
    private readonly Button refresh = Ui.IconButton("Refresh review", "\uE72C");
    private readonly SelectorBar tabs = new();
    private readonly SelectorBarItem filesTab = new() { Text = "Files" };
    private readonly SelectorBarItem attentionTab = new() { Text = "Attention" };
    private readonly SelectorBarItem notesTab = new() { Text = "Notes" };
    private readonly Grid content = new();
    private readonly ContentControl contentHost = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch };
    private readonly Grid files = new();
    private readonly Grid attention = new();
    private readonly StackPanel noteItems = Ui.Stack(15);
    private readonly ListView fileList = new() { SelectionMode = ListViewSelectionMode.Single, HorizontalContentAlignment = HorizontalAlignment.Stretch };
    private readonly ListView projectList = new() { SelectionMode = ListViewSelectionMode.Single, Width = 176 };
    private readonly TextBox search = new() { PlaceholderText = "Find a file or project", MaxWidth = 350, HorizontalAlignment = HorizontalAlignment.Stretch };
    private readonly ComboBox sort = new() { Width = 150 };
    private readonly ComboBox actionFilter = new() { Width = 145 };
    private readonly Dictionary<string, string> destinations = new();
    private readonly DispatcherTimer pollTimer = new() { Interval = TimeSpan.FromMilliseconds(500) };
    private readonly List<ReviewChange> changes = new();
    private JsonObject? preview;
    private JsonObject reviewConfig = new();
    private bool working;
    private bool readingProgress;
    private bool loaded;
    private bool cancelled;
    private int generation;
    private string projectFilter = "";
    private sealed record ProjectFilter(string Id, string Name, double Bytes) { public override string ToString() => Name + (Bytes > 0 ? "\n" + Wire.Bytes(Bytes) : ""); }

    public ReviewPage(SpiceRouteContext context)
    {
        this.context = context;
        execute = Ui.Button(context.ReviewDirection == "push" ? "Push" : "Pull", context.ReviewDirection == "push" ? "\uE74A" : "\uE74B", true);
        for (var i = 0; i < 7; i++) root.RowDefinitions.Add(new() { Height = i == 5 ? new GridLength(1, GridUnitType.Star) : GridLength.Auto });
        var top = Ui.Columns(new GridLength(1, GridUnitType.Star), GridLength.Auto);
        top.Margin = new Thickness(0, 0, 0, 5);
        heading.Text = context.ReviewDirection == "push" ? "Review push" : "Review pull";
        Ui.Add(top, heading); Ui.Add(top, refresh, column: 1); Ui.Add(root, top);
        Ui.Add(root, error, 1); Ui.Add(root, progress, 2); Ui.Add(root, summary, 3);
        summary.Margin = new Thickness(0, 6, 0, 7);
        tabs.Items.Add(filesTab); tabs.Items.Add(attentionTab); tabs.Items.Add(notesTab); tabs.SelectedItem = filesTab;
        tabs.SelectionChanged += (_, _) => ShowTab(); Ui.Add(root, tabs, 4);
        BuildFiles();
        content.Children.Add(files); content.Children.Add(attention);
        var notesScroll = new ScrollViewer { Content = noteItems, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, Visibility = Visibility.Collapsed, Tag = "notes" };
        content.Children.Add(notesScroll); contentHost.Content = content; Ui.Add(root, contentHost, 5);
        var bottom = Ui.Columns(new GridLength(1, GridUnitType.Star), GridLength.Auto);
        bottom.Padding = new Thickness(0, 11, 0, 13);
        Ui.Add(bottom, state);
        var actions = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        actions.Children.Add(cancel); actions.Children.Add(execute); Ui.Add(bottom, actions, column: 1);
        Ui.Add(root, new Border { BorderBrush = Ui.Resource("SpiceLine"), BorderThickness = new Thickness(0, 1, 0, 0), Child = bottom }, 6);
        Content = root;
        execute.Click += async (_, _) => await ExecuteAsync();
        refresh.Click += async (_, _) => await PrepareAsync();
        cancel.Click += async (_, _) => await CancelAsync();
        pollTimer.Tick += async (_, _) => await ReadProgressAsync();
        Loaded += async (_, _) => { loaded = true; await PrepareAsync(); };
        Unloaded += (_, _) => { loaded = false; generation++; pollTimer.Stop(); };
        ShowTab(); UpdateActions();
    }

    private void BuildFiles()
    {
        files.RowDefinitions.Add(new() { Height = GridLength.Auto }); files.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        var toolbar = Ui.Columns(new GridLength(1, GridUnitType.Star), GridLength.Auto, GridLength.Auto);
        toolbar.Margin = new Thickness(0, 6, 0, 14);
        sort.Items.Add("Largest first"); sort.Items.Add("Name"); sort.Items.Add("Change"); sort.SelectedIndex = 0;
        actionFilter.Items.Add("All changes"); actionFilter.Items.Add("Conflicts"); actionFilter.Items.Add("New files"); actionFilter.Items.Add("Updates"); actionFilter.Items.Add("Deletions"); actionFilter.Items.Add("Unchanged"); actionFilter.SelectedIndex = 0;
        AutomationProperties.SetName(search, "Find a file or project"); AutomationProperties.SetName(sort, "Sort files"); AutomationProperties.SetName(actionFilter, "Filter changes");
        search.TextChanged += (_, _) => FilterFiles(); sort.SelectionChanged += (_, _) => FilterFiles(); actionFilter.SelectionChanged += (_, _) => FilterFiles();
        Ui.Add(toolbar, search); Ui.Add(toolbar, sort, column: 1); Ui.Add(toolbar, actionFilter, column: 2); Ui.Add(files, toolbar);
        var lists = Ui.Columns(GridLength.Auto, new GridLength(1, GridUnitType.Star));
        projectList.SelectionChanged += (_, _) => { projectFilter = (projectList.SelectedItem as ProjectFilter)?.Id ?? ""; FilterFiles(); };
        AutomationProperties.SetName(projectList, "Project filter"); AutomationProperties.SetName(fileList, "Files and conversations in this handoff");
        fileList.ItemContainerStyle = StretchedRowStyle();
        fileList.ItemTemplate = (DataTemplate)XamlReader.Load("""
          <DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
            <Grid ColumnSpacing="12" Padding="4,9" ToolTipService.ToolTip="{Binding Detail}">
              <Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="76"/><ColumnDefinition Width="70"/></Grid.ColumnDefinitions>
              <StackPanel Spacing="3"><TextBlock Text="{Binding Label}" FontWeight="SemiBold" FontSize="13" TextTrimming="CharacterEllipsis"/><TextBlock Text="{Binding Detail}" FontSize="12" Opacity="0.7" TextTrimming="CharacterEllipsis"/></StackPanel>
              <TextBlock Grid.Column="1" Text="{Binding ActionLabel}" FontSize="12" VerticalAlignment="Center"/>
              <TextBlock Grid.Column="2" Text="{Binding Size}" FontSize="12" HorizontalAlignment="Right" VerticalAlignment="Center"/>
            </Grid>
          </DataTemplate>
          """);
        var listPanel = new Grid(); listPanel.RowDefinitions.Add(new() { Height = GridLength.Auto }); listPanel.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        var labels = Ui.Columns(new GridLength(1, GridUnitType.Star), new GridLength(76), new GridLength(70));
        labels.Padding = new Thickness(16, 8, 16, 8); Ui.Add(labels, Ui.Text("Name and location", 12)); Ui.Add(labels, Ui.Text("Change", 12), column: 1); Ui.Add(labels, Ui.Text("Size", 12), column: 2);
        Ui.Add(listPanel, labels); Ui.Add(listPanel, fileList, 1);
        Ui.Add(lists, projectList); Ui.Add(lists, listPanel, column: 1); Ui.Add(files, lists, 1);
    }

    private async Task PrepareAsync()
    {
        if (working) return;
        var version = ++generation;
        SetWorking(true); error.IsOpen = false; cancelled = false; preview = null; context.CurrentPreview = null;
        state.Text = "Comparing selected history and project files…"; summary.Text = "Preparing the comparison…";
        changes.Clear(); destinations.Clear(); FilterFiles();
        try
        {
            reviewConfig = Wire.Clone(context.Config);
            var parameters = new JsonObject { ["config"] = reviewConfig.DeepClone() };
            if (context.ReviewDirection == "pull") parameters["snapshotId"] = context.ReviewSnapshotId;
            var result = await context.Engine.CallAsync(context.ReviewDirection == "push" ? "preview_push" : "preview_pull", parameters);
            if (!loaded || version != generation) return;
            preview = result as JsonObject ?? throw new InvalidDataException("The review could not be read.");
            context.CurrentPreview = preview;
            BuildReview();
        }
        catch (Exception exception) { if (loaded && version == generation) { error.Message = exception.Message; error.IsOpen = true; state.Text = "Review could not be prepared. Try Refresh review."; } }
        finally { if (version == generation) { SetWorking(false); UpdateActions(); } }
    }

    private void BuildReview()
    {
        if (preview is null) return;
        var replacement = Wire.Bool(preview, "replacesCloudHistory");
        heading.Text = replacement ? "Review cloud replacement" : context.ReviewDirection == "push" ? "Review push" : "Review pull";
        var projects = new Dictionary<string, string>();
        foreach (var item in Wire.Array(context.Catalog, "projects").OfType<JsonObject>()) projects[Wire.Text(item, "id")] = Wire.Text(item, "name");
        foreach (var item in Wire.Array(preview, "changes").OfType<JsonObject>().Where(item => Wire.Text(item, "kind") == "project")) projects[Wire.Text(item, "key").Replace("project:", "")] = Wire.Text(item, "label");
        var threadProjects = Wire.Array(context.Catalog, "threads").OfType<JsonObject>().ToDictionary(item => Wire.Text(item, "id"), item => Wire.Text(item, "projectId"));
        changes.Clear();
        foreach (var item in Wire.Array(preview, "changes").OfType<JsonObject>())
        {
            var key = Wire.Text(item, "key"); var projectId = "";
            if (key.StartsWith("file:projects/", StringComparison.Ordinal)) projectId = key.Split('/').ElementAtOrDefault(1) ?? "";
            else if (key.StartsWith("git:", StringComparison.Ordinal)) projectId = key.Split(':').ElementAtOrDefault(1) ?? "";
            else if (key.StartsWith("project:", StringComparison.Ordinal)) projectId = key[8..];
            else if (key.StartsWith("thread:", StringComparison.Ordinal)) projectId = threadProjects.GetValueOrDefault(key[7..], "");
            var row = new ReviewChange { Key = key, Label = Wire.Text(item, "label"), Detail = Wire.Text(item, "detail"), Bytes = Wire.Number(item, "bytes"), Action = Wire.Text(item, "action"), ProjectId = projectId, ProjectName = projects.GetValueOrDefault(projectId, projectId == "" ? "Chats and other items" : "Incoming project") };
            row.PropertyChanged += (_, _) => UpdateActions(); changes.Add(row);
        }
        var filters = changes.GroupBy(item => item.ProjectId).Select(group => new ProjectFilter(group.Key == "" ? "__other" : group.Key, group.First().ProjectName, group.Where(item => item.Key.StartsWith("file:", StringComparison.Ordinal)).Sum(item => item.Bytes))).OrderByDescending(item => item.Bytes).ToList();
        filters.Insert(0, new ProjectFilter("", "All projects", Wire.Number(preview, "estimatedBytes")));
        projectList.ItemsSource = filters; projectList.SelectedIndex = 0;
        var altered = changes.Count(item => item.Action != "unchanged");
        summary.Text = replacement
            ? $"Cloud replacement · {changes.Count:N0} reviewed items · {Wire.Bytes(Wire.Number(preview, "estimatedBytes"))} selected content"
            : $"{altered:N0} changes · {changes.Count(item => item.Action == "conflict"):N0} conflicts · {Wire.Bytes(Wire.Number(preview, "estimatedBytes"))} selected content";
        filesTab.Text = $"Files ({changes.Count:N0})";
        BuildAttentionAndNotes(); FilterFiles(); UpdateActions();
        tabs.SelectedItem = NeedsDecision() || replacement ? attentionTab : filesTab;
    }

    private void BuildAttentionAndNotes()
    {
        attention.Children.Clear(); attention.RowDefinitions.Clear();
        attention.RowDefinitions.Add(new() { Height = GridLength.Auto }); attention.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        noteItems.Children.Clear();
        var required = Ui.Stack(10);
        foreach (var reason in Wire.Array(preview, "blockedReasons")) required.Children.Add(new InfoBar { IsOpen = true, IsClosable = false, Severity = InfoBarSeverity.Error, Message = reason?.ToString() ?? "" });
        var mappings = Wire.Array(preview, "requiredMappings").OfType<JsonObject>().ToList();
        if (mappings.Count > 0)
        {
            var folders = Ui.Stack(10);
            foreach (var mapping in mappings)
            {
                var key = MappingKey(mapping); destinations[key] = Wire.Text(mapping, "suggestedPath");
                var row = Ui.Columns(new GridLength(1, GridUnitType.Star), GridLength.Auto);
                var text = Ui.Text(Wire.Text(mapping, "projectName") + "\n" + (destinations[key].Length > 0 ? destinations[key] : "Choose a local folder"), 13);
                var choose = Ui.Button("Change folder", "\uE8B7");
                choose.Click += async (_, _) => await Ui.GuardAsync(async () => { var folder = await context.PickFolderAsync(destinations[key]); if (folder is null) return; destinations[key] = folder; text.Text = Wire.Text(mapping, "projectName") + "\n" + folder; }, context);
                Ui.Add(row, text); Ui.Add(row, choose, column: 1); folders.Children.Add(row);
            }
            var save = Ui.Button("Save folders and refresh", null, true); save.Click += async (_, _) => await SaveMappingsAsync(mappings); folders.Children.Add(save);
            required.Children.Add(new Expander { Header = $"Choose destinations for {mappings.Count} folders", IsExpanded = true, HorizontalAlignment = HorizontalAlignment.Stretch, Content = new ScrollViewer { Content = folders, MaxHeight = 220 } });
        }
        var warnings = Wire.Array(preview, "warnings").Select(item => item?.ToString() ?? "").Where(text => text.Length > 0).Distinct().ToList();
        var replacement = Wire.Bool(preview, "replacesCloudHistory");
        var replacementWarning = replacement ? warnings.FirstOrDefault(text => text.StartsWith("This device has no saved sync baseline.", StringComparison.Ordinal)) : null;
        if (replacement)
        {
            required.Children.Add(new InfoBar
            {
                IsOpen = true,
                IsClosable = false,
                Severity = InfoBarSeverity.Warning,
                Title = "This Push replaces the visible cloud handoff",
                Message = replacementWarning ?? "Only this device's current selection will appear in the new cloud handoff."
            });
            var clearOldContent = Ui.TextButton("Reset cloud history first");
            ToolTipService.SetToolTip(clearOldContent, "Remove older snapshots and stored objects before creating the new handoff");
            clearOldContent.Click += (_, _) => context.Navigate("settings");
            required.Children.Add(clearOldContent);
        }
        var actionable = warnings.Where(text => text != replacementWarning && !text.Contains("includes complete Git history", StringComparison.OrdinalIgnoreCase) && !text.Contains("excluded", StringComparison.OrdinalIgnoreCase) && !text.Contains("selected file exclusions", StringComparison.OrdinalIgnoreCase) && !text.Contains("Git history is included", StringComparison.OrdinalIgnoreCase)).ToList();
        if (actionable.Count > 0)
        {
            var items = Ui.Stack(12); foreach (var warning in actionable) items.Children.Add(Ui.Text(warning, 13));
            required.Children.Add(new Expander { Header = $"Capture details to review ({actionable.Count})", Content = new ScrollViewer { Content = items, MaxHeight = 160 }, HorizontalAlignment = HorizontalAlignment.Stretch });
        }
        var large = changes.Where(item => item.Key.StartsWith("file:", StringComparison.Ordinal) && item.Bytes >= 512 * 1024 * 1024).OrderByDescending(item => item.Bytes).ToList();
        if (large.Count > 0)
        {
            var items = Ui.Stack(10); items.Children.Add(Ui.Text("These files stay included. They may be needed on your other device.", 13));
            foreach (var file in large) items.Children.Add(Ui.Text($"{file.Label} · {Wire.Bytes(file.Bytes)}\n{file.ProjectName}", 13));
            var change = Ui.Button("Change sync choices"); change.Click += (_, _) => context.Navigate("selection"); items.Children.Add(change);
            required.Children.Add(new Expander { Header = $"Large files ({large.Count}) · {Wire.Bytes(large.Sum(item => item.Bytes))}", Content = new ScrollViewer { Content = items, MaxHeight = 160 }, HorizontalAlignment = HorizontalAlignment.Stretch });
        }
        Ui.Add(attention, new ScrollViewer { Content = required, MaxHeight = 320 });
        var conflicts = changes.Where(item => item.Action == "conflict").ToList();
        if (conflicts.Count > 0)
        {
            var list = new ListView { ItemsSource = conflicts, SelectionMode = ListViewSelectionMode.None, HorizontalContentAlignment = HorizontalAlignment.Stretch };
            list.ItemContainerStyle = StretchedRowStyle();
            list.ItemTemplate = (DataTemplate)XamlReader.Load("""
              <DataTemplate xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"><Grid ColumnSpacing="16" Padding="4,12"><Grid.ColumnDefinitions><ColumnDefinition Width="*"/><ColumnDefinition Width="180"/></Grid.ColumnDefinitions><StackPanel Spacing="5"><TextBlock Text="{Binding Label}" FontWeight="SemiBold" TextWrapping="Wrap"/><TextBlock Text="{Binding Detail}" FontSize="12" Opacity="0.7" TextWrapping="Wrap"/></StackPanel><ComboBox Grid.Column="1" SelectedIndex="{Binding ChoiceIndex, Mode=TwoWay}" HorizontalAlignment="Stretch" AutomationProperties.Name="{Binding Label}"><ComboBoxItem Content="Choose a version"/><ComboBoxItem Content="Keep mine"/><ComboBoxItem Content="Use incoming"/></ComboBox></Grid></DataTemplate>
              """);
            Ui.Add(attention, list, 1);
        }
        else if (required.Children.Count == 0) Ui.Add(attention, Ui.Text("No decisions are needed for this handoff.", 16), 1);
        attentionTab.Text = $"Attention ({Wire.Array(preview, "blockedReasons").Count + mappings.Count + conflicts.Count + actionable.Count + (large.Count > 0 ? 1 : 0) + (replacement ? 1 : 0)})";
        noteItems.Children.Add(Ui.Text("What travels with this handoff", 20, true));
        noteItems.Children.Add(Ui.Text("Full projects include Git history and selected working files. Project configuration and secrets follow your saved choices. Codex sign-in and machine settings stay local.", 14));
        foreach (var warning in warnings.Except(actionable)) noteItems.Children.Add(Ui.Text(warning, 13));
        noteItems.Children.Add(Ui.Text("Cloud delivery is handled by your drive client. Local publication does not confirm that another device has received the files.", 13));
    }

    private void FilterFiles()
    {
        IEnumerable<ReviewChange> result = changes;
        if (projectFilter.Length > 0) result = result.Where(item => item.ProjectId == (projectFilter == "__other" ? "" : projectFilter));
        if (search.Text.Length > 0) result = result.Where(item => (item.Label + " " + item.Detail + " " + item.ProjectName).Contains(search.Text, StringComparison.CurrentCultureIgnoreCase));
        var action = actionFilter.SelectedIndex switch { 1 => "conflict", 2 => "add", 3 => "update", 4 => "delete", 5 => "unchanged", _ => "" };
        if (action.Length > 0) result = result.Where(item => item.Action == action);
        result = sort.SelectedIndex switch { 1 => result.OrderBy(item => item.Label, StringComparer.CurrentCultureIgnoreCase), 2 => result.OrderBy(item => item.Action).ThenBy(item => item.Label), _ => result.OrderByDescending(item => item.Bytes).ThenBy(item => item.Label) };
        fileList.ItemsSource = result.ToList();
    }

    private void ShowTab()
    {
        files.Visibility = tabs.SelectedItem == filesTab ? Visibility.Visible : Visibility.Collapsed;
        attention.Visibility = tabs.SelectedItem == attentionTab ? Visibility.Visible : Visibility.Collapsed;
        foreach (var item in content.Children.OfType<ScrollViewer>()) item.Visibility = tabs.SelectedItem == notesTab ? Visibility.Visible : Visibility.Collapsed;
    }
    private bool NeedsDecision() => Wire.Array(preview, "blockedReasons").Count > 0 || Wire.Array(preview, "requiredMappings").Count > 0 || changes.Any(item => item.Action == "conflict" && item.ChoiceIndex is not (1 or 2));
    private void SetWorking(bool value)
    {
        working = value; context.IsBusy = value; progress.Visibility = value ? Visibility.Visible : Visibility.Collapsed; refresh.IsEnabled = !value; tabs.IsEnabled = !value; contentHost.IsEnabled = !value;
        cancel.IsEnabled = !value || preview is not null; UpdateActions();
    }
    private void UpdateActions()
    {
        execute.IsEnabled = !working && preview is not null && !NeedsDecision() && (context.ReviewDirection == "pull" || Wire.Bool(preview, "replacesCloudHistory") || changes.Any(item => item.Action != "unchanged"));
        if (working) return;
        var unresolved = changes.Count(item => item.Action == "conflict" && item.ChoiceIndex is not (1 or 2));
        if (unresolved > 0) state.Text = $"Choose a version for {unresolved} conflicts.";
        else if (Wire.Array(preview, "requiredMappings").Count > 0) state.Text = "Choose project destinations in Attention.";
        else if (Wire.Array(preview, "blockedReasons").Count > 0) state.Text = "Resolve the issues in Attention, then refresh.";
        else if (preview is not null && Wire.Bool(preview, "replacesCloudHistory")) state.Text = "Push will replace the visible cloud handoff with this device's current selection.";
        else if (preview is not null) state.Text = Wire.Bool(preview, "requiresCodexClose") ? "Codex will be asked to close before the handoff." : "Ready to continue.";
    }
    private static string MappingKey(JsonObject mapping) => Wire.Text(mapping, "projectId") + ":" + (int)Wire.Number(mapping, "rootIndex");
    private static Style StretchedRowStyle()
    {
        var style = new Style(typeof(ListViewItem));
        style.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch));
        return style;
    }
    private async Task SaveMappingsAsync(List<JsonObject> mappings)
    {
        if (mappings.Any(mapping => string.IsNullOrWhiteSpace(destinations.GetValueOrDefault(MappingKey(mapping))))) { error.Message = "Choose a local folder for every project destination."; error.IsOpen = true; return; }
        SetWorking(true); error.IsOpen = false;
        try
        {
            // Keep reviewing the same incoming snapshot after saving destinations.
            if (context.ReviewDirection == "pull") context.ReviewSnapshotId = Wire.Text(preview, "snapshotId");
            var config = Wire.Clone(context.Config); var source = Wire.Object(config, "sourceRoots"); var destination = Wire.Object(config, "destinationRoots");
            foreach (var mapping in mappings) { var id = Wire.Text(mapping, "projectId"); var key = MappingKey(mapping); if (Wire.Number(mapping, "rootIndex") == 0) { source.Remove(id); destination.Remove(id); } source[key] = destinations[key]; destination[key] = destinations[key]; }
            config["sourceRoots"] = source.DeepClone(); config["destinationRoots"] = destination.DeepClone();
            await context.SaveConfigAsync(config);
            SetWorking(false); await PrepareAsync();
        }
        catch (Exception exception) { error.Message = exception.Message; error.IsOpen = true; SetWorking(false); }
    }
    private async Task ExecuteAsync()
    {
        if (preview is null || working || NeedsDecision()) return;
        SetWorking(true); error.IsOpen = false; cancelled = false; progress.IsIndeterminate = true; state.Text = "Checking Codex before the handoff…";
        pollTimer.Start();
        try
        {
            var resolutionSnapshot = new JsonArray(changes.Where(item => item.Action == "conflict").Select(item => (JsonNode)new JsonObject { ["key"] = item.Key, ["choice"] = Wire.ConflictChoice(item.ChoiceIndex) }).ToArray());
            if (Wire.Bool(preview, "requiresCodexClose"))
            {
                var closed = await context.Engine.CallAsync("request_codex_close");
                if (closed?.GetValue<bool>() != true) throw new InvalidOperationException("Codex is still running. Save your work and quit Codex, then try again.");
            }
            var parameters = new JsonObject { ["config"] = reviewConfig.DeepClone(), ["operationId"] = Wire.Text(preview, "operationId") };
            if (context.ReviewDirection == "pull") parameters["resolutions"] = resolutionSnapshot;
            var result = await context.Engine.CallAsync(context.ReviewDirection == "push" ? "execute_push" : "execute_pull", parameters);
            pollTimer.Stop(); context.CurrentPreview = null; preview = null;
            var message = Wire.Text(result, "statusMessage", "Handoff completed.") + " " + Wire.Text(Wire.Object(result, "snapshot"), "shortId");
            var refreshFailed = false;
            try { await context.RefreshAsync(); }
            catch (Exception refreshError) { refreshFailed = true; message += " The handoff completed, but Overview could not refresh: " + refreshError.Message; }
            SetWorking(false); context.Navigate("overview"); context.ShowMessage(message, refreshFailed);
        }
        catch (Exception exception)
        {
            pollTimer.Stop(); error.Message = exception.Message; error.IsOpen = true;
            try { await context.RefreshAsync(); } catch { }
            SetWorking(false);
            if (cancelled) { preview = null; context.CurrentPreview = null; execute.IsEnabled = false; state.Text = "Cancelled. Refresh the review before trying again."; }
        }
    }
    private async Task ReadProgressAsync()
    {
        if (preview is null || readingProgress || !working) return;
        readingProgress = true;
        try
        {
            var value = await context.Engine.CallAsync("get_operation_progress", new() { ["operationId"] = Wire.Text(preview, "operationId") });
            if (!loaded || !working || value is null) return;
            state.Text = Wire.Text(value, "message", state.Text);
            var total = Wire.Number(value, "totalSteps"); progress.IsIndeterminate = total <= 0;
            if (total > 0) progress.Value = Math.Clamp(Wire.Number(value, "completedSteps") / total * 100, 0, 100);
        }
        catch { }
        finally { readingProgress = false; }
    }
    private async Task CancelAsync()
    {
        if (preview is not null)
        {
            try { await context.Engine.CallAsync("cancel_operation", new() { ["operationId"] = Wire.Text(preview, "operationId") }); }
            catch (Exception exception) { error.Message = exception.Message; error.IsOpen = true; return; }
        }
        if (working) { cancelled = true; cancel.IsEnabled = false; state.Text = "Stopping at a safe checkpoint…"; return; }
        context.CurrentPreview = null; context.Navigate("overview");
    }
}

