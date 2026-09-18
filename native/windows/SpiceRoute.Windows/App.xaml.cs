using Microsoft.UI.Xaml;

namespace SpiceRoute.Windows;

public partial class App : Application
{
    private MainWindow? window;
    public App() => InitializeComponent();
    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        var arguments = Environment.GetCommandLineArgs();
        var visualProbeIndex = Array.IndexOf(arguments, "--visual-probe");
        if (visualProbeIndex >= 0)
        {
            if (visualProbeIndex + 1 >= arguments.Length) { Environment.ExitCode = 2; Exit(); return; }
            var outputDirectory = Path.GetFullPath(arguments[visualProbeIndex + 1]);
            Directory.CreateDirectory(outputDirectory);
            try
            {
                window = new MainWindow(visualProbe: true);
                Environment.ExitCode = await VisualProbe.RunAsync(window, outputDirectory) ? 0 : 1;
            }
            catch (Exception error)
            {
                Environment.ExitCode = 1;
                await File.WriteAllTextAsync(Path.Combine(outputDirectory, "visual-probe-error.txt"), error.ToString());
            }
            finally { window?.Close(); Exit(); }
            return;
        }
        window = new MainWindow();
        if (arguments.Any(argument => string.Equals(argument, "--startup-probe", StringComparison.Ordinal)))
        {
            window.RunStartupProbe();
            window.Close();
            Exit();
            return;
        }
        window.Activate();
    }
}
