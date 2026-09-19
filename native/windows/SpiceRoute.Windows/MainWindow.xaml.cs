using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Text.Json.Nodes;
using Windows.Graphics;

namespace SpiceRoute.Windows;

public sealed partial class MainWindow : Window
{
    private readonly SpiceRouteContext context;
    private readonly DispatcherTimer noticeTimer = new() { Interval = TimeSpan.FromSeconds(6) };
    private bool navigating;
    private string currentPage = "overview";

    public MainWindow(bool visualProbe = false, string? engineDataDirectory = null, bool deferInitialization = false)
    {
        InitializeComponent();
        Title = "Spice Route";
        var appVersion = typeof(MainWindow).Assembly.GetName().Version;
        BuildText.Text = appVersion is null ? "Windows app" : $"Spice Route {appVersion.Major}.{appVersion.Minor}.{appVersion.Build}";
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleStrip);
        AppWindow.Resize(new SizeInt32(1180, 820));
        AppWindow.SetIcon(Path.Combine(System.AppContext.BaseDirectory, "Assets", "SpiceRoute.ico"));
        context = visualProbe ? new(this, new VisualProbeFixture()) : new(this, engineDataDirectory);
        context.NavigateAction = Navigate;
        context.MessageRequested += ShowMessage;
        context.StateChanged += ApplyState;
        context.BusyChanged += UpdateNavigationAvailability;
        noticeTimer.Tick += (_, _) => { Notice.IsOpen = false; noticeTimer.Stop(); };
        AppWindow.Closing += (_, args) =>
        {
            if (!context.IsBusy) return;
            args.Cancel = true;
            ShowMessage("An operation is running. Wait for it to finish, or cancel the handoff at a safe checkpoint, before closing Spice Route.", true);
        };
        Closed += async (_, _) => { noticeTimer.Stop(); await context.Engine.DisposeAsync(); };
        if (!visualProbe && !deferInitialization) Root.Loaded += async (_, _) => await InitializeAsync();
    }

    private async Task InitializeAsync(bool throwOnFailure = false)
    {
        StartupLog.Write("Workspace initialization started.");
        var loading = new StackPanel { Spacing = 14 };
        loading.Children.Add(Ui.Text("Loading your workspace…", 24, true));
        loading.Children.Add(new ProgressBar { IsIndeterminate = true });
        PageHost.Content = loading;
        try
        {
            StartupLog.Write("Checking for a stopped sync engine.");
            await context.Engine.RestartIfStoppedAsync();
            StartupLog.Write("Starting the sync-engine protocol handshake.");
            var protocol = await context.Engine.CallAsync("get_protocol_info");
            if (Wire.Number(protocol, "protocolVersion") != 1) throw new InvalidOperationException("This app and sync engine use different protocol versions. Reinstall the matching release.");
            StartupLog.Write("Loading Codex discovery, configuration, content inventory, and cloud status.");
            await context.RefreshAsync();
            StartupLog.Write("Workspace data loaded. Opening the initial page.");
            Navigate(Wire.Bool(context.Config, "onboardingComplete") ? "overview" : "setup");
            StartupLog.Write("Workspace initialization completed.");
        }
        catch (Exception error)
        {
            StartupLog.WriteException("Workspace initialization failed", error);
            loading.Children.Clear();
            loading.Children.Add(Ui.Text("The workspace could not open", 24, true));
            loading.Children.Add(Ui.Text(error.Message));
            var logPath = Ui.Muted($"Startup log: {StartupLog.LogPath}", 11);
            logPath.IsTextSelectionEnabled = true;
            loading.Children.Add(logPath);
            var retry = Ui.Button("Try again", "\uE72C");
            retry.Click += async (_, _) => await InitializeAsync();
            loading.Children.Add(retry);
            ConnectionText.Text = "Sync engine unavailable";
            if (throwOnFailure) throw;
        }
    }

    internal bool TryShowUnhandledFailure(Exception error)
    {
        try
        {
            noticeTimer.Stop();
            var failure = new StackPanel { Spacing = 14, MaxWidth = 760, HorizontalAlignment = HorizontalAlignment.Left };
            failure.Children.Add(Ui.Text("Spice Route ran into a problem", 24, true));
            failure.Children.Add(Ui.Text(error.Message));
            failure.Children.Add(Ui.Muted("Spice Route kept this window open so you can copy the diagnostic log path below.", 12));
            var logPath = Ui.Muted(StartupLog.LogPath, 11);
            logPath.IsTextSelectionEnabled = true;
            failure.Children.Add(logPath);
            if (!context.IsBusy)
            {
                var retry = Ui.Button("Retry workspace", "\uE72C");
                retry.Click += async (_, _) => await InitializeAsync();
                failure.Children.Add(retry);
            }
            PageHost.Content = failure;
            ConnectionText.Text = "App needs attention";
            return true;
        }
        catch (Exception surfaceError)
        {
            StartupLog.WriteException("Could not show the in-app failure surface", surfaceError);
            return false;
        }
    }

    private void ApplyState()
    {
        Root.RequestedTheme = Wire.Text(context.Config, "theme") switch { "light" => ElementTheme.Light, "dark" => ElementTheme.Dark, _ => ElementTheme.Default };
        ConnectionText.Text = Wire.Bool(context.Config, "onboardingComplete") ? $"{Wire.Text(context.Config, "deviceName")} · {ProviderName(Wire.Text(context.Config, "cloudProvider"))}" : "Connect your cloud folder to get started";
    }

    private static string ProviderName(string provider) => provider switch { "oneDrive" => "OneDrive", "googleDrive" => "Google Drive", "iCloud" => "iCloud Drive", _ => "Cloud folder" };

    internal async Task RunStartupProbeAsync(string dataDirectory)
    {
        var codexHome = Path.Combine(dataDirectory, "codex");
        var projectlessRoot = Path.Combine(dataDirectory, "projectless");
        Directory.CreateDirectory(codexHome);
        Directory.CreateDirectory(projectlessRoot);
        var config = new JsonObject
        {
            ["schemaVersion"] = 1,
            ["deviceId"] = "startup-probe",
            ["deviceName"] = "Startup probe",
            ["codexHome"] = codexHome,
            ["projectlessRoot"] = projectlessRoot,
            ["projectsRoot"] = "",
            ["cloudRoot"] = "",
            ["cloudProvider"] = "custom",
            ["theme"] = "system",
            ["onboardingComplete"] = false,
            ["sourceRoots"] = new JsonObject(),
            ["destinationRoots"] = new JsonObject(),
            ["selection"] = new JsonObject
            {
                ["revision"] = "startup-probe",
                ["defaultProjectMode"] = "full",
                ["projectModes"] = new JsonObject(),
                ["excludedThreadIds"] = new JsonArray(),
                ["includeArchived"] = true,
                ["includeBuildOutputs"] = false,
                ["includeSensitiveFiles"] = true,
                ["extraExcludePatterns"] = new JsonArray()
            }
        };
        await context.Engine.CallAsync("save_config", new JsonObject { ["config"] = config });
        await InitializeAsync(throwOnFailure: true);

        foreach (var page in new Page[]
        {
            new OverviewPage(context),
            new SelectionPage(context),
            new SettingsPage(context),
            new SetupPage(context),
            new DiagnosticsPage(context),
            new ReviewPage(context)
        })
        {
            PageHost.Content = page;
            await Task.Yield();
        }
        PageHost.Content = null;
    }

    internal ValueTask StopEngineForProbeAsync() => context.Engine.DisposeAsync();

    internal FrameworkElement VisualProbeRoot => Root;
    internal NavigationView VisualProbeNavigation => Navigation;
    internal SpiceRouteContext VisualProbeContext => context;
    internal void ShowVisualProbePage(string page, ElementTheme theme)
    {
        context.Config["theme"] = theme == ElementTheme.Dark ? "dark" : "light";
        ApplyState();
        Navigate(page);
    }

    private void Navigate(string key)
    {
        if (context.IsBusy)
        {
            navigating = true;
            Navigation.SelectedItem = currentPage == "settings" ? Navigation.SettingsItem : Navigation.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(item => item.Tag?.ToString() == NavigationKey(currentPage));
            navigating = false;
            return;
        }
        Notice.IsOpen = false;
        noticeTimer.Stop();
        currentPage = key;
        PageHost.Content = key switch
        {
            "selection" => new SelectionPage(context), "recovery" => new RecoveryPage(context),
            "settings" => new SettingsPage(context), "setup" => new SetupPage(context),
            "diagnostics" => new DiagnosticsPage(context),
            "review" => new ReviewPage(context), _ => new OverviewPage(context)
        };
        navigating = true;
        Navigation.SelectedItem = key == "settings" ? Navigation.SettingsItem : Navigation.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(item => item.Tag?.ToString() == NavigationKey(key));
        navigating = false;
    }

    private void OnNavigationChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (navigating || context is null) return;
        var key = args.IsSettingsSelected ? "settings" : (args.SelectedItem as NavigationViewItem)?.Tag?.ToString();
        if (key is null || key == currentPage) return;
        Navigate(key);
    }

    private static string NavigationKey(string page) => page switch { "review" => "overview", "diagnostics" => "recovery", _ => page };

    private void UpdateNavigationAvailability()
    {
        foreach (var item in Navigation.MenuItems.OfType<NavigationViewItem>())
            item.IsEnabled = !context.IsBusy;
        if (Navigation.SettingsItem is NavigationViewItem settings)
            settings.IsEnabled = !context.IsBusy;
    }

    private void ShowMessage(string message, bool error)
    {
        Notice.Severity = error ? InfoBarSeverity.Error : InfoBarSeverity.Success;
        Notice.Message = message;
        Notice.IsOpen = true;
        noticeTimer.Stop();
        if (!error) noticeTimer.Start();
    }
}
