using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Automation.Peers;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class AppUpdatesRow : Grid
{
    private readonly SpiceRouteContext _context;
    private readonly Func<bool> _hasUnsavedSettings;
    private readonly Action<bool> _setSettingsBusy;
    private readonly AppUpdates _service = new();
    private readonly Button _check = Ui.Button("Check for updates", "\uE72C");
    private readonly Button _install = Ui.Button("Install and restart", "\uE896", primary: true);
    private readonly Button _cancel = Ui.TextButton("Cancel");
    private readonly TextBlock _status = Ui.Muted("", 12);
    private readonly ProgressBar _progress = new() { Minimum = 0, Maximum = 100, Height = 3, Visibility = Visibility.Collapsed };
    private AppUpdate? _available;
    private CancellationTokenSource? _operation;
    private bool _working;

    public AppUpdatesRow(SpiceRouteContext context, Func<bool> hasUnsavedSettings, Action<bool> setSettingsBusy)
    {
        _context = context;
        _hasUnsavedSettings = hasUnsavedSettings;
        _setSettingsBusy = setSettingsBusy;
        Style = Ui.Style("SpiceLineTopGridStyle");
        BorderThickness = new Thickness(0, 1, 0, 0);
        Padding = new Thickness(2, 24, 0, 8);
        ColumnSpacing = 24;
        ColumnDefinitions.Add(new() { Width = new(1, GridUnitType.Star) });
        ColumnDefinitions.Add(new() { Width = GridLength.Auto });
        var copy = new StackPanel { Spacing = 6, VerticalAlignment = VerticalAlignment.Top };
        copy.Children.Add(Ui.SectionTitle("Updates"));
        copy.Children.Add(Ui.Muted($"Spice Route {AppUpdates.CurrentVersion}", 12));
        Children.Add(copy);
        var actions = new StackPanel { Spacing = 8, MaxWidth = 360, HorizontalAlignment = HorizontalAlignment.Left };
        var buttons = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        buttons.Children.Add(_check); buttons.Children.Add(_install); buttons.Children.Add(_cancel);
        actions.Children.Add(buttons); actions.Children.Add(_progress); actions.Children.Add(_status);
        Grid.SetColumn(actions, 1); Children.Add(actions);
        _install.Visibility = _cancel.Visibility = Visibility.Collapsed;
        AutomationProperties.SetLiveSetting(_status, AutomationLiveSetting.Polite);
        AutomationProperties.SetName(_progress, "Update download progress");
        _check.Click += async (_, _) => await CheckAsync();
        _install.Click += async (_, _) => await InstallAsync();
        _cancel.Click += (_, _) => _operation?.Cancel();
        Unloaded += (_, _) => { _operation?.Cancel(); _service.Dispose(); };
    }

    public void SettingsChanged()
    {
        _install.IsEnabled = !_working && !_hasUnsavedSettings();
        if (_available is not null && !_working)
            _status.Text = _hasUnsavedSettings() ? "Save your settings before installing." : $"Version {_available.Version} is available.";
    }

    private void Working(bool value)
    {
        _working = value;
        _context.IsBusy = value;
        _setSettingsBusy(value);
        _check.IsEnabled = !value;
        _install.IsEnabled = !value && !_hasUnsavedSettings();
    }

    private async Task CheckAsync()
    {
        if (_context.IsBusy) return;
        Working(true);
        _status.Text = "Checking for updates…";
        _available = null;
        _install.Visibility = Visibility.Collapsed;
        using var operation = new CancellationTokenSource();
        _operation = operation;
        try
        {
            _available = await _service.CheckAsync(operation.Token);
            _status.Text = _available is null ? "Up to date" : $"Version {_available.Version} is available.";
            _install.Visibility = _available is null ? Visibility.Collapsed : Visibility.Visible;
            _check.Visibility = _available is null ? Visibility.Visible : Visibility.Collapsed;
        }
        catch (Exception error) { _status.Text = FriendlyError(error, false); }
        finally { _operation = null; Working(false); SettingsChanged(); }
    }

    private async Task InstallAsync()
    {
        if (_context.IsBusy || _hasUnsavedSettings() || _available is null) return;
        Working(true);
        using var operation = new CancellationTokenSource();
        _operation = operation;
        _progress.Value = 0;
        _progress.Visibility = _cancel.Visibility = Visibility.Visible;
        _status.Text = "Downloading update…";
        try
        {
            var progress = new Progress<int>(percent =>
            {
                _progress.Value = percent;
                _status.Text = percent < 100 ? $"Downloading update… {percent}%" : "Verifying update…";
            });
            using var downloaded = await _service.DownloadAsync(_available, async (path, signature) =>
                await _context.Engine.CallAsync("verify_update_artifact", new() { ["path"] = path, ["signature"] = signature }), progress, operation.Token);
            operation.Token.ThrowIfCancellationRequested();
            _cancel.Visibility = Visibility.Collapsed;
            _status.Text = "Installing update. Spice Route will restart…";
            downloaded.StartInstaller();
            // The installer waits for this process to exit. Finish engine shutdown
            // before closing the window, so its files and profile lock are released.
            await _context.Engine.DisposeAsync();
            _context.UpdateReadyToRestart = true;
            _context.Window.Close();
        }
        catch (Exception error) { _status.Text = FriendlyError(error, operation.IsCancellationRequested); }
        finally
        {
            _operation = null;
            _cancel.Visibility = _progress.Visibility = Visibility.Collapsed;
            Working(false);
        }
    }

    private static string FriendlyError(Exception error, bool cancelled) => error switch
    {
        OperationCanceledException => cancelled ? "Download cancelled. You can try again." : "The update request timed out. Try again.",
        System.Net.Http.HttpRequestException => "Could not reach GitHub. Check your connection and try again.",
        IOException => "Could not download or verify the update. Check free space and try again.",
        _ => error.Message
    };
}
