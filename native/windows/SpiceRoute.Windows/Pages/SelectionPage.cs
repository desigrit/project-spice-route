using System.ComponentModel;
using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Markup;

namespace SpiceRoute.Windows;

[Microsoft.UI.Xaml.Data.Bindable]
public sealed class SyncChoiceRow : INotifyPropertyChanged
{
    public JsonObject Source { get; }
    public string Id => Wire.Text(Source, "id");
    public string Name => Wire.Text(Source, Source.ContainsKey("title") ? "title" : "name", "Untitled chat");
    public string ModeLabel { get; private set; } = "";
    public string Size { get; private set; } = "";
    public string Detail { get; private set; } = "";
    public bool Allowed { get; private set; } = true;
    private bool included;
    private readonly Action<SyncChoiceRow, bool>? onInclude;
    public bool Included { get => included; set { if (included == value) return; included = value; onInclude?.Invoke(this, value); Notify(); } }
    public string AccessibleName => $"{Name}, {ModeLabel}, {Size}";
    public event PropertyChangedEventHandler? PropertyChanged;
    public SyncChoiceRow(JsonObject source, Action<SyncChoiceRow, bool>? onInclude = null) { Source = source; this.onInclude = onInclude; }
    public void Update(string mode, string size, string detail = "", bool allowed = true, bool selected = false)
    { ModeLabel = mode; Size = size; Detail = detail; Allowed = allowed; included = selected; Notify(); }
    private void Notify() => PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(""));
}

public sealed class SelectionPage : Page
{
    private readonly SpiceRouteContext _context;
    private readonly JsonObject _draft;
    private JsonObject _catalog;
    private readonly ListView _items = new() { Style = Ui.Style("SpiceReviewListStyle"), SelectionMode = ListViewSelectionMode.Single };
    private readonly TextBox _search = new() { PlaceholderText = "Find a project", Width = 260, MinHeight = 32, FontSize = 13, HorizontalAlignment = HorizontalAlignment.Left };
    private readonly SelectorBar _scope = new();
    private readonly SelectorBarItem _projectsTab = new() { Text = "Projects" };
    private readonly SelectorBarItem _projectChatsTab = new() { Text = "Project chats" };
    private readonly SelectorBarItem _projectlessTab = new() { Text = "Projectless chats" };
    private readonly Button _save = Ui.Button("Save choices", primary: true);
    private readonly InfoBar _feedback = new() { IsOpen = false, IsClosable = true };
    private readonly TextBlock _summary = Ui.Muted("", 12);
    private readonly TextBlock _state = Ui.Muted("Project folders are specific to this device.", 12);
    private readonly TextBlock _saved = Ui.Muted("", 12);
    private readonly TextBlock _empty = Ui.Muted("No projects match your search.", 13);
    private readonly Grid _workspace = new();
    private readonly Grid _table = new();
    private readonly Grid _headers = ProjectColumns();
    private readonly StackPanel _inspector = new() { Spacing = 0 };
    private readonly Border _inspectorBorder = new() { Style = Ui.Style("SpiceLineBottomBorderStyle") };
    private readonly DispatcherTimer _feedbackTimer = new() { Interval = TimeSpan.FromSeconds(4) };
    private readonly CancellationTokenSource _lifetime = new();
    private List<SyncChoiceRow> _projects = new(), _threads = new();
    private string? _selectedId;
    private bool _sizesReady, _sizeFailed, _selectionChanged, _filtering, _narrow;
    private TextBlock? _inspectorSize;
    private int _sizeGeneration;
    private readonly DataTemplate _projectTemplate;
    private readonly DataTemplate _chatTemplate;

    public SelectionPage(SpiceRouteContext context)
    {
        _context = context; _draft = (JsonObject)context.Config.DeepClone(); _catalog = (JsonObject)context.Catalog.DeepClone();
        _projectTemplate = (DataTemplate)Application.Current.Resources["SpiceSyncProjectTemplate"];
        _chatTemplate = (DataTemplate)Application.Current.Resources["SpiceSyncChatTemplate"];
        _items.ItemContainerStyle = (Style)Application.Current.Resources["SpiceSyncRowStyle"];
        _save.IsEnabled = false;
        AutomationProperties.SetName(_search, "Search sync choices"); AutomationProperties.SetName(_items, "Content to sync");
        AutomationProperties.SetName(_summary, "Selected content summary");
        AutomationProperties.SetLiveSetting(_saved, AutomationLiveSetting.Polite); _saved.Style = Ui.Style("SpiceSuccessTextStyle");
        var defaults = Ui.TextButton("Defaults"); defaults.Flyout = BuildDefaults();
        var body = NativePageUi.PageGrid("What to sync", defaults, out var content);
        content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        content.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        var top = new StackPanel { Spacing = 12 };
        top.Children.Add(_summary); top.Children.Add(_feedback);
        _scope.Items.Add(_projectsTab); _scope.Items.Add(_projectChatsTab); _scope.Items.Add(_projectlessTab); _scope.SelectedItem = _projectsTab; top.Children.Add(_scope);
        var tools = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto);
        tools.Padding = new Thickness(0, 0, 0, 14); tools.Children.Add(_search);
        var shared = Ui.Muted("Shared across devices", 12); shared.VerticalAlignment = VerticalAlignment.Center; Ui.Add(tools, shared, column: 1); top.Children.Add(tools);
        content.Children.Add(top);
        _workspace.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        _workspace.ColumnDefinitions.Add(new() { Width = new GridLength(238) });
        _workspace.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        _workspace.RowDefinitions.Add(new() { Height = new GridLength(0) });
        _table.RowDefinitions.Add(new() { Height = GridLength.Auto }); _table.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        _headers.MinHeight = 36; _headers.Padding = new Thickness(12, 0, 12, 0);
        _headers.Style = Ui.Style("SpiceLineBottomGridStyle"); _headers.BorderThickness = new Thickness(0, 0, 0, 1);
        foreach (var (label, column) in new[] { ("Project", 0), ("Sync", 1), ("Size", 2) })
        {
            var text = Ui.Muted(label, 12); text.VerticalAlignment = VerticalAlignment.Center;
            if (column == 2) text.HorizontalAlignment = HorizontalAlignment.Right;
            Ui.Add(_headers, text, column: column);
        }
        _table.Children.Add(_headers); Ui.Add(_table, _items, row: 1);
        _empty.Margin = new Thickness(12, 24, 12, 0); _empty.VerticalAlignment = VerticalAlignment.Top; Ui.Add(_table, _empty, row: 1);
        _workspace.Children.Add(_table);
        _inspectorBorder.Child = new ScrollViewer { Content = _inspector, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
        AutomationProperties.SetName(_inspectorBorder, "Project details");
        Ui.Add(_workspace, _inspectorBorder, column: 1); Ui.Add(content, _workspace, row: 1);
        var footer = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto, GridLength.Auto);
        footer.Margin = new Thickness(0, 14, 0, 0); footer.Padding = new Thickness(0, 12, 0, 0);
        footer.Style = Ui.Style("SpiceLineTopGridStyle"); footer.BorderThickness = new Thickness(0, 1, 0, 0);
        _state.VerticalAlignment = VerticalAlignment.Center; _saved.VerticalAlignment = VerticalAlignment.Center;
        footer.Children.Add(_state); Ui.Add(footer, _saved, column: 1); Ui.Add(footer, _save, column: 2); Ui.Add(content, footer, row: 2);
        Content = body;
        _save.Click += async (_, _) => await SaveAsync();
        _scope.SelectionChanged += (_, _) => { _search.PlaceholderText = ProjectScope ? "Find a project" : "Find a chat"; ApplyFilter(); };
        _search.TextChanged += (_, _) => ApplyFilter();
        _items.SelectionChanged += (_, _) => { if (!_filtering && ProjectScope && _items.SelectedItem is SyncChoiceRow project) { _selectedId = project.Id; RenderInspector(); } };
        _feedbackTimer.Tick += (_, _) => { _saved.Text = ""; _feedbackTimer.Stop(); };
        SizeChanged += (_, args) => { var next = args.NewSize.Width < 720; if (next != _narrow) { _narrow = next; RenderInspector(); } Adapt(); };
        Loaded += async (_, _) => await LoadSizesAsync();
        Unloaded += (_, _) => { _lifetime.Cancel(); _feedbackTimer.Stop(); };
        RebuildRows(); Adapt();
    }

    private JsonObject Selection => NativePageUi.EnsureObject(_draft, "selection");
    private bool ProjectScope => _scope.SelectedItem == _projectsTab;
    private static Grid ProjectColumns() => Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), new GridLength(137), new GridLength(70));
    private void MarkChanged(bool selection)
    { _selectionChanged |= selection; _save.IsEnabled = true; _saved.Text = ""; UpdateRows(); }

    private Flyout BuildDefaults()
    {
        var body = new StackPanel { Spacing = 16, Width = 300 };
        body.Children.Add(Ui.SectionTitle("Sync defaults"));
        var mode = NativePageUi.ModePicker(Wire.Text(Selection, "defaultProjectMode", "full")); mode.Header = "New projects"; mode.Width = 300;
        AutomationProperties.SetName(mode, "Default sync mode for new projects");
        mode.SelectionChanged += (_, _) => { Selection["defaultProjectMode"] = NativePageUi.ModeValue(mode); MarkChanged(true); RenderInspector(); };
        body.Children.Add(mode);
        foreach (var (key, label, fallback) in new[] { ("includeArchived", "Include archived chats", true), ("includeSensitiveFiles", "Include project secrets", true), ("includeBuildOutputs", "Include build and dependency folders", false) })
        {
            var toggle = new ToggleSwitch { Header = label, IsOn = Wire.Bool(Selection, key, fallback) };
            toggle.Toggled += async (_, _) => { Selection[key] = toggle.IsOn; MarkChanged(true); RenderInspector(); if (key != "includeArchived") await LoadSizesAsync(); };
            body.Children.Add(toggle);
        }
        var patterns = new TextBox { Header = "Additional file exclusions", PlaceholderText = "coverage/**, *.iso", Text = string.Join(", ", Wire.Array(Selection, "extraExcludePatterns").Select(node => node?.ToString())) };
        patterns.LostFocus += async (_, _) =>
        {
            var values = patterns.Text.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            var next = new JsonArray(values.Select(value => (JsonNode?)JsonValue.Create(value)).ToArray());
            if (next.ToJsonString() == Wire.Array(Selection, "extraExcludePatterns").ToJsonString()) return;
            Selection["extraExcludePatterns"] = next; MarkChanged(true); await LoadSizesAsync();
        };
        body.Children.Add(patterns); body.Children.Add(Ui.Muted("Applies to future handoffs. Existing cloud history is kept.", 12));
        return new Flyout { Content = new ScrollViewer { Content = body, MaxHeight = 520, VerticalScrollBarVisibility = ScrollBarVisibility.Auto } };
    }

    private void RebuildRows()
    {
        _projects = Wire.Array(_catalog, "projects").OfType<JsonObject>().Select(project => new SyncChoiceRow(project)).ToList();
        _threads = Wire.Array(_catalog, "threads").OfType<JsonObject>().Select(thread => new SyncChoiceRow(thread, (row, included) =>
        {
            var excluded = Wire.Array(Selection, "excludedThreadIds").Select(node => node?.ToString()).Where(id => id is not null).Cast<string>().ToHashSet();
            if (included) excluded.Remove(row.Id); else excluded.Add(row.Id);
            Selection["excludedThreadIds"] = new JsonArray(excluded.OrderBy(id => id).Select(id => (JsonNode?)JsonValue.Create(id)).ToArray());
            MarkChanged(true);
        })).ToList();
        UpdateRows(); ApplyFilter();
    }

    private void UpdateRows()
    {
        foreach (var row in _projects)
        {
            var mode = SelectionSummary.Mode(_draft, row.Id);
            row.Update(SelectionSummary.ModeLabel(mode), ProjectSize(row.Source));
        }
        foreach (var row in _threads)
        {
            var id = Wire.Text(row.Source, "projectId");
            var project = _projects.FirstOrDefault(project => project.Id == id);
            var allowed = (id.Length == 0 || SelectionSummary.Mode(_draft, id) != "excluded") && (!Wire.Bool(row.Source, "archived") || Wire.Bool(Selection, "includeArchived", true));
            var detail = !allowed ? "Excluded by project or archive settings" : string.Join(" · ", new[] { project?.Name ?? "Projectless chat", Wire.Bool(row.Source, "archived") ? "Archived" : "" }.Where(value => value.Length > 0));
            row.Update("", Wire.Bytes(Wire.Number(row.Source, "estimatedBytes")), detail, allowed, SelectionSummary.Includes(_draft, row.Source));
        }
        var summary = SelectionSummary.Count(_draft, _catalog);
        var bytes = summary.Full > 0 && !_sizesReady ? _sizeFailed ? "Size unavailable" : "Calculating size…" : Wire.Bytes(summary.Bytes) + " selected";
        _summary.Text = $"{summary.Chats} chats · {summary.Full + summary.History} projects · {bytes}";
        var selected = _projects.FirstOrDefault(row => row.Id == _selectedId);
        if (_inspectorSize is not null && selected is not null) _inspectorSize.Text = ProjectSize(selected.Source);
    }

    private void ApplyFilter()
    {
        var query = _search.Text.Trim();
        var projectless = _scope.SelectedItem == _projectlessTab;
        var rows = (ProjectScope ? _projects : _threads.Where(row => Wire.Bool(row.Source, "projectless") == projectless))
            .Where(row => (row.Name + " " + row.Detail + " " + (ProjectScope ? string.Join(" ", Wire.Array(row.Source, "roots").Select(node => node?.ToString())) : "")).Contains(query, StringComparison.OrdinalIgnoreCase)).ToList();
        _filtering = true;
        _items.SelectionMode = ProjectScope ? ListViewSelectionMode.Single : ListViewSelectionMode.None;
        _items.ItemTemplate = ProjectScope ? _projectTemplate : _chatTemplate;
        _items.ItemsSource = rows;
        if (ProjectScope)
        {
            var selected = rows.FirstOrDefault(row => row.Id == _selectedId) ?? rows.FirstOrDefault();
            _selectedId = selected?.Id; _items.SelectedItem = selected;
        }
        _filtering = false;
        _headers.Visibility = ProjectScope ? Visibility.Visible : Visibility.Collapsed;
        _empty.Text = query.Length > 0 ? "No matches. Try another search." : ProjectScope ? "No projects found in your Codex profile." : "No chats found in this category.";
        _empty.Visibility = rows.Count == 0 ? Visibility.Visible : Visibility.Collapsed;
        RenderInspector(); Adapt();
    }

    private void Adapt()
    {
        var inspector = ProjectScope && _selectedId is not null;
        _inspectorBorder.Visibility = inspector ? Visibility.Visible : Visibility.Collapsed;
        _workspace.ColumnSpacing = inspector && !_narrow ? 22 : 0;
        _workspace.ColumnDefinitions[1].Width = inspector && !_narrow ? new GridLength(238) : new GridLength(0);
        _workspace.RowDefinitions[1].Height = inspector && _narrow ? new GridLength(230) : new GridLength(0);
        Grid.SetColumn(_inspectorBorder, _narrow ? 0 : 1); Grid.SetRow(_inspectorBorder, _narrow ? 1 : 0);
        _inspectorBorder.Padding = _narrow ? new Thickness(0, 14, 0, 0) : new Thickness(22, 14, 0, 0);
        _inspectorBorder.BorderThickness = _narrow ? new Thickness(0, 1, 0, 0) : new Thickness(1, 0, 0, 0);
        _search.Width = _narrow ? 205 : 260;
    }

    private string ProjectSize(JsonObject project) => SelectionSummary.Mode(_draft, Wire.Text(project, "id")) == "full" && !_sizesReady ? _sizeFailed ? "Size unavailable" : "Calculating…" : Wire.Bytes(SelectionSummary.ProjectBytes(_draft, _catalog, project));
    private void RenderInspector()
    {
        _inspectorSize = null;
        _inspector.Children.Clear();
        var project = _projects.FirstOrDefault(row => row.Id == _selectedId)?.Source;
        if (!ProjectScope || project is null) return;
        var id = Wire.Text(project, "id"); var name = Wire.Text(project, "name");
        var icon = Ui.Icon("\uE8B7", 28); icon.Style = Ui.Style("SpiceAccentIconStyle"); icon.HorizontalAlignment = HorizontalAlignment.Left; icon.Visibility = _narrow ? Visibility.Collapsed : Visibility.Visible; icon.Margin = new Thickness(0, 0, 0, 14); _inspector.Children.Add(icon);
        _inspector.Children.Add(Ui.Text(name, 16, true));
        var count = Wire.Array(_catalog, "threads").OfType<JsonObject>().Count(thread => Wire.Text(thread, "projectId") == id);
        _inspector.Children.Add(Ui.WithMargin(Ui.Muted($"{count} {(count == 1 ? "chat" : "chats")}", 12), new Thickness(0, 5, 0, 0)));
        var preferences = new StackPanel();
        var folders = new StackPanel();
        var groups = Ui.ColumnsWithSpacing(24, new GridLength(1, GridUnitType.Star), _narrow ? new GridLength(1, GridUnitType.Star) : new GridLength(0));
        groups.Margin = new Thickness(0, _narrow ? 14 : 24, 0, 0);
        groups.RowDefinitions.Add(new() { Height = GridLength.Auto }); groups.RowDefinitions.Add(new() { Height = GridLength.Auto });
        preferences.Children.Add(Ui.WithMargin(Ui.Muted("Include in handoff", 12), new Thickness(0, 0, 0, 8)));
        var mode = NativePageUi.ModePicker(SelectionSummary.Mode(_draft, id)); mode.Width = double.NaN; mode.HorizontalAlignment = HorizontalAlignment.Stretch;
        AutomationProperties.SetName(mode, $"Sync mode for {name}"); preferences.Children.Add(mode);
        var sizeRow = Ui.ColumnsWithSpacing(8, new GridLength(1, GridUnitType.Star), GridLength.Auto); sizeRow.Margin = new Thickness(0, 12, 0, 0);
        sizeRow.Children.Add(Ui.Muted("Selected content", 12));
        var size = Ui.Text(ProjectSize(project), 12, true); _inspectorSize = size; size.Name = "InspectorSelectedSize"; AutomationProperties.SetLiveSetting(size, AutomationLiveSetting.Polite); Ui.Add(sizeRow, size, column: 1); preferences.Children.Add(sizeRow);
        folders.Children.Add(Ui.Muted("Folder on this device", 12));
        var roots = Wire.Array(project, "roots"); var locals = Wire.Array(project, "localRoots");
        for (var index = 0; index < roots.Count; index++)
        {
            var rootIndex = index; var key = $"{id}:{index}";
            var discovered = roots[index]?.ToString() ?? "";
            var path = Wire.Text(Wire.Object(_draft, "sourceRoots"), key, index == 0 ? Wire.Text(Wire.Object(_draft, "sourceRoots"), id, index < locals.Count ? locals[index]?.ToString() ?? discovered : discovered) : index < locals.Count ? locals[index]?.ToString() ?? discovered : discovered);
            var folder = NativePageUi.FolderControl(path, async () =>
            {
                try
                {
                    var chosen = await _context.PickFolderAsync(path); if (string.IsNullOrEmpty(chosen)) return;
                    var sources = NativePageUi.EnsureObject(_draft, "sourceRoots"); var destinations = NativePageUi.EnsureObject(_draft, "destinationRoots");
                    if (rootIndex == 0) { sources.Remove(id); destinations.Remove(id); }
                    sources[key] = chosen; destinations[key] = chosen; MarkChanged(false); RenderInspector(); await LoadSizesAsync();
                }
                catch (Exception error) { NativePageUi.Error(_feedback, error); }
            }, roots.Count > 1 ? $"Change folder {index + 1} for {name}" : $"Change folder for {name}");
            folder.Margin = new Thickness(0, 7, 0, 0); folders.Children.Add(folder);
        }
        folders.Children.Add(Ui.WithMargin(Ui.Muted(roots.Count == 0 ? "No workspace folder is recorded." : "Changing this path does not move files.", 12), new Thickness(0, 9, 0, 0)));
        if (!_narrow) folders.Margin = new Thickness(0, 24, 0, 0);
        Ui.Add(groups, preferences); Ui.Add(groups, folders, row: _narrow ? 0 : 1, column: _narrow ? 1 : 0);
        _inspector.Children.Add(groups);
        _inspector.Children.Add(Ui.Rule(20, 18));
        var note = Ui.Muted("", 12); _inspector.Children.Add(note);
        void UpdateNote() => note.Text = SelectionSummary.Mode(_draft, id) switch { "full" => "Code, Git history, and selected working files are included.", "historyOnly" => "Chats and the project listing are included. Project files stay here.", _ => "Excluded from future handoffs. Local files stay here." };
        UpdateNote();
        mode.SelectionChanged += (_, _) => { NativePageUi.EnsureObject(Selection, "projectModes")[id] = NativePageUi.ModeValue(mode); MarkChanged(true); size.Text = ProjectSize(project); UpdateNote(); };
    }

    private async Task LoadSizesAsync()
    {
        var generation = ++_sizeGeneration; _sizesReady = false; _sizeFailed = false; UpdateRows();
        try
        {
            var result = await _context.Engine.CallAsync("list_content", new JsonObject { ["config"] = _draft.DeepClone() }, _lifetime.Token);
            if (_lifetime.IsCancellationRequested || generation != _sizeGeneration) return;
            _catalog = result as JsonObject ?? new(); _sizesReady = true; RebuildRows();
        }
        catch (OperationCanceledException) { }
        catch (Exception error) { if (!_lifetime.IsCancellationRequested && generation == _sizeGeneration) { _sizeFailed = true; UpdateRows(); _state.Text = "Workspace sizes are unavailable. Reopen this page to retry."; NativePageUi.Error(_feedback, error); } }
    }

    private async Task SaveAsync()
    {
        IsEnabled = false; _save.IsEnabled = false;
        try
        {
            if (_selectionChanged) Selection["revision"] = Guid.NewGuid().ToString();
            await _context.SaveConfigAsync((JsonObject)_draft.DeepClone()); _selectionChanged = false;
            _saved.Text = "Choices saved"; _feedbackTimer.Stop(); _feedbackTimer.Start(); _feedback.IsOpen = false;
        }
        catch (Exception error) { _save.IsEnabled = true; NativePageUi.Error(_feedback, error); }
        finally { IsEnabled = true; }
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
