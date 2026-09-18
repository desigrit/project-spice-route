using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class SelectionPage : Page
{
    private readonly SpiceRouteContext _context;
    private readonly JsonObject _draft;
    private JsonObject _catalog;
    private readonly ListView _items = new() { SelectionMode = ListViewSelectionMode.None, HorizontalContentAlignment = HorizontalAlignment.Stretch };
    private readonly TextBox _search = new() { PlaceholderText = "Search projects", Width = 280 };
    private readonly ComboBox _scope = new() { Width = 190 };
    private readonly Button _save = new() { Content = "Save choices", IsEnabled = false };
    private readonly InfoBar _feedback = new() { IsOpen = false, IsClosable = true };
    private readonly TextBlock _state = NativePageUi.Text("Calculating selected sizes…", secondary: true);
    private readonly CancellationTokenSource _lifetime = new();
    private bool _sizesReady;
    private bool _selectionChanged;
    private int _sizeGeneration;

    public SelectionPage(SpiceRouteContext context)
    {
        _context = context;
        _draft = (JsonObject)context.Config.DeepClone();
        _catalog = (JsonObject)context.Catalog.DeepClone();
        _save.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        var body = NativePageUi.PageGrid("What to sync", _save, out var content);
        content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        content.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        var top = new StackPanel { Spacing = 12 };
        top.Children.Add(NativePageUi.Text("Choose what travels. Each project keeps its own local folder on this PC.", secondary: true));
        top.Children.Add(_feedback);
        var tools = new Grid { ColumnSpacing = 12 };
        tools.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        tools.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        tools.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        foreach (var label in new[] { "Projects", "Project chats", "Projectless chats" }) _scope.Items.Add(label);
        _scope.SelectedIndex = 0;
        tools.Children.Add(_scope);
        Grid.SetColumn(_search, 2);
        tools.Children.Add(_search);
        top.Children.Add(tools);
        content.Children.Add(top);
        var defaults = new Grid { Margin = new Thickness(0, 16, 0, 12), ColumnSpacing = 16 };
        defaults.ColumnDefinitions.Add(new() { Width = new GridLength(1, GridUnitType.Star) });
        defaults.ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        var caption = new StackPanel { Spacing = 4 };
        caption.Children.Add(NativePageUi.Text("Default for new projects"));
        caption.Children.Add(_state);
        defaults.Children.Add(caption);
        var mode = NativePageUi.ModePicker(Wire.Text(Selection, "defaultProjectMode", "full"));
        mode.SelectionChanged += (_, _) => { Selection["defaultProjectMode"] = NativePageUi.ModeValue(mode); MarkChanged(true); Render(); };
        Grid.SetColumn(mode, 1);
        defaults.Children.Add(mode);
        Grid.SetRow(defaults, 1);
        content.Children.Add(defaults);
        Grid.SetRow(_items, 2);
        content.Children.Add(_items);
        Content = body;
        _save.Click += async (_, _) => await SaveAsync();
        _scope.SelectionChanged += (_, _) => { _search.PlaceholderText = _scope.SelectedIndex == 0 ? "Search projects" : "Search chats"; Render(); };
        _search.TextChanged += (_, _) => Render();
        Loaded += async (_, _) => await LoadSizesAsync();
        Unloaded += (_, _) => _lifetime.Cancel();
        Render();
    }

    private JsonObject Selection => NativePageUi.EnsureObject(_draft, "selection");
    private void MarkChanged(bool selection)
    {
        _selectionChanged |= selection;
        _save.IsEnabled = true;
    }

    private async Task LoadSizesAsync()
    {
        var generation = ++_sizeGeneration;
        _sizesReady = false;
        _state.Text = "Calculating workspace sizes. You can keep choosing.";
        try
        {
            var result = await _context.Engine.CallAsync("list_content", new JsonObject { ["config"] = _draft.DeepClone() }, _lifetime.Token);
            if (_lifetime.IsCancellationRequested || generation != _sizeGeneration) return;
            _catalog = result as JsonObject ?? new JsonObject();
            _sizesReady = true;
            _state.Text = "Individual project choices override the default.";
            Render();
        }
        catch (OperationCanceledException) { }
        catch (Exception error) { if (!_lifetime.IsCancellationRequested && generation == _sizeGeneration) { _state.Text = "Workspace sizes are unavailable."; NativePageUi.Error(_feedback, error); } }
    }

    private void Render()
    {
        _items.Items.Clear();
        var query = _search.Text.Trim();
        if (_scope.SelectedIndex == 0)
        {
            foreach (var project in Wire.Array(_catalog, "projects").OfType<JsonObject>().Where(p => Wire.Text(p, "name").Contains(query, StringComparison.OrdinalIgnoreCase)))
                _items.Items.Add(ProjectRow(project));
        }
        else
        {
            var projectless = _scope.SelectedIndex == 2;
            foreach (var thread in Wire.Array(_catalog, "threads").OfType<JsonObject>().Where(t => Wire.Bool(t, "projectless") == projectless && Wire.Text(t, "title").Contains(query, StringComparison.OrdinalIgnoreCase)))
                _items.Items.Add(ChatRow(thread));
        }
        if (_items.Items.Count == 0) _items.Items.Add(NativePageUi.Text(query.Length > 0 ? "No matches. Try another search." : "No items were found in the configured Codex folders.", secondary: true));
    }

    private Grid ProjectRow(JsonObject project)
    {
        var id = Wire.Text(project, "id");
        var row = NativePageUi.RowGrid(40, -1, 110, 172);
        row.Margin = new Thickness(0, 7, 0, 7);
        row.Children.Add(new SymbolIcon(Symbol.Folder) { VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, 4, 12, 0) });
        var details = new StackPanel { Spacing = 4 };
        details.Children.Add(NativePageUi.Text(Wire.Text(project, "name", "Project")));
        var threads = Wire.Array(_catalog, "threads").OfType<JsonObject>().Where(t => Wire.Text(t, "projectId") == id).ToList();
        details.Children.Add(NativePageUi.Text($"{threads.Count} {(threads.Count == 1 ? "chat" : "chats")}", 12, true));
        var roots = Wire.Array(project, "roots");
        var localRoots = Wire.Array(project, "localRoots");
        for (var index = 0; index < Math.Max(1, roots.Count); index++)
        {
            var rootIndex = index;
            var key = $"{id}:{index}";
            var discovered = index < roots.Count ? roots[index]?.GetValue<string>() ?? "" : "";
            var path = Wire.Text(Wire.Object(_draft, "sourceRoots"), key,
                index < localRoots.Count ? localRoots[index]?.GetValue<string>() ?? discovered : discovered);
            details.Children.Add(NativePageUi.FolderControl(path, async () =>
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
        Grid.SetColumn(details, 1); row.Children.Add(details);
        var selectedMode = Wire.Text(Wire.Object(Selection, "projectModes"), id, Wire.Text(Selection, "defaultProjectMode", "full"));
        var size = NativePageUi.Text(ProjectSize(project, threads, selectedMode), 12, true);
        size.VerticalAlignment = VerticalAlignment.Top; size.Margin = new Thickness(0, 6, 0, 0);
        Grid.SetColumn(size, 2); row.Children.Add(size);
        var mode = NativePageUi.ModePicker(selectedMode);
        mode.VerticalAlignment = VerticalAlignment.Top;
        mode.SelectionChanged += (_, _) =>
        {
            var value = NativePageUi.ModeValue(mode);
            NativePageUi.EnsureObject(Selection, "projectModes")[id] = value;
            size.Text = ProjectSize(project, threads, value);
            MarkChanged(true);
        };
        AutomationProperties.SetName(mode, $"Sync mode for {Wire.Text(project, "name")}");
        Grid.SetColumn(mode, 3); row.Children.Add(mode);
        return row;
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
        row.Margin = new Thickness(0, 6, 0, 6);
        var label = new StackPanel { Spacing = 3 };
        label.Children.Add(NativePageUi.Text(Wire.Text(thread, "title", "Untitled chat")));
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
        size.VerticalAlignment = VerticalAlignment.Center;
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
            _state.Text = "Sync choices saved.";
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
        var block = Ui.Text(text, size);
        if (secondary) block.Foreground = Ui.Resource("SpiceTextSecondary");
        return block;
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
        var combo = new ComboBox { Width = 156, MinHeight = 32 };
        foreach (var pair in new[] { ("Full project", "full"), ("Chat history only", "historyOnly"), ("Excluded", "excluded") })
            combo.Items.Add(new ComboBoxItem { Content = pair.Item1, Tag = pair.Item2 });
        combo.SelectedIndex = mode == "historyOnly" ? 1 : mode == "excluded" ? 2 : 0;
        return combo;
    }
    public static string ModeValue(ComboBox combo) => (combo.SelectedItem as ComboBoxItem)?.Tag?.ToString() ?? "full";
    public static Grid FolderControl(string path, Func<Task> choose, string label)
    {
        var grid = RowGrid(-1, 36);
        var text = Text(string.IsNullOrEmpty(path) ? "Choose a folder" : FolderName(path), 12, true);
        text.TextWrapping = TextWrapping.NoWrap; text.TextTrimming = TextTrimming.CharacterEllipsis; text.VerticalAlignment = VerticalAlignment.Center;
        ToolTipService.SetToolTip(text, path);
        grid.Children.Add(text);
        var button = Ui.IconButton(label, "\uE712");
        button.HorizontalAlignment = HorizontalAlignment.Right;
        AutomationProperties.SetName(button, label); ToolTipService.SetToolTip(button, label);
        button.Click += async (_, _) => await choose();
        Grid.SetColumn(button, 1); grid.Children.Add(button);
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
