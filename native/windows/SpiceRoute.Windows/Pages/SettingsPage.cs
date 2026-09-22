using System;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class SettingsPage : Page
{
    private readonly SpiceRouteContext _context;
    private readonly JsonObject _draft;
    private readonly Button _save = Ui.Button("Save settings", primary: true);
    private readonly Button _discard = Ui.TextButton("Discard changes");
    private readonly InfoBar _feedback = new() { IsOpen = false, IsClosable = true };
    private readonly TextBlock _saved = Ui.Muted("", 12);
    private readonly DispatcherTimer _savedTimer = new() { Interval = TimeSpan.FromSeconds(4) };
    private bool _policyChanged;
    private bool _dirty;

    public SettingsPage(SpiceRouteContext context)
    {
        _context = context;
        _draft = (JsonObject)context.Config.DeepClone();
        _save.IsEnabled = false;
        _discard.IsEnabled = false;
        var page = NativePageUi.PageGrid("Settings", null, out var content);
        content.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        var form = new StackPanel { Spacing = 24, MaxWidth = 920, HorizontalAlignment = HorizontalAlignment.Left, Margin = new Thickness(0, 0, 12, 16) };
        form.Children.Add(_feedback);
        var device = Group(form, "This device");
        var name = new TextBox { Text = Wire.Text(_draft, "deviceName"), MinHeight = 32, HorizontalAlignment = HorizontalAlignment.Stretch };
        AutomationProperties.SetName(name, "Device name");
        name.TextChanged += (_, _) => { _draft["deviceName"] = name.Text; Changed(); };
        AddRow(device, SettingRow("\uE77B", "Device name", "Shown beside the handoffs saved from this PC.", name));
        AddRow(device, Choice("\uE790", "Appearance", "Choose a theme or follow Windows.", "theme", new[] { ("Use Windows setting", "system"), ("Light", "light"), ("Dark", "dark") }));

        var cloud = Group(form, "Cloud drive");
        AddRow(cloud, Choice("\uE753", "Provider", "Your drive app handles sign-in and cloud delivery.", "cloudProvider", new[] { ("OneDrive", "oneDrive"), ("Google Drive", "googleDrive"), ("iCloud Drive", "iCloud"), ("Another folder", "custom") }));
        AddRow(cloud, Folder("Sync folder", "cloudRoot", "Shared snapshots only. Keep active Codex data and projects outside this folder."));

        var folders = Group(form, "Local folders");
        AddRow(folders, Folder("Codex tasks and history", "codexHome", "The .codex folder containing history databases and conversation transcripts."));
        AddRow(folders, Folder("Projectless chat workspaces", "projectlessRoot", "Working files and artifacts for chats outside a project."));
        AddRow(folders, Folder("Project discovery and restores", "projectsRoot", "Scan here for Git folders and suggest this location on Pull. Each project can use other folders too."));
        var projectLink = Ui.TextButton("Manage project folders", "\uE76C");
        projectLink.Click += async (_, _) =>
        {
            if (await ConfirmDiscardAsync()) context.Navigate("selection");
        };
        AddRow(folders, SettingRow("\uE8B7", "Individual projects", "Choose each project's folder in What to sync.", projectLink));

        var policy = Group(form, "Content preferences");
        var defaultMode = NativePageUi.ModePicker(Wire.Text(Selection, "defaultProjectMode", "full"));
        defaultMode.Width = double.NaN;
        defaultMode.HorizontalAlignment = HorizontalAlignment.Stretch;
        AutomationProperties.SetName(defaultMode, "Default sync mode for new projects");
        defaultMode.SelectionChanged += (_, _) =>
        {
            FreezeExistingProjects();
            Selection["defaultProjectMode"] = NativePageUi.ModeValue(defaultMode);
            Changed(true);
        };
        AddRow(policy, SettingRow("\uE8B7", "New projects", "Choose the sync mode for projects found in the future. Existing choices stay as they are.", defaultMode));
        var patterns = new TextBox
        {
            MinHeight = 32,
            Text = string.Join(", ", Wire.Array(Selection, "extraExcludePatterns").Select(n => n?.GetValue<string>()).Where(v => v is not null)),
            PlaceholderText = "coverage/**, *.iso"
        };
        AutomationProperties.SetName(patterns, "Additional exclusions for projectless workspace files");
        patterns.TextChanged += (_, _) =>
        {
            Selection["extraExcludePatterns"] = new JsonArray(patterns.Text.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
                .Select(value => (JsonNode?)JsonValue.Create(value)).ToArray());
            Changed(true);
        };
        AddRow(policy, SettingRow("\uE71C", "Additional exclusions", "For projectless files. Each project has its own file rules in What to sync.", patterns));

        var maintenance = Group(form, "About and storage");
        var version = Ui.Text(Wire.Text(context.Environment, "codexVersion", "Runtime not detected"), 13);
        version.HorizontalAlignment = HorizontalAlignment.Right;
        AddRow(maintenance, SettingRow("\uE946", "Codex compatibility", Wire.Text(Wire.Object(context.Environment, "compatibility"), "explanation", "Refresh Overview to check the configured Codex installation."), version));
        var reset = Ui.DangerButton("Reset");
        reset.Click += async (_, _) => await ResetCloudAsync();
        AddRow(maintenance, SettingRow("\uE74D", "Cloud history", "Remove shared snapshots and stored content. Local work stays here.", reset));
        var scroll = new ScrollViewer { Content = form, HorizontalContentAlignment = HorizontalAlignment.Left, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled };
        content.Children.Add(scroll);

        var footer = Ui.ColumnsWithSpacing(12, new GridLength(1, GridUnitType.Star), GridLength.Auto, GridLength.Auto);
        footer.MaxWidth = 920;
        footer.HorizontalAlignment = HorizontalAlignment.Left;
        scroll.SizeChanged += (_, args) => form.Width = footer.Width = Math.Max(0, Math.Min(920, args.NewSize.Width - 12));
        footer.Margin = new Thickness(0, 12, 12, 0);
        footer.Style = Ui.Style("SpiceLineTopGridStyle");
        footer.BorderThickness = new Thickness(0, 1, 0, 0);
        footer.Padding = new Thickness(0, 12, 0, 0);
        _saved.VerticalAlignment = VerticalAlignment.Center;
        Ui.Add(footer, _saved);
        Ui.Add(footer, _discard, column: 1);
        Ui.Add(footer, _save, column: 2);
        Grid.SetRow(footer, 1); content.Children.Add(footer);
        Content = page;
        _save.Click += async (_, _) => await SaveAsync();
        _discard.Click += (_, _) => context.Navigate("settings");
        _savedTimer.Tick += (_, _) => { _saved.Text = ""; _savedTimer.Stop(); };
        Unloaded += (_, _) => _savedTimer.Stop();
    }

    private JsonObject Selection => NativePageUi.EnsureObject(_draft, "selection");
    private void FreezeExistingProjects()
    {
        var legacy = !Wire.Bool(Selection, "projectModesInitialized");
        var modes = NativePageUi.EnsureObject(Selection, "projectModes");
        var content = NativePageUi.EnsureObject(Selection, "projectContent");
        foreach (var project in Wire.Array(_context.Catalog, "projects").OfType<JsonObject>())
        {
            var id = Wire.Text(project, "id");
            if (id.Length == 0) continue;
            if (!modes.ContainsKey(id)) modes[id] = Wire.Text(Selection, "defaultProjectMode", "full");
            if (!content.ContainsKey(id))
                content[id] = new JsonObject {
                    ["includeArchived"] = legacy ? Wire.Bool(Selection, "includeArchived", true) : true,
                    ["includeSensitiveFiles"] = legacy ? Wire.Bool(Selection, "includeSensitiveFiles", true) : true,
                    ["includeBuildOutputs"] = legacy && Wire.Bool(Selection, "includeBuildOutputs"),
                    ["extraExcludePatterns"] = legacy ? Wire.Array(Selection, "extraExcludePatterns").DeepClone() : new JsonArray()
                };
        }
        Selection["projectModesInitialized"] = true;
    }
    private void Changed(bool policy = false)
    {
        _savedTimer.Stop();
        _dirty = true; _save.IsEnabled = true; _discard.IsEnabled = true; _policyChanged |= policy; _saved.Text = "Unsaved changes";
    }
    internal static StackPanel Section(StackPanel form, string title)
    {
        var group = new StackPanel { Spacing = 12 };
        group.Children.Add(NativePageUi.Text(title, 18)); form.Children.Add(group); return group;
    }
    private static StackPanel Group(StackPanel form, string title)
    {
        var section = new StackPanel { Spacing = 6 };
        section.Children.Add(Ui.SectionTitle(title));
        var rows = new StackPanel { Spacing = 0 };
        section.Children.Add(rows); form.Children.Add(section);
        return rows;
    }
    private static void AddRow(StackPanel group, FrameworkElement row)
    {
        if (group.Children.Count > 0) group.Children.Add(Ui.WithMargin(Ui.Rule(), new Thickness(42, 0, 0, 0)));
        group.Children.Add(row);
    }
    private static Grid SettingRow(string glyph, string label, string description, FrameworkElement control)
    {
        var row = Ui.ColumnsWithSpacing(16, new GridLength(24), new GridLength(1, GridUnitType.Star), new GridLength(250));
        row.Padding = new Thickness(2, 13, 0, 13);
        row.MinHeight = 66;
        row.RowDefinitions.Add(new() { Height = GridLength.Auto });
        row.RowDefinitions.Add(new() { Height = GridLength.Auto });
        var icon = Ui.Icon(glyph, 18);
        icon.Style = Ui.Style("SpiceMutedIconStyle");
        icon.VerticalAlignment = VerticalAlignment.Center;
        Ui.Add(row, icon);
        var copy = new StackPanel { Spacing = 3, VerticalAlignment = VerticalAlignment.Center };
        copy.Children.Add(Ui.Text(label, 14));
        copy.Children.Add(Ui.Muted(description, 12));
        Ui.Add(row, copy, column: 1);
        var controlHost = new Grid { MaxWidth = 280, VerticalAlignment = VerticalAlignment.Center };
        controlHost.Children.Add(control);
        Ui.Add(row, controlHost, column: 2);
        row.SizeChanged += (_, args) =>
        {
            var narrow = args.NewSize.Width < 680;
            row.ColumnDefinitions[2].Width = narrow ? new GridLength(0) : new GridLength(250);
            Grid.SetColumnSpan(copy, narrow ? 2 : 1);
            Grid.SetRow(controlHost, narrow ? 1 : 0);
            Grid.SetColumn(controlHost, narrow ? 1 : 2);
            Grid.SetColumnSpan(controlHost, narrow ? 2 : 1);
            controlHost.HorizontalAlignment = narrow ? HorizontalAlignment.Left : HorizontalAlignment.Stretch;
            controlHost.Margin = new Thickness(0, narrow ? 10 : 0, 0, 0);
            controlHost.Width = narrow ? Math.Min(280, Math.Max(0, args.NewSize.Width - 42)) : double.NaN;
        };
        return row;
    }
    private static Expander Details(string glyph, string label, string description, FrameworkElement content)
    {
        var header = Ui.ColumnsWithSpacing(16, new GridLength(24), new GridLength(1, GridUnitType.Star));
        var icon = Ui.Icon(glyph, 18); icon.Style = Ui.Style("SpiceMutedIconStyle"); icon.VerticalAlignment = VerticalAlignment.Center;
        Ui.Add(header, icon);
        var copy = new StackPanel { Spacing = 3 };
        copy.Children.Add(Ui.Text(label, 14)); copy.Children.Add(Ui.Muted(description, 12));
        Ui.Add(header, copy, column: 1);
        return new Expander { Header = header, Content = content, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Stretch, Margin = new Thickness(0, 8, 0, 0) };
    }
    private Grid Choice(string glyph, string label, string description, string key, (string Label, string Value)[] options)
    {
        var combo = new ComboBox { MinHeight = 32, HorizontalAlignment = HorizontalAlignment.Stretch };
        AutomationProperties.SetName(combo, label);
        foreach (var item in options) combo.Items.Add(new ComboBoxItem { Content = item.Label, Tag = item.Value });
        combo.SelectedIndex = Math.Max(0, Array.FindIndex(options, o => o.Value == Wire.Text(_draft, key)));
        combo.SelectionChanged += (_, _) => { _draft[key] = (combo.SelectedItem as ComboBoxItem)?.Tag?.ToString(); Changed(); };
        return SettingRow(glyph, label, description, combo);
    }
    private Grid Folder(string title, string key, string description)
    {
        var host = new Grid();
        host.Children.Add(NativePageUi.FolderControl(Wire.Text(_draft, key), Choose, $"Change {title.ToLowerInvariant()} folder"));
        return SettingRow("\uE8B7", title, description, host);
        async Task Choose()
        {
            try
            {
                var path = await _context.PickFolderAsync(Wire.Text(_draft, key));
                if (string.IsNullOrEmpty(path)) return;
                _draft[key] = path; Changed();
                host.Children.Clear();
                host.Children.Add(NativePageUi.FolderControl(path, Choose, $"Change {title.ToLowerInvariant()} folder"));
            }
            catch (Exception error) { NativePageUi.Error(_feedback, error); }
        }
    }
    private Grid Toggle(string glyph, string label, string explanation, string key)
    {
        var toggle = new ToggleSwitch { IsOn = Wire.Bool(Selection, key), HorizontalAlignment = HorizontalAlignment.Right };
        AutomationProperties.SetName(toggle, label);
        toggle.Toggled += (_, _) => { Selection[key] = toggle.IsOn; Changed(true); };
        return SettingRow(glyph, label, explanation, toggle);
    }
    private async Task<bool> ConfirmDiscardAsync()
    {
        if (!_dirty) return true;
        var dialog = new ContentDialog { XamlRoot = XamlRoot, Title = "Discard unsaved settings?", Content = "Your saved settings will stay as they are.", PrimaryButtonText = "Discard changes", CloseButtonText = "Keep editing", DefaultButton = ContentDialogButton.Close };
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }
    private async Task SaveAsync()
    {
        IsEnabled = false;
        _save.Content = "Saving…";
        _save.IsEnabled = false;
        try
        {
            if (_policyChanged) Selection["revision"] = Guid.NewGuid().ToString();
            await _context.SaveConfigAsync((JsonObject)_draft.DeepClone());
            _dirty = false; _policyChanged = false; _discard.IsEnabled = false; _feedback.IsOpen = false; _saved.Text = "Settings saved.";
            _savedTimer.Stop(); _savedTimer.Start();
        }
        catch (Exception error) { _save.IsEnabled = true; NativePageUi.Error(_feedback, error); }
        finally { IsEnabled = true; _save.Content = "Save settings"; }
    }

    private async Task ResetCloudAsync()
    {
        if (_context.IsBusy) return;
        _context.IsBusy = true;
        IsEnabled = false;
        try
        {
            if (_dirty) throw new InvalidOperationException("Save your settings before resetting cloud history.");
            var config = (JsonObject)_context.Config.DeepClone();
            var preview = await _context.Engine.CallAsync("preview_cloud_cleanup", new JsonObject { ["config"] = config.DeepClone() }) as JsonObject
                ?? throw new InvalidOperationException("The cleanup review was not returned. Try again.");
            var phrase = Wire.Text(preview, "confirmationPhrase");
            var content = new StackPanel { Spacing = 12 };
            content.Children.Add(NativePageUi.Text($"This removes {Wire.Number(preview, "snapshotCount"):0} shared snapshots and {NativePageUi.Bytes(Wire.Number(preview, "storedBytes"))} of stored content from the selected cloud folder. Other devices will no longer be able to pull these snapshots."));
            content.Children.Add(NativePageUi.Text(Wire.Text(config, "cloudRoot"), 12, true));
            var confirmation = new TextBox { Header = $"Type {phrase} to confirm" };
            content.Children.Add(confirmation);
            var dialog = new ContentDialog { XamlRoot = XamlRoot, Title = "Reset shared cloud history?", Content = content,
                PrimaryButtonText = "Reset cloud history", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close, IsPrimaryButtonEnabled = false };
            confirmation.TextChanged += (_, _) => dialog.IsPrimaryButtonEnabled = phrase.Length > 0 && confirmation.Text == phrase;
            if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
            await _context.Engine.CallAsync("execute_cloud_cleanup", new JsonObject { ["config"] = config.DeepClone(), ["operationId"] = Wire.Text(preview, "operationId"), ["confirmation"] = confirmation.Text });
            _feedback.IsOpen = false; _saved.Text = "Cloud history reset."; _savedTimer.Stop(); _savedTimer.Start();
            try { await _context.RefreshAsync(); }
            catch (Exception error)
            {
                _feedback.Severity = InfoBarSeverity.Warning;
                _feedback.Title = "Cloud history reset";
                _feedback.Message = $"Cloud history was reset successfully, but workspace status could not refresh. Refresh Overview when ready. {error.Message}";
                _feedback.IsOpen = true;
            }
        }
        catch (Exception error) { NativePageUi.Error(_feedback, error); }
        finally { _context.IsBusy = false; IsEnabled = true; }
    }
}
