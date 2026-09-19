using System;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace SpiceRoute.Windows;

public sealed class RecoveryPage : Page
{
    private readonly SpiceRouteContext _context;
    private readonly ListView _points = new() { SelectionMode = ListViewSelectionMode.None, HorizontalContentAlignment = HorizontalAlignment.Stretch };
    private readonly Button _refresh = new() { Content = "Refresh" };
    private readonly InfoBar _feedback = new() { IsOpen = false, IsClosable = true };
    private readonly TextBlock _status = NativePageUi.Text("Loading recovery points…", secondary: true);
    private readonly ProgressBar _progress = new() { IsIndeterminate = true, Visibility = Visibility.Collapsed };
    private bool _busy;

    public RecoveryPage(SpiceRouteContext context)
    {
        _context = context;
        var page = NativePageUi.PageGrid("Recovery", _refresh, out var content);
        content.RowDefinitions.Add(new() { Height = GridLength.Auto });
        content.RowDefinitions.Add(new() { Height = new GridLength(1, GridUnitType.Star) });
        var intro = new StackPanel { Spacing = 12, Margin = new Thickness(0, 0, 0, 20) };
        intro.Children.Add(NativePageUi.Text("Before Pull changes local files, Spice Route saves a recovery point. The latest ten completed points and any unfinished recovery data stay on this PC.", secondary: true));
        var diagnostics = Ui.Button("Diagnose missing chats", "\uE946");
        diagnostics.Click += (_, _) => context.Navigate("diagnostics");
        intro.Children.Add(diagnostics);
        intro.Children.Add(_feedback); intro.Children.Add(_status); intro.Children.Add(_progress);
        content.Children.Add(intro); Grid.SetRow(_points, 1); content.Children.Add(_points);
        Content = page;
        Loaded += async (_, _) => await LoadAsync();
        _refresh.Click += async (_, _) => await LoadAsync();
    }

    private async Task<bool> LoadAsync()
    {
        if (_busy) return false;
        _refresh.IsEnabled = false; _progress.Visibility = Visibility.Visible;
        try
        {
            var records = await _context.Engine.CallAsync("list_recoveries") as JsonArray ?? new JsonArray();
            _points.Items.Clear();
            foreach (var point in records.OfType<JsonObject>()) _points.Items.Add(PointRow(point));
            _status.Text = records.Count == 0 ? "Your first Pull will create a recovery point here." : "Restoring a point replaces only the local data it protected.";
            _feedback.IsOpen = false;
            return true;
        }
        catch (Exception error) { _status.Text = "Recovery history could not be loaded."; NativePageUi.Error(_feedback, error); return false; }
        finally { _refresh.IsEnabled = true; _progress.Visibility = Visibility.Collapsed; }
    }

    private Grid PointRow(JsonObject point)
    {
        var row = NativePageUi.RowGrid(-1, 112);
        row.Margin = new Thickness(0, 10, 0, 10);
        var details = new StackPanel { Spacing = 5 };
        details.Children.Add(NativePageUi.Text(Wire.Text(point, "reason", "Recovery point")));
        var status = Wire.Text(point, "status");
        var state = status == "pending" ? "Needs recovery" : status == "restored" ? "Previously restored" : "Available";
        details.Children.Add(NativePageUi.Text($"{NativePageUi.Time(Wire.Text(point, "createdAt"))} · {NativePageUi.Bytes(Wire.Number(point, "sizeBytes"))} · {state}", 12, true));
        if (status == "pending") details.Children.Add(NativePageUi.Text("Finish recovery before starting another handoff.", 12));
        row.Children.Add(details);
        var restore = new Button { Content = "Restore", VerticalAlignment = VerticalAlignment.Center, HorizontalAlignment = HorizontalAlignment.Right };
        restore.Click += async (_, _) => await RestoreAsync(point);
        Grid.SetColumn(restore, 1); row.Children.Add(restore); return row;
    }

    private async Task RestoreAsync(JsonObject point)
    {
        if (_busy) return;
        var dialog = new ContentDialog
        {
            XamlRoot = XamlRoot,
            Title = "Restore this recovery point?",
            Content = $"This returns the affected local data to {NativePageUi.Time(Wire.Text(point, "createdAt"))}. Changes to those files since then will be replaced. Save your work first. Codex will be asked to close.",
            PrimaryButtonText = "Close Codex and restore", CloseButtonText = "Cancel", DefaultButton = ContentDialogButton.Close,
        };
        if (await dialog.ShowAsync() != ContentDialogResult.Primary) return;
        _busy = true; _context.IsBusy = true; _points.IsEnabled = false; _refresh.IsEnabled = false; _progress.Visibility = Visibility.Visible;
        var restored = false;
        string? refreshError = null;
        try
        {
            _status.Text = "Waiting for Codex to close…";
            var closed = await _context.Engine.CallAsync("request_codex_close");
            if (closed?.GetValue<bool>() != true) throw new InvalidOperationException("Codex is still running. Save your work, fully quit Codex, and try again.");
            _status.Text = "Restoring the protected local files…";
            await _context.Engine.CallAsync("restore_recovery", new JsonObject { ["recoveryId"] = Wire.Text(point, "id") });
            restored = true;
            _status.Text = "Recovery restored. Your local data is ready.";
            _feedback.IsOpen = false;
            try { await _context.RefreshAsync(); }
            catch (Exception error) { refreshError = error.Message; }
        }
        catch (Exception error) { NativePageUi.Error(_feedback, error); }
        finally { _busy = false; _context.IsBusy = false; _points.IsEnabled = true; _refresh.IsEnabled = true; _progress.Visibility = Visibility.Collapsed; }
        if (restored)
        {
            var historyLoaded = await LoadAsync();
            _status.Text = historyLoaded ? "Recovery restored. Your local data is ready." : "Recovery restored. Recovery history could not refresh.";
            if (refreshError is not null)
            {
                _feedback.Severity = InfoBarSeverity.Warning;
                _feedback.Title = "Recovery restored";
                _feedback.Message = $"Your local data was restored successfully, but workspace status could not refresh. Refresh Overview when ready. {refreshError}";
                _feedback.IsOpen = true;
            }
        }
    }
}
