using Microsoft.UI.Xaml;
using System.Runtime.InteropServices;

namespace SpiceRoute.Windows;

public partial class App : Application
{
    private MainWindow? window;
    private int startupFailureShown;
    private readonly bool suppressStartupDialog = Environment.GetCommandLineArgs().Any(argument =>
        string.Equals(argument, "--startup-probe", StringComparison.Ordinal) ||
        string.Equals(argument, "--visual-probe", StringComparison.Ordinal));
    public App()
    {
        if (suppressStartupDialog) StartupLog.Suppress();
        UnhandledException += OnUnhandledException;
        AppDomain.CurrentDomain.UnhandledException += (_, args) =>
        {
            if (args.ExceptionObject is Exception error) StartupLog.WriteException("Unhandled application-domain failure", error);
            else StartupLog.Write("Unhandled application-domain failure without an Exception object.");
        };
        TaskScheduler.UnobservedTaskException += (_, args) =>
        {
            StartupLog.WriteException("Unobserved background task failure", args.Exception);
            args.SetObserved();
        };
        StartupLog.WriteProcessContext("Spice Route app constructor entered");
        try
        {
            InitializeComponent();
            StartupLog.Write("Spice Route application resources initialized.");
        }
        catch (Exception error)
        {
            StartupLog.WriteException("Application resource initialization failed", error);
            if (!suppressStartupDialog) ShowStartupFailure(error);
            Environment.ExitCode = 1;
            throw;
        }
    }

    private void OnUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs args)
    {
        StartupLog.WriteException("Unhandled WinUI failure", args.Exception);
        args.Handled = true;
        if (window?.TryShowUnhandledFailure(args.Exception) == true) return;
        if (!suppressStartupDialog && Interlocked.Exchange(ref startupFailureShown, 1) == 0) ShowStartupFailure(args.Exception);
        Environment.ExitCode = 1;
        Exit();
    }

    private static void ShowStartupFailure(Exception error)
    {
        var message = $"Spice Route encountered an unexpected startup problem. Reinstall the package made for this computer, then try again. A diagnostic log was saved to:\n\n{StartupLog.LogPath}\n\n{error.Message}";
        MessageBoxW(IntPtr.Zero, message, "Spice Route could not start", 0x00000010);
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int MessageBoxW(IntPtr window, string text, string caption, uint type);

    protected override async void OnLaunched(LaunchActivatedEventArgs args)
    {
        try
        {
            StartupLog.Write("Application launch started.");
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
            if (arguments.Any(argument => string.Equals(argument, "--startup-probe", StringComparison.Ordinal)))
            {
                var temporaryRoot = Path.GetFullPath(Path.GetTempPath());
                var probeDirectory = Path.GetFullPath(Path.Combine(temporaryRoot, $"SpiceRoute.AppProbe.{Guid.NewGuid():N}"));
                if (!probeDirectory.StartsWith(temporaryRoot, StringComparison.OrdinalIgnoreCase)
                    || !Path.GetFileName(probeDirectory).StartsWith("SpiceRoute.AppProbe.", StringComparison.Ordinal))
                {
                    throw new InvalidOperationException("The disposable startup-probe folder could not be confirmed.");
                }
                Directory.CreateDirectory(probeDirectory);
                try
                {
                    window = new MainWindow(engineDataDirectory: probeDirectory, deferInitialization: true);
                    window.AppWindow.IsShownInSwitchers = false;
                    window.AppWindow.MoveAndResize(new global::Windows.Graphics.RectInt32(-32000, -32000, 1180, 820));
                    window.Activate();
                    await window.RunStartupProbeAsync(probeDirectory);
                    await window.StopEngineForProbeAsync();
                    window.Close();
                    window = null;
                }
                finally
                {
                    if (window is not null)
                    {
                        await window.StopEngineForProbeAsync();
                        window.Close();
                        window = null;
                    }
                    if (Directory.Exists(probeDirectory)) Directory.Delete(probeDirectory, true);
                }
                Environment.ExitCode = 0;
                Exit();
                return;
            }
            window = new MainWindow();
            StartupLog.Write("Main window created.");
            window.Activate();
            StartupLog.Write("Main window activated.");
        }
        catch (Exception error)
        {
            StartupLog.WriteException("App launch failed", error);
            if (!suppressStartupDialog) ShowStartupFailure(error);
            Environment.ExitCode = 1;
            Exit();
        }
    }
}
