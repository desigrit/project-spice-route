using Microsoft.UI.Xaml;

namespace SpiceRoute.Windows;

public partial class App : Application
{
    private MainWindow? window;
    public App() => InitializeComponent();
    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        window = new MainWindow();
        if (Environment.GetCommandLineArgs().Any(argument => string.Equals(argument, "--startup-probe", StringComparison.Ordinal)))
        {
            window.RunStartupProbe();
            window.Close();
            Exit();
            return;
        }
        window.Activate();
    }
}
