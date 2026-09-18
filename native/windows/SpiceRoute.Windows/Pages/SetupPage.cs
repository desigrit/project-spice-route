using System;
using System.IO;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class SetupPage : Page
{
    private readonly SpiceRouteContext _context;
    private readonly JsonObject _draft;
    private readonly Button _continue = new() { Content = "Save and choose content" };
    private readonly InfoBar _feedback = new() { IsOpen = false, IsClosable = true };
    private readonly StackPanel _cloudPath = new() { Spacing = 5 };

    public SetupPage(SpiceRouteContext context)
    {
        _context = context;
        // Start from the complete engine config. Device identity, exclusions,
        // path mappings, and existing policy survive setup and app upgrades.
        _draft = (JsonObject)context.Config.DeepClone();
        _continue.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        if (string.IsNullOrWhiteSpace(Wire.Text(_draft, "codexHome")) && !string.IsNullOrEmpty(Wire.Text(context.Environment, "codexHome")))
            _draft["codexHome"] = Wire.Text(context.Environment, "codexHome");
        var page = NativePageUi.PageGrid("Welcome to Spice Route", null, out var content);
        var form = new StackPanel { Spacing = 24, MaxWidth = 740, HorizontalAlignment = HorizontalAlignment.Left };
        form.Children.Add(NativePageUi.Text("Bring your Codex chats and project work with you. Set up this PC, then choose what travels.", 16, true));
        form.Children.Add(_feedback);
        var device = SettingsPage.Section(form, "Name this PC");
        var name = new TextBox { Header = "Device name", Text = Wire.Text(_draft, "deviceName"), Width = 380 };
        name.TextChanged += (_, _) => _draft["deviceName"] = name.Text;
        device.Children.Add(name);
        device.Children.Add(NativePageUi.Text("This name appears beside snapshots so you know where each handoff came from.", 12, true));
        var cloud = SettingsPage.Section(form, "Choose your cloud folder");
        cloud.Children.Add(NativePageUi.Text("Sign in through OneDrive, Google Drive, or iCloud Drive on this PC. Spice Route uses the folder their desktop app syncs.", secondary: true));
        var providers = new ComboBox { Header = "Provider", Width = 260 };
        var options = new[] { ("OneDrive", "oneDrive"), ("Google Drive", "googleDrive"), ("iCloud Drive", "iCloud"), ("Another folder", "custom") };
        foreach (var option in options) providers.Items.Add(new ComboBoxItem { Content = option.Item1, Tag = option.Item2 });
        providers.SelectedIndex = Math.Max(0, Array.FindIndex(options, p => p.Item2 == Wire.Text(_draft, "cloudProvider")));
        providers.SelectionChanged += (_, _) => _draft["cloudProvider"] = (providers.SelectedItem as ComboBoxItem)?.Tag?.ToString();
        cloud.Children.Add(providers);
        var candidates = Wire.Array(context.Environment, "cloudCandidates").OfType<JsonObject>().ToList();
        if (candidates.Count > 0)
        {
            var detected = new ComboBox { Header = "Detected on this PC", PlaceholderText = "Choose a detected drive", MinWidth = 320 };
            foreach (var candidate in candidates) detected.Items.Add(new ComboBoxItem { Content = $"{Wire.Text(candidate, "label")} ({NativePageUi.FolderName(Wire.Text(candidate, "path"))})", Tag = candidate });
            detected.SelectionChanged += (_, _) =>
            {
                if ((detected.SelectedItem as ComboBoxItem)?.Tag is not JsonObject candidate) return;
                _draft["cloudRoot"] = Path.Combine(Wire.Text(candidate, "path"), "Codex Sync - Spice Route");
                _draft["cloudProvider"] = Wire.Text(candidate, "provider", "custom");
                providers.SelectedIndex = Math.Max(0, Array.FindIndex(options, p => p.Item2 == Wire.Text(_draft, "cloudProvider")));
                RenderCloudPath();
            };
            cloud.Children.Add(detected);
        }
        cloud.Children.Add(_cloudPath);
        RenderCloudPath();
        var local = SettingsPage.Section(form, "Check the local Codex folders");
        local.Children.Add(Folder("Codex tasks and history", "codexHome", "The .codex folder with the history databases and session transcripts."));
        local.Children.Add(Folder("Projectless chat workspaces", "projectlessRoot", "Working files and artifacts for chats outside projects. Keep this separate from .codex."));
        local.Children.Add(NativePageUi.Text("Projects can live anywhere on this PC. Choose each project's folder on the next page; there is no single shared project root.", 12, true));
        var footer = new StackPanel { Spacing = 10 };
        footer.Children.Add(NativePageUi.Text("Setup saves your preferences. It does not push, pull, or close Codex.", 12, true));
        _continue.HorizontalAlignment = HorizontalAlignment.Left;
        footer.Children.Add(_continue); form.Children.Add(footer);
        content.Children.Add(new ScrollViewer { Content = form, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled });
        Content = page;
        _continue.Click += async (_, _) => await SaveAsync();
    }

    private void RenderCloudPath()
    {
        _cloudPath.Children.Clear();
        _cloudPath.Children.Add(NativePageUi.Text("Snapshot folder"));
        _cloudPath.Children.Add(NativePageUi.FolderControl(Wire.Text(_draft, "cloudRoot"), async () =>
        {
            try
            {
                var path = await _context.PickFolderAsync(Wire.Text(_draft, "cloudRoot"));
                if (string.IsNullOrEmpty(path)) return;
                _draft["cloudRoot"] = path; RenderCloudPath();
            }
            catch (Exception error) { NativePageUi.Error(_feedback, error); }
        }, "Choose cloud snapshot folder"));
        _cloudPath.Children.Add(NativePageUi.Text("Use the same cloud snapshot folder on every device. Spice Route keeps active Codex data outside this folder.", 12, true));
    }

    private StackPanel Folder(string title, string key, string description)
    {
        var group = new StackPanel { Spacing = 5 };
        group.Children.Add(NativePageUi.Text(title));
        group.Children.Add(NativePageUi.FolderControl(Wire.Text(_draft, key), Choose, $"Change {title.ToLowerInvariant()} folder"));
        group.Children.Add(NativePageUi.Text(description, 12, true));
        return group;
        async Task Choose()
        {
            try
            {
                var path = await _context.PickFolderAsync(Wire.Text(_draft, key));
                if (string.IsNullOrEmpty(path)) return;
                _draft[key] = path;
                group.Children[1] = NativePageUi.FolderControl(path, Choose, $"Change {title.ToLowerInvariant()} folder");
            }
            catch (Exception error) { NativePageUi.Error(_feedback, error); }
        }
    }

    private async Task SaveAsync()
    {
        IsEnabled = false;
        _continue.Content = "Saving setup…";
        _continue.IsEnabled = false;
        try
        {
            _draft["onboardingComplete"] = true;
            await _context.SaveConfigAsync((JsonObject)_draft.DeepClone());
            _context.Navigate("selection");
        }
        catch (Exception error) { NativePageUi.Error(_feedback, error); _continue.IsEnabled = true; }
        finally { IsEnabled = true; _continue.Content = "Save and choose content"; }
    }
}
