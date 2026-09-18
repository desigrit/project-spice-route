using System;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class SettingsPage : Page
{
    private readonly SpiceRouteContext _context;
    private readonly JsonObject _draft;
    private readonly Button _save = new() { Content = "Save settings", IsEnabled = false };
    private readonly InfoBar _feedback = new() { IsOpen = false, IsClosable = true };
    private readonly TextBlock _saved = NativePageUi.Text("", 12, true);
    private readonly DispatcherTimer _savedTimer = new() { Interval = TimeSpan.FromSeconds(4) };
    private bool _policyChanged;
    private bool _dirty;

    public SettingsPage(SpiceRouteContext context)
    {
        _context = context;
        _draft = (JsonObject)context.Config.DeepClone();
        _save.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        var page = NativePageUi.PageGrid("Settings", _save, out var content);
        var form = new StackPanel { Spacing = 24, MaxWidth = 820, HorizontalAlignment = HorizontalAlignment.Stretch };
        form.Children.Add(_feedback);
        form.Children.Add(_saved);
        var device = Section(form, "This device");
        var name = new TextBox { Header = "Device name", Text = Wire.Text(_draft, "deviceName"), MaxWidth = 420, HorizontalAlignment = HorizontalAlignment.Left };
        name.TextChanged += (_, _) => { _draft["deviceName"] = name.Text; Changed(); };
        device.Children.Add(name);
        device.Children.Add(Choice("Appearance", "theme", new[] { ("Use Windows setting", "system"), ("Light", "light"), ("Dark", "dark") }));
        var cloud = Section(form, "Cloud drive");
        cloud.Children.Add(NativePageUi.Text("Your drive's desktop app handles sign-in and cloud delivery.", secondary: true));
        cloud.Children.Add(Choice("Provider", "cloudProvider", new[] { ("OneDrive", "oneDrive"), ("Google Drive", "googleDrive"), ("iCloud Drive", "iCloud"), ("Another folder", "custom") }));
        cloud.Children.Add(Folder("Sync folder", "cloudRoot", "Only Spice Route snapshots go here. Keep live Codex data and project folders outside it."));
        var folders = Section(form, "Local folders");
        folders.Children.Add(Folder("Codex tasks and history", "codexHome", "The Codex data folder, usually .codex. Contains session metadata, history databases, and transcripts."));
        folders.Children.Add(Folder("Projectless chat workspaces", "projectlessRoot", "Files and artifacts created in chats that do not belong to a project. This is separate from the history folder."));
        folders.Children.Add(Folder("Default location for new restores", "projectsRoot", "A starting location only. Choose each project's actual folder in What to sync or during Pull."));
        var projectLink = new HyperlinkButton { Content = "Choose individual project folders", HorizontalAlignment = HorizontalAlignment.Left, Padding = new Thickness(0) };
        projectLink.Click += (_, _) => context.Navigate("selection"); folders.Children.Add(projectLink);
        var policy = Section(form, "What files travel");
        policy.Children.Add(Toggle("Include archived chats", "includeArchived", "Archived conversations follow your individual chat selections."));
        policy.Children.Add(Toggle("Include project secrets and configuration", "includeSensitiveFiles", "Includes project .env files, credentials, keys, and certificates. They are readable in your chosen cloud folder. Codex account credentials remain local."));
        policy.Children.Add(Toggle("Include dependencies and build outputs", "includeBuildOutputs", "Usually unnecessary on another PC. This can add large caches, packages, and compiled files to the handoff."));
        var patterns = new TextBox { Header = "Additional file exclusions", AcceptsReturn = true, TextWrapping = TextWrapping.Wrap, MinHeight = 82,
            Text = string.Join(Environment.NewLine, Wire.Array(Selection, "extraExcludePatterns").Select(n => n?.GetValue<string>()).Where(v => v is not null)),
            PlaceholderText = "One pattern per line, for example **/local-backups/**" };
        patterns.TextChanged += (_, _) =>
        {
            Selection["extraExcludePatterns"] = new JsonArray(patterns.Text.Split(new[] { '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries).Select(p => (JsonNode?)JsonValue.Create(p.Trim())).ToArray());
            Changed(true);
        };
        policy.Children.Add(patterns);
        var compatibility = Section(form, "Codex compatibility");
        compatibility.Children.Add(NativePageUi.Text(Wire.Text(context.Environment, "codexVersion", "Runtime not detected")));
        compatibility.Children.Add(NativePageUi.Text(Wire.Text(Wire.Object(context.Environment, "compatibility"), "explanation", "Refresh Overview to check the configured Codex installation."), secondary: true));
        var cleanup = new StackPanel { Spacing = 12 };
        cleanup.Children.Add(NativePageUi.Text("Snapshots stay in your cloud folder until you explicitly remove them. Resetting cloud history keeps local Codex data and your sync choices.", secondary: true));
        var reset = new Button { Content = "Reset cloud history…", HorizontalAlignment = HorizontalAlignment.Left };
        reset.Click += async (_, _) => await ResetCloudAsync();
        cleanup.Children.Add(reset);
        form.Children.Add(new Expander { Header = "Cloud history", Content = cleanup, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Stretch });
        content.Children.Add(new ScrollViewer { Content = form, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled });
        Content = page;
        _save.Click += async (_, _) => await SaveAsync();
        _savedTimer.Tick += (_, _) => { _saved.Text = ""; _savedTimer.Stop(); };
        Unloaded += (_, _) => _savedTimer.Stop();
    }

    private JsonObject Selection => NativePageUi.EnsureObject(_draft, "selection");
    private void Changed(bool policy = false) { _dirty = true; _save.IsEnabled = true; _policyChanged |= policy; _saved.Text = "Unsaved changes"; }
    internal static StackPanel Section(StackPanel form, string title)
    {
        var group = new StackPanel { Spacing = 12 };
        group.Children.Add(NativePageUi.Text(title, 18)); form.Children.Add(group); return group;
    }
    private StackPanel Choice(string label, string key, (string Label, string Value)[] options)
    {
        var block = new StackPanel { Spacing = 6 };
        var combo = new ComboBox { Header = label, Width = 230 };
        foreach (var item in options) combo.Items.Add(new ComboBoxItem { Content = item.Label, Tag = item.Value });
        combo.SelectedIndex = Math.Max(0, Array.FindIndex(options, o => o.Value == Wire.Text(_draft, key)));
        combo.SelectionChanged += (_, _) => { _draft[key] = (combo.SelectedItem as ComboBoxItem)?.Tag?.ToString(); Changed(); };
        block.Children.Add(combo); return block;
    }
    private StackPanel Folder(string title, string key, string description)
    {
        var block = new StackPanel { Spacing = 4 };
        block.Children.Add(NativePageUi.Text(title));
        var control = NativePageUi.FolderControl(Wire.Text(_draft, key), Choose, $"Change {title.ToLowerInvariant()} folder");
        block.Children.Add(control); block.Children.Add(NativePageUi.Text(description, 12, true));
        return block;
        async Task Choose()
        {
            try
            {
                var path = await _context.PickFolderAsync(Wire.Text(_draft, key));
                if (string.IsNullOrEmpty(path)) return;
                _draft[key] = path; Changed();
                block.Children[1] = NativePageUi.FolderControl(path, Choose, $"Change {title.ToLowerInvariant()} folder");
            }
            catch (Exception error) { NativePageUi.Error(_feedback, error); }
        }
    }
    private StackPanel Toggle(string label, string key, string explanation)
    {
        var group = new StackPanel { Spacing = 4 };
        var toggle = new ToggleSwitch { Header = label, IsOn = Wire.Bool(Selection, key) };
        toggle.Toggled += (_, _) => { Selection[key] = toggle.IsOn; Changed(true); };
        group.Children.Add(toggle); group.Children.Add(NativePageUi.Text(explanation, 12, true)); return group;
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
            _dirty = false; _policyChanged = false; _feedback.IsOpen = false; _saved.Text = "Settings saved.";
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
