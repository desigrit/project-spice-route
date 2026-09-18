using Microsoft.UI.Xaml;
using System.Text.Json.Nodes;
using Windows.Storage.Pickers;
using WinRT.Interop;

namespace SpiceRoute.Windows;

public sealed class SpiceRouteContext
{
    private int generation;
    public Window Window { get; }
    public EngineClient Engine { get; }
    public JsonObject Config { get; private set; } = new();
    public JsonObject Environment { get; private set; } = new();
    public JsonObject Catalog { get; private set; } = new();
    public JsonObject Status { get; private set; } = new();
    public JsonObject? CurrentPreview { get; set; }
    public bool IsBusy { get; set; }
    public string ReviewDirection { get; set; } = "push";
    public string? ReviewSnapshotId { get; set; }
    public event Action? StateChanged;
    public event Action<string, bool>? MessageRequested;
    public Action<string>? NavigateAction { get; set; }
    public SpiceRouteContext(Window window)
    {
        Window = window;
        Engine = new();
    }
    internal SpiceRouteContext(Window window, VisualProbeFixture fixture)
    {
        Window = window;
        Engine = new(fixture.Respond);
        Config = Wire.Clone(fixture.Config);
        Environment = Wire.Clone(fixture.Environment);
        Catalog = Wire.Clone(fixture.Catalog);
        Status = Wire.Clone(fixture.Status);
    }
    public void Navigate(string key) => NavigateAction?.Invoke(key);
    public void ShowMessage(string message, bool error = false) => MessageRequested?.Invoke(message, error);
    public void NotifyChanged() => StateChanged?.Invoke();

    public async Task RefreshAsync()
    {
        var current = ++generation;
        var environmentTask = Engine.CallAsync("discover_environment");
        var configTask = Engine.CallAsync("load_config");
        await Task.WhenAll(environmentTask, configTask);
        if (current != generation) return;
        Environment = await environmentTask as JsonObject ?? new JsonObject();
        Config = await configTask as JsonObject ?? new JsonObject();
        if (Wire.Bool(Config, "onboardingComplete"))
        {
            var catalogTask = Engine.CallAsync("list_content_quick", new() { ["config"] = Config.DeepClone() });
            var statusTask = Engine.CallAsync("get_sync_status", new() { ["config"] = Config.DeepClone() });
            await Task.WhenAll(catalogTask, statusTask);
            if (current != generation) return;
            Catalog = await catalogTask as JsonObject ?? new JsonObject();
            Status = await statusTask as JsonObject ?? new JsonObject();
        }
        StateChanged?.Invoke();
    }

    public async Task SaveConfigAsync(JsonObject config)
    {
        var previousBusy = IsBusy;
        IsBusy = true;
        try
        {
            Config = await Engine.CallAsync("save_config", new() { ["config"] = config.DeepClone() }) as JsonObject ?? throw new InvalidDataException("The saved configuration was not returned.");
            Catalog = new(); Status = new();
            StateChanged?.Invoke();
            try { await RefreshAsync(); }
            catch (Exception error) { ShowMessage("Settings were saved, but the workspace could not refresh. " + error.Message, true); }
        }
        finally { IsBusy = previousBusy; }
    }

    public async Task<string?> PickFolderAsync(string? initialPath = null)
    {
        var picker = new FolderPicker { SuggestedStartLocation = PickerLocationId.ComputerFolder };
        picker.FileTypeFilter.Add("*");
        InitializeWithWindow.Initialize(picker, WindowNative.GetWindowHandle(Window));
        var folder = await picker.PickSingleFolderAsync();
        return folder?.Path;
    }
}
