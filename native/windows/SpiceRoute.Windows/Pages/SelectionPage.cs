using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class SelectionPage : Page
{
    private readonly SpiceRouteContext _context;
    private readonly JsonObject _draft;
    private JsonObject _catalog;
    private readonly ListView _items = new() { SelectionMode = ListViewSelectionMode.None, HorizontalContentAlignment = HorizontalAlignment.Stretch };
    private readonly TextBox _search = new() { PlaceholderText = "Search projects", Width = 248, MinHeight = 32, FontSize = 13 };
    private readonly SelectorBar _scope = new();
    private readonly SelectorBarItem _projectsTab = new() { Text = "Projects" };
    private readonly SelectorBarItem _projectChatsTab = new() { Text = "Project chats" };
    private readonly SelectorBarItem _projectlessTab = new() { Text = "Projectless chats" };
    private readonly Button _save = Ui.Button("Save choices", primary: true);
    private readonly InfoBar _feedback = new() { IsOpen = false, IsClosable = true };
    private readonly TextBlock _state = NativePageUi.Text("Calculating selected sizes…", 12, true);
    private readonly TextBlock _listCount = NativePageUi.Text("", 12, true);
    private readonly TextBlock _saved = NativePageUi.Text("", 12, true);
    private readonly Grid _defaults = new() { ColumnSpacing = 16, Padding = new Thickness(0, 12, 0, 12) };
    private readonly Grid _columnHeaders = ProjectColumns();
    private readonly DispatcherTimer _feedbackTimer = new() { Interval = TimeSpan.FromSeconds(4) };
    private readonly CancellationTokenSource _lifetime = new();
    private bool _sizesReady;
    private bool _selectionChanged;
    private int _sizeGeneration;

    public SelectionPage(SpiceRouteContext context)
    {
        _context = context;
        _draft = (JsonObject)context.Config.DeepClone();
        _catalog = (JsonObject)context.Catalog.DeepClone();
        _save.IsEnabled = false;
        AutomationProperties.SetName(_search, "Search sync choices");
        AutomationProperties.SetName(_items, "Content to sync");
        _items.ItemContainerStyle = new Style(typeof(ListViewItem));
        _items.ItemContainerStyle.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch));
        _items.ItemContainerStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(0)));
        _items.ItemContainerStyle.Setters.Add(new Setter(FrameworkElement.MinHeightProperty, 0d));
        var body = NativePageUi.PageGrid("What to sync", _save, out var content);
        for (var index = 0; index < 5; index++) content.RowDefinitions.Add(new() { Height = index == 3 ? new GridLength(1, GridUnitType.Star) : GridLength.Auto });
        var top = new StackPanel { Spacing = 10 };
        top.Children.Add(NativePageUi.Text("Choose what travels. Folder locations are saved only on this PC.", 13, true));
        top.Children.Add(_feedback);
        _scope.Items.Add(_projectsTab); _scope.Items.Add(_projectChatsTab); _scope.Items.Add(_projectlessTab);
        _scope.SelectedItem = _projectsTab;
        top.Children.Add(_scope);
        var tools = Ui.Columns(new GridLength(1, GridUnitType.Star), GridLength.Auto);
        tools.Margin = new Thickness(0, 0, 0, 10);
        _listCount.VerticalAlignment = VerticalAlignment.Center;
        tools.Children.Add(_listCount);
        Grid.SetColumn(_search, 1);
        tools.Children.Add(_search);
        top.Children.Add(tools);
        content.Children.Add(top);
        _defaults.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        _defaults.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        var caption = new StackPanel { Spacing = 3, VerticalAlignment = VerticalAlignment.Center };
        caption.Children.Add(Ui.Text("Default for new projects", 13, true));
        caption.Children.Add(NativePageUi.Text("Individual project choices override this setting.", 12, true));
        _defaults.Children.Add(caption);
        var mode = NativePageUi.ModePicker(Wire.Text(Selection, "defaultProjectMode", "full"));
        mode.SelectionChanged += (_, _) => { Selection["defaultProjectMode"] = NativePageUi.ModeValue(mode); MarkChanged(true); Render(); };
        AutomationProperties.SetName(mode, "Default sync mode for new projects");
        Grid.SetColumn(mode, 1);
        _defaults.Children.Add(mode);
        Grid.SetRow(_defaults, 1);
        content.Children.Add(_defaults);
        _columnHeaders.Padding = new Thickness(0, 8, 4, 8);
        _columnHeaders.Style = Ui.Style("SpiceSubtleGridStyle");
        var projectHeader = NativePageUi.Text("Project", 12, true);
        Grid.SetColumn(projectHeader, 1); _columnHeaders.Children.Add(projectHeader);
        var folderHeader = NativePageUi.Text("Local folder", 12, true);
        Grid.SetColumn(folderHeader, 2); _columnHeaders.Children.Add(folderHeader);
        var modeHeader = NativePageUi.Text("Sync", 12, true);
        Grid.SetColumn(modeHeader, 3); _columnHeaders.Children.Add(modeHeader);
        AdaptProjectColumns(_columnHeaders, folderHeader, true);
        Grid.SetRow(_columnHeaders, 2); content.Children.Add(_columnHeaders);
        Grid.SetRow(_items, 3);
        content.Children.Add(_items);
        var footer = Ui.Columns(new GridLength(1, GridUnitType.Star), GridLength.Auto);
        footer.Padding = new Thickness(0, 10, 0, 0);
        footer.Children.Add(_state);
        _saved.Style = Ui.Style("SpiceSuccessTextStyle");
        AutomationProperties.SetLiveSetting(_saved, AutomationLiveSetting.Polite);
        Grid.SetColumn(_saved, 1); footer.Children.Add(_saved);
        Grid.SetRow(footer, 4); content.Children.Add(footer);
        Content = body;
        _save.Click += async (_, _) => await SaveAsync();
        _scope.SelectionChanged += (_, _) => { _search.PlaceholderText = ProjectScope ? "Search projects" : "Search chats"; Render(); };
        _search.TextChanged += (_, _) => Render();
        _feedbackTimer.Tick += (_, _) => { _saved.Text = ""; _feedbackTimer.Stop(); };
        Loaded += async (_, _) => await LoadSizesAsync();
        Unloaded += (_, _) => { _lifetime.Cancel(); _feedbackTimer.Stop(); };
        Render();
    }

    private JsonObject Selection => NativePageUi.EnsureObject(_draft, "selection");
    private bool ProjectScope => _scope.SelectedItem == _projectsTab;
    private void MarkChanged(bool selection)
    {
        _selectionChanged |= selection;
        _save.IsEnabled = true;
        _saved.Text = "";
    }

    private async Task LoadSizesAsync()
    {
        var generation = ++_sizeGeneration;
        _sizesReady = false;
        _state.Text = "Calculating workspace sizes. You can keep choosing.";
        Render();
        try
        {
            var result = await _context.Engine.CallAsync("list_content", new JsonObject { ["config"] = _draft.DeepClone() }, _lifetime.Token);
            if (_lifetime.IsCancellationRequested || generation != _sizeGeneration) return;
            _catalog = result as JsonObject ?? new JsonObject();
            _sizesReady = true;
            _state.Text = "Changes apply to the next Push. Existing cloud history is kept.";
            Render();
        }
        catch (OperationCanceledException) { }
        catch (Exception error) { if (!_lifetime.IsCancellationRequested && generation == _sizeGeneration) { _state.Text = "Workspace sizes are unavailable."; NativePageUi.Error(_feedback, error); } }
    }

    private void Render()
    {
        _items.Items.Clear();
        var query = _search.Text.Trim();
        _defaults.Visibility = _columnHeaders.Visibility = ProjectScope ? Visibility.Visible : Visibility.Collapsed;
        var total = 0;
        if (ProjectScope)
        {
            var projects = Wire.Array(_catalog, "projects").OfType<JsonObject>().ToList();
            total = projects.Count;
            foreach (var project in projects.Where(p => Wire.Text(p, "name").Contains(query, StringComparison.OrdinalIgnoreCase)))
                _items.Items.Add(ProjectRow(project));
        }
        else
        {
            var projectless = _scope.SelectedItem == _projectlessTab;
            var threads = Wire.Array(_catalog, "threads").OfType<JsonObject>().Where(t => Wire.Bool(t, "projectless") == projectless).ToList();
            total = threads.Count;
            foreach (var thread in threads.Where(t => Wire.Text(t, "title").Contains(query, StringComparison.OrdinalIgnoreCase)))
                _items.Items.Add(ChatRow(thread));
        }
        var noun = ProjectScope ? (total == 1 ? "project" : "projects") : (total == 1 ? "chat" : "chats");
        _listCount.Text = query.Length > 0 ? $"{_items.Items.Count} of {total} {noun}" : $"{total} {noun}";
        if (_items.Items.Count == 0) _items.Items.Add(new Border { Padding = new Thickness(0, 24, 0, 24), Child = NativePageUi.Text(query.Length > 0 ? "No matches. Try another search." : "No items were found in the configured Codex folders.", secondary: true) });
    }

    private static Grid ProjectColumns() => Ui.ColumnsWithSpacing(12, new GridLength(28), new GridLength(1, GridUnitType.Star), new GridLength(1, GridUnitType.Star), new GridLength(164));

    private static void AdaptProjectColumns(Grid row, FrameworkElement folder, bool header = false)
    {
        row.RowDefinitions.Add(new() { Height = GridLength.Auto });
        row.RowDefinitions.Add(new() { Height = GridLength.Auto });
        row.SizeChanged += (_, args) =>
        {
            var narrow = args.NewSize.Width < 700;
            row.ColumnDefinitions[2].Width = narrow ? new GridLength(0) : new GridLength(1, GridUnitType.Star);
            if (header) folder.Visibility = narrow ? Visibility.Collapsed : Visibility.Visible;
            else
            {
                Grid.SetRow(folder, narrow ? 1 : 0);
                Grid.SetColumn(folder, narrow ? 1 : 2);
                Grid.SetColumnSpan(folder, narrow ? 3 : 1);
                folder.Margin = new Thickness(0, narrow ? 4 : 0, 0, 0);
            }
        };
    }

    private Grid ProjectRow(JsonObject project)
    {
        var id = Wire.Text(project, "id");
        var row = ProjectColumns();
        row.Padding = new Thickness(0, 12, 4, 12);
        row.MinHeight = 72;
        row.Style = Ui.Style("SpiceLineBottomGridStyle"); row.BorderThickness = new Thickness(0, 0, 0, 1);
        var icon = Ui.Icon("\uE8B7", 19); icon.Style = Ui.Style("SpiceAccentIconStyle");
        icon.VerticalAlignment = VerticalAlignment.Center; row.Children.Add(icon);
        var details = new StackPanel { Spacing = 4, VerticalAlignment = VerticalAlignment.Center };
        var name = Ui.Text(Wire.Text(project, "name", "Project"), 13, true);
        name.TextWrapping = TextWrapping.NoWrap; name.TextTrimming = TextTrimming.CharacterEllipsis;
        ToolTipService.SetToolTip(name, name.Text); details.Children.Add(name);
        var threads = Wire.Array(_catalog, "threads").OfType<JsonObject>().Where(t => Wire.Text(t, "projectId") == id).ToList();
        var selectedMode = Wire.Text(Wire.Object(Selection, "projectModes"), id, Wire.Text(Selection, "defaultProjectMode", "full"));
        var size = NativePageUi.Text(ProjectDetail(project, threads, selectedMode), 12, true);
        details.Children.Add(size);
        Grid.SetColumn(details, 1); row.Children.Add(details);
        var folders = new StackPanel { Spacing = 2, VerticalAlignment = VerticalAlignment.Center };
        var roots = Wire.Array(project, "roots");
        var localRoots = Wire.Array(project, "localRoots");
        for (var index = 0; index < Math.Max(1, roots.Count); index++)
        {
            var rootIndex = index;
            var key = $"{id}:{index}";
            var discovered = index < roots.Count ? roots[index]?.GetValue<string>() ?? "" : "";
            var path = Wire.Text(Wire.Object(_draft, "sourceRoots"), key,
                index < localRoots.Count ? localRoots[index]?.GetValue<string>() ?? discovered : discovered);
            folders.Children.Add(NativePageUi.FolderControl(path, async () =>
            {
                try
                {
                    var chosen = await _context.PickFolderAsync(path);
                    if (string.IsNullOrEmpty(chosen)) return;
                    var sources = NativePageUi.EnsureObject(_draft, "sourceRoots");
                    var destinations = NativePageUi.EnsureObject(_draft, "destinationRoots");
                    if (rootIndex == 0) { sources.Remove(id); destinations.Remove(id); }
                    sources[key] = chosen;
                    destinations[key] = chosen;
                    MarkChanged(false);
                    Render();
                    await LoadSizesAsync();
                }
                catch (Exception error) { NativePageUi.Error(_feedback, error); }
            }, $"Change folder for {Wire.Text(project, "name")}"));
        }
        Grid.SetColumn(folders, 2); row.Children.Add(folders);
        AdaptProjectColumns(row, folders);
        var mode = NativePageUi.ModePicker(selectedMode);
        mode.VerticalAlignment = VerticalAlignment.Center;
        mode.SelectionChanged += (_, _) =>
        {
            var value = NativePageUi.ModeValue(mode);
            NativePageUi.EnsureObject(Selection, "projectModes")[id] = value;
            size.Text = ProjectDetail(project, threads, value);
            MarkChanged(true);
        };
        AutomationProperties.SetName(mode, $"Sync mode for {Wire.Text(project, "name")}");
        Grid.SetColumn(mode, 3); row.Children.Add(mode);
        return row;
    }

    private string ProjectDetail(JsonObject project, List<JsonObject> threads, string mode)
    {
        var excluded = Wire.Array(Selection, "excludedThreadIds").Select(n => n?.GetValue<string>()).ToHashSet();
        var included = threads.Count(t => !excluded.Contains(Wire.Text(t, "id")) && (Wire.Bool(Selection, "includeArchived", true) || !Wire.Bool(t, "archived")));
        return mode == "excluded" ? "Not syncing" : $"{included} {(included == 1 ? "chat" : "chats")} · {ProjectSize(project, threads, mode)}";
    }

    private string ProjectSize(JsonObject project, List<JsonObject> threads, string mode)
    {
        if (mode == "excluded") return "Not syncing";
        var excluded = Wire.Array(Selection, "excludedThreadIds").Select(n => n?.GetValue<string>()).ToHashSet();
        var history = threads.Where(t => !excluded.Contains(Wire.Text(t, "id")) && (Wire.Bool(Selection, "includeArchived", true) || !Wire.Bool(t, "archived"))).Sum(t => Wire.Number(t, "estimatedBytes"));
        return mode == "historyOnly" ? NativePageUi.Bytes(history) : _sizesReady ? NativePageUi.Bytes(history + Wire.Number(project, "estimatedBytes")) : "Calculating…";
    }

    private Grid ChatRow(JsonObject thread)
    {
        var id = Wire.Text(thread, "id");
        var excluded = Wire.Array(Selection, "excludedThreadIds").Select(n => n?.GetValue<string>()).ToHashSet();
        var row = NativePageUi.RowGrid(-1, 100);
        row.Padding = new Thickness(0, 10, 4, 10);
        row.Style = Ui.Style("SpiceLineBottomGridStyle"); row.BorderThickness = new Thickness(0, 0, 0, 1);
        var label = new StackPanel { Spacing = 3 };
        var title = Ui.Text(Wire.Text(thread, "title", "Untitled chat"), 13, true);
        title.TextWrapping = TextWrapping.NoWrap; title.TextTrimming = TextTrimming.CharacterEllipsis;
        ToolTipService.SetToolTip(title, title.Text); label.Children.Add(title);
        var projectId = Wire.Text(thread, "projectId");
        var project = Wire.Array(_catalog, "projects").OfType<JsonObject>().FirstOrDefault(p => Wire.Text(p, "id") == projectId);
        var projectMode = Wire.Text(Wire.Object(Selection, "projectModes"), projectId, Wire.Text(Selection, "defaultProjectMode", "full"));
        var inheritedExclusion = projectId.Length > 0 && projectMode == "excluded";
        var archivedExclusion = Wire.Bool(thread, "archived") && !Wire.Bool(Selection, "includeArchived", true);
        var detail = string.Join(" · ", new[] { project is null ? "" : Wire.Text(project, "name"), Wire.Bool(thread, "archived") ? "Archived" : "", inheritedExclusion ? "Excluded by project setting" : archivedExclusion ? "Archived chats are turned off" : "" }.Where(s => s.Length > 0));
        if (detail.Length > 0) label.Children.Add(NativePageUi.Text(detail, 12, true));
        var check = new CheckBox { Content = label, IsChecked = !excluded.Contains(id) && !inheritedExclusion && !archivedExclusion, IsEnabled = !inheritedExclusion && !archivedExclusion, HorizontalContentAlignment = HorizontalAlignment.Stretch };
        AutomationProperties.SetName(check, $"Sync {Wire.Text(thread, "title", "chat")}");
        void Changed()
        {
            var values = Wire.Array(Selection, "excludedThreadIds").Select(n => n?.GetValue<string>()).Where(v => v is not null).Cast<string>().ToHashSet();
            if (check.IsChecked == true) values.Remove(id); else values.Add(id);
            Selection["excludedThreadIds"] = new JsonArray(values.OrderBy(v => v).Select(v => (JsonNode?)JsonValue.Create(v)).ToArray());
            MarkChanged(true);
        }
        check.Checked += (_, _) => Changed(); check.Unchecked += (_, _) => Changed();
        row.Children.Add(check);
        var size = NativePageUi.Text(NativePageUi.Bytes(Wire.Number(thread, "estimatedBytes")), 12, true);
        size.VerticalAlignment = VerticalAlignment.Center; size.HorizontalAlignment = HorizontalAlignment.Right;
        Grid.SetColumn(size, 1); row.Children.Add(size);
        return row;
    }

    private async Task SaveAsync()
    {
        IsEnabled = false;
        _save.Content = "Saving…";
        _save.IsEnabled = false;
        try
        {
            if (_selectionChanged) Selection["revision"] = Guid.NewGuid().ToString();
            await _context.SaveConfigAsync((JsonObject)_draft.DeepClone());
            _selectionChanged = false;
            _saved.Text = "Choices saved";
            _feedbackTimer.Stop(); _feedbackTimer.Start();
            _feedback.IsOpen = false;
        }
        catch (Exception error) { _save.IsEnabled = true; NativePageUi.Error(_feedback, error); }
        finally { IsEnabled = true; _save.Content = "Save choices"; }
    }
}

internal static class NativePageUi
{
    public static TextBlock Text(string text, double size = 14, bool secondary = false)
    {
        return secondary ? Ui.Muted(text, size) : Ui.Text(text, size);
    }
    public static JsonObject EnsureObject(JsonObject owner, string key)
    {
        if (owner[key] is not JsonObject value) { value = new JsonObject(); owner[key] = value; }
        return value;
    }
    public static Grid PageGrid(string title, FrameworkElement? action, out Grid content)
    {
        var grid = new Grid { Padding = new Thickness(0, 0, 0, 20), RowSpacing = 14 };
        grid.RowDefinitions.Add(new() { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        var header = RowGrid(-1, 180);
        header.Children.Add(Ui.PageTitle(title));
        if (action is not null) { Grid.SetColumn(action, 1); header.Children.Add(action); if (action is FrameworkElement control) control.HorizontalAlignment = HorizontalAlignment.Right; }
        grid.Children.Add(header);
        content = new Grid(); Grid.SetRow(content, 1); grid.Children.Add(content);
        return grid;
    }
    public static Grid RowGrid(params double[] columns)
    {
        var grid = new Grid { ColumnSpacing = 12, HorizontalAlignment = HorizontalAlignment.Stretch };
        foreach (var width in columns) grid.ColumnDefinitions.Add(new() { Width = width < 0 ? new GridLength(1, GridUnitType.Star) : new GridLength(width) });
        return grid;
    }
    public static ComboBox ModePicker(string mode)
    {
        var combo = new ComboBox { Width = 164, MinHeight = 32, FontSize = 13, CornerRadius = new CornerRadius(4), Padding = new Thickness(10, 4, 10, 4) };
        foreach (var pair in new[] { ("Full project", "full"), ("Chat history only", "historyOnly"), ("Excluded", "excluded") })
            combo.Items.Add(new ComboBoxItem { Content = pair.Item1, Tag = pair.Item2 });
        combo.SelectedIndex = mode == "historyOnly" ? 1 : mode == "excluded" ? 2 : 0;
        return combo;
    }
    public static string ModeValue(ComboBox combo) => (combo.SelectedItem as ComboBoxItem)?.Tag?.ToString() ?? "full";
    public static Grid FolderControl(string path, Func<Task> choose, string label)
    {
        var grid = Ui.ColumnsWithSpacing(6, new GridLength(16), new GridLength(1, GridUnitType.Star), new GridLength(32));
        grid.MinHeight = 32;
        var icon = Ui.Icon("\uE8B7", 13);
        icon.Style = Ui.Style("SpiceMutedIconStyle");
        icon.VerticalAlignment = VerticalAlignment.Center;
        grid.Children.Add(icon);
        var text = Text(string.IsNullOrEmpty(path) ? "Choose a folder" : FolderName(path), 12, true);
        text.TextWrapping = TextWrapping.NoWrap; text.TextTrimming = TextTrimming.CharacterEllipsis; text.VerticalAlignment = VerticalAlignment.Center;
        ToolTipService.SetToolTip(text, path);
        AutomationProperties.SetName(text, string.IsNullOrEmpty(path) ? "No folder selected" : path);
        Grid.SetColumn(text, 1);
        grid.Children.Add(text);
        var button = Ui.IconButton(label, "\uE712");
        button.HorizontalAlignment = HorizontalAlignment.Right;
        AutomationProperties.SetName(button, label); AutomationProperties.SetHelpText(button, path);
        ToolTipService.SetToolTip(button, string.IsNullOrEmpty(path) ? label : $"{label}\n{path}");
        button.Click += async (_, _) => await choose();
        Grid.SetColumn(button, 2); grid.Children.Add(button);
        return grid;
    }
    public static string FolderName(string path)
    {
        var parts = path.TrimEnd('\\', '/').Split(new[] { '\\', '/' }, StringSplitOptions.RemoveEmptyEntries);
        return string.Join("\\", parts.Skip(Math.Max(0, parts.Length - 2)));
    }
    public static string Bytes(double bytes)
    {
        if (bytes <= 0) return "0 B";
        var units = new[] { "B", "KB", "MB", "GB", "TB" }; var unit = 0;
        while (bytes >= 1024 && unit < units.Length - 1) { bytes /= 1024; unit++; }
        return $"{bytes:0.#} {units[unit]}";
    }
    public static void Error(InfoBar bar, Exception error) { bar.Severity = InfoBarSeverity.Error; bar.Title = "Something needs attention"; bar.Message = error.Message; bar.IsOpen = true; }
    public static string Time(string value) => DateTimeOffset.TryParse(value, out var time) ? time.ToLocalTime().ToString("g") : value;
}
