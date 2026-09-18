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

    public MainWindow()
    {
        InitializeComponent();
        Title = "Spice Route";
        var appVersion = typeof(MainWindow).Assembly.GetName().Version;
        VersionText.Text = appVersion is null ? string.Empty : $"{appVersion.Major}.{appVersion.Minor}";
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(TitleStrip);
        AppWindow.Resize(new SizeInt32(1180, 820));
        AppWindow.SetIcon(Path.Combine(System.AppContext.BaseDirectory, "Assets", "SpiceRoute.ico"));
        context = new(this) { NavigateAction = Navigate };
        context.MessageRequested += ShowMessage;
        context.StateChanged += ApplyState;
        noticeTimer.Tick += (_, _) => { Notice.IsOpen = false; noticeTimer.Stop(); };
        AppWindow.Closing += (_, args) =>
        {
            if (!context.IsBusy) return;
            args.Cancel = true;
            ShowMessage("An operation is running. Wait for it to finish, or cancel the handoff at a safe checkpoint, before closing Spice Route.", true);
        };
        Closed += async (_, _) => { noticeTimer.Stop(); await context.Engine.DisposeAsync(); };
        Root.Loaded += async (_, _) => await InitializeAsync();
    }

    private async Task InitializeAsync()
    {
        var loading = new StackPanel { Spacing = 14 };
        loading.Children.Add(Ui.Text("Loading your workspace…", 24, true));
        loading.Children.Add(new ProgressBar { IsIndeterminate = true });
        PageHost.Content = loading;
        try
        {
            await context.Engine.RestartIfStoppedAsync();
            var protocol = await context.Engine.CallAsync("get_protocol_info");
            if (Wire.Number(protocol, "protocolVersion") != 1) throw new InvalidOperationException("This app and sync engine use different protocol versions. Reinstall the matching release.");
            await context.RefreshAsync();
            Navigate(Wire.Bool(context.Config, "onboardingComplete") ? "overview" : "setup");
        }
        catch (Exception error)
        {
            loading.Children.Clear();
            loading.Children.Add(Ui.Text("The workspace could not open", 24, true));
            loading.Children.Add(Ui.Text(error.Message));
            var retry = Ui.Button("Try again", "\uE72C");
            retry.Click += async (_, _) => await InitializeAsync();
            loading.Children.Add(retry);
            ConnectionText.Text = "Sync engine unavailable";
        }
    }

    private void ApplyState()
    {
        Root.RequestedTheme = Wire.Text(context.Config, "theme") switch { "light" => ElementTheme.Light, "dark" => ElementTheme.Dark, _ => ElementTheme.Default };
        ConnectionText.Text = Wire.Bool(context.Config, "onboardingComplete") ? $"{Wire.Text(context.Config, "deviceName")} · {ProviderName(Wire.Text(context.Config, "cloudProvider"))}" : "Connect your cloud folder to get started";
    }

    private static string ProviderName(string provider) => provider switch { "oneDrive" => "OneDrive", "googleDrive" => "Google Drive", "iCloud" => "iCloud Drive", _ => "Cloud folder" };

    private void Navigate(string key)
    {
        if (context.IsBusy)
        {
            navigating = true;
            Navigation.SelectedItem = currentPage == "settings" ? Navigation.SettingsItem : Navigation.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(item => item.Tag?.ToString() == (currentPage == "review" ? "overview" : currentPage));
            navigating = false;
            ShowMessage("Wait for the current operation to finish before changing pages. A running handoff can be cancelled from its review.", true);
            return;
        }
        Notice.IsOpen = false;
        noticeTimer.Stop();
        currentPage = key;
        PageHost.Content = key switch
        {
            "selection" => new SelectionPage(context), "recovery" => new RecoveryPage(context),
            "settings" => new SettingsPage(context), "setup" => new SetupPage(context),
            "review" => new ReviewPage(context), _ => new OverviewPage(context)
        };
        navigating = true;
        Navigation.SelectedItem = key == "settings" ? Navigation.SettingsItem : Navigation.MenuItems.OfType<NavigationViewItem>().FirstOrDefault(item => item.Tag?.ToString() == (key == "review" ? "overview" : key));
        navigating = false;
    }

    private void OnNavigationChanged(NavigationView sender, NavigationViewSelectionChangedEventArgs args)
    {
        if (navigating || context is null) return;
        var key = args.IsSettingsSelected ? "settings" : (args.SelectedItem as NavigationViewItem)?.Tag?.ToString();
        if (key is null || key == currentPage) return;
        Navigate(key);
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
