using System.Collections.Concurrent;
using System.ComponentModel;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace SpiceRoute.Windows;

internal static class StartupLog
{
    private const long MaximumBytes = 256 * 1024;
    private static readonly object Gate = new();
    private static string directory = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "com.spiceroute.codexsync", "logs");
    private static bool suppressed;

    internal static string LogPath
    {
        get { lock (Gate) return Path.Combine(directory, "startup.log"); }
    }

    internal static void Configure(string dataDirectory)
    {
        lock (Gate) directory = Path.Combine(dataDirectory, "logs");
    }

    internal static void Suppress()
    {
        lock (Gate) suppressed = true;
    }

    internal static void WriteProcessContext(string stage)
    {
        Write($"{stage}. processArchitecture={RuntimeInformation.ProcessArchitecture}; osArchitecture={RuntimeInformation.OSArchitecture}; framework={RuntimeInformation.FrameworkDescription}; os={RuntimeInformation.OSDescription}");
    }

    internal static void WriteException(string stage, Exception error)
    {
        Write($"{stage}. exception={error.GetType().FullName}; hresult=0x{error.HResult:X8}; message={Clean(error.Message, 4000)}; stack={Clean(error.StackTrace ?? "unavailable", 12000)}");
    }

    internal static void Write(string message)
    {
        try
        {
            lock (Gate)
            {
                if (suppressed) return;
                Directory.CreateDirectory(directory);
                var current = Path.Combine(directory, "startup.log");
                var previous = Path.Combine(directory, "startup.previous.log");
                if (File.Exists(current) && new FileInfo(current).Length >= MaximumBytes)
                {
                    File.Move(current, previous, true);
                }
                File.AppendAllText(current, $"{DateTimeOffset.UtcNow:O} {Clean(message, 16000)}{Environment.NewLine}", new UTF8Encoding(false));
            }
        }
        catch
        {
            // Startup diagnostics must never become another startup failure.
        }
    }

    private static string Clean(string value, int maximumLength)
    {
        var cleaned = new string(value.Select(character => character is '\r' or '\n' or '\t' || !char.IsControl(character) ? character : ' ').ToArray())
            .Replace("\r\n", " | ", StringComparison.Ordinal)
            .Replace('\r', ' ')
            .Replace('\n', ' ')
            .Trim();
        return cleaned.Length <= maximumLength ? cleaned : cleaned[..maximumLength] + " [truncated]";
    }
}

public sealed class EngineClient : IAsyncDisposable
{
    private static readonly TimeSpan StartupTimeout = TimeSpan.FromSeconds(15);
    private readonly SemaphoreSlim writer = new(1, 1);
    private readonly ConcurrentDictionary<string, TaskCompletionSource<JsonNode?>> pending = new();
    private Process? process;
    private Task? readerTask;
    private Task? errorTask;
    private long nextId;
    private bool disposed;
    private readonly string executablePath;
    private readonly string dataDirectory;
    private readonly ConcurrentQueue<string> diagnostics = new();
    private readonly Func<string, JsonObject?, JsonNode?>? fixtureResponder;

    internal bool HasStartedProcess => process is not null;

    internal EngineClient(Func<string, JsonObject?, JsonNode?> fixtureResponder)
    {
        this.fixtureResponder = fixtureResponder;
        executablePath = "";
        dataDirectory = "";
    }

    public EngineClient(string? executablePath = null, string? dataDirectory = null)
    {
        this.executablePath = executablePath ?? Path.Combine(System.AppContext.BaseDirectory, "SpiceRoute.Engine.exe");
        this.dataDirectory = dataDirectory ?? Path.Combine(System.Environment.GetFolderPath(System.Environment.SpecialFolder.LocalApplicationData), "com.spiceroute.codexsync");
        StartupLog.Configure(this.dataDirectory);
    }

    public async Task RestartIfStoppedAsync()
    {
        ObjectDisposedException.ThrowIf(disposed, this);
        if (process is null || !process.HasExited) return;
        if (readerTask is not null) await readerTask.ConfigureAwait(false);
        if (errorTask is not null) await errorTask.ConfigureAwait(false);
        StartupLog.Write("Clearing a stopped sync engine before retry.");
        process.Dispose(); process = null; readerTask = null; errorTask = null;
        diagnostics.Clear();
    }

    public void Start()
    {
        if (fixtureResponder is not null) return;
        if (process is not null) return;
        ObjectDisposedException.ThrowIf(disposed, this);
        var executable = executablePath;
        if (!File.Exists(executable))
        {
            StartupLog.Write($"Sync engine file is missing. expectedFile={Path.GetFileName(executable)}");
            throw new FileNotFoundException($"The sync engine is missing. Reinstall Spice Route. Details were saved to {StartupLog.LogPath}.");
        }
        var engineArchitecture = ReadExecutableArchitecture(executable);
        StartupLog.WriteProcessContext($"Starting sync engine. engineArchitecture={engineArchitecture}; engineFile={Path.GetFileName(executable)}");
        if (engineArchitecture != "unknown" && !string.Equals(engineArchitecture, RuntimeInformation.ProcessArchitecture.ToString(), StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException($"The installed sync engine is {engineArchitecture}, but the app is {RuntimeInformation.ProcessArchitecture}. Reinstall the matching Spice Route package.");
        }
        var start = new ProcessStartInfo(executable)
        {
            UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden,
            RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
            StandardInputEncoding = new UTF8Encoding(false), StandardOutputEncoding = new UTF8Encoding(false), StandardErrorEncoding = new UTF8Encoding(false),
            WorkingDirectory = System.AppContext.BaseDirectory
        };
        start.ArgumentList.Add("--data-dir");
        start.ArgumentList.Add(dataDirectory);
        try
        {
            process = Process.Start(start) ?? throw new InvalidOperationException("The sync engine could not start.");
        }
        catch (Exception error) when (error is Win32Exception or InvalidOperationException or BadImageFormatException)
        {
            StartupLog.WriteException("Sync engine launch failed", error);
            throw new InvalidOperationException($"The sync engine could not start on this {RuntimeInformation.OSArchitecture} computer. Reinstall the matching Spice Route package. Details were saved to {StartupLog.LogPath}.", error);
        }
        process.EnableRaisingEvents = true;
        var startedProcess = process;
        process.Exited += (_, _) =>
        {
            try { StartupLog.Write($"Sync engine exited. processId={startedProcess.Id}; exitCode={startedProcess.ExitCode}"); }
            catch (InvalidOperationException) { StartupLog.Write("Sync engine exited before its process details could be read."); }
        };
        errorTask = DrainDiagnosticsAsync(process);
        readerTask = ReadResponsesAsync(process);
    }

    public async Task<JsonNode?> CallAsync(string method, JsonObject? parameters = null, CancellationToken cancellationToken = default)
    {
        if (fixtureResponder is not null)
        {
            ObjectDisposedException.ThrowIf(disposed, this);
            cancellationToken.ThrowIfCancellationRequested();
            return fixtureResponder(method, parameters)?.DeepClone();
        }
        Start();
        using var startupDeadline = method == "get_protocol_info" ? CancellationTokenSource.CreateLinkedTokenSource(cancellationToken) : null;
        if (startupDeadline is not null) startupDeadline.CancelAfter(StartupTimeout);
        var requestCancellation = startupDeadline?.Token ?? cancellationToken;
        var id = Interlocked.Increment(ref nextId).ToString(System.Globalization.CultureInfo.InvariantCulture);
        var completion = new TaskCompletionSource<JsonNode?>(TaskCreationOptions.RunContinuationsAsynchronously);
        pending[id] = completion;
        using var registration = requestCancellation.Register(() => { if (pending.TryRemove(id, out var request)) request.TrySetCanceled(requestCancellation); });
        try
        {
            var request = new JsonObject { ["id"] = id, ["method"] = method, ["params"] = parameters?.DeepClone() ?? new JsonObject() };
            await writer.WaitAsync(requestCancellation).ConfigureAwait(false);
            try
            {
                if (process is null || process.HasExited) throw new IOException(StoppedMessage());
                await process.StandardInput.WriteLineAsync(request.ToJsonString().AsMemory(), requestCancellation).ConfigureAwait(false);
                await process.StandardInput.FlushAsync(requestCancellation).ConfigureAwait(false);
            }
            finally { writer.Release(); }
            return await completion.Task.ConfigureAwait(false);
        }
        catch (OperationCanceledException error) when (startupDeadline?.IsCancellationRequested == true && !cancellationToken.IsCancellationRequested)
        {
            StartupLog.WriteException("Sync engine startup handshake timed out", error);
            StopUnresponsiveProcess();
            throw new TimeoutException($"The sync engine did not answer within {StartupTimeout.TotalSeconds:0} seconds. Restart Spice Route and try again. Details were saved to {StartupLog.LogPath}.", error);
        }
        catch { pending.TryRemove(id, out _); throw; }
    }

    private async Task ReadResponsesAsync(Process engine)
    {
        Exception failure = new IOException("The sync engine stopped. Close and reopen Spice Route to continue.");
        try
        {
            while (await engine.StandardOutput.ReadLineAsync().ConfigureAwait(false) is { } line)
            {
                var response = JsonNode.Parse(line) as JsonObject ?? throw new JsonException("Invalid sync-engine response.");
                var id = Wire.Text(response, "id");
                if (!pending.TryRemove(id, out var completion)) continue;
                if (response["error"] is JsonObject error) completion.TrySetException(new InvalidOperationException(Wire.Text(error, "message", "The operation did not complete.")));
                else completion.TrySetResult(response["result"]?.DeepClone());
            }
        }
        catch (Exception error) { failure = new IOException("The sync engine connection failed. " + error.Message, error); }
        finally
        {
            if (engine.HasExited && errorTask is not null) await errorTask.ConfigureAwait(false);
            if (!diagnostics.IsEmpty) failure = new IOException(StoppedMessage(), failure);
            var tail = diagnostics.IsEmpty ? "none" : string.Join(" | ", diagnostics.TakeLast(4));
            StartupLog.Write($"Sync engine response stream closed. exited={engine.HasExited}; diagnostics={tail}");
            foreach (var item in pending) if (pending.TryRemove(item.Key, out var completion)) completion.TrySetException(failure);
        }
    }

    private string StoppedMessage() => diagnostics.IsEmpty ? "The sync engine stopped. Close and reopen Spice Route to continue." : "The sync engine could not continue. " + string.Join(" ", diagnostics.TakeLast(4));

    private async Task DrainDiagnosticsAsync(Process engine)
    {
        // Diagnostics never share the structured response stream or become file rows.
        try
        {
            while (await engine.StandardError.ReadLineAsync().ConfigureAwait(false) is { } line)
            {
                diagnostics.Enqueue(line.Length > 1000 ? line[..1000] : line);
                while (diagnostics.Count > 8) diagnostics.TryDequeue(out _);
            }
        }
        catch (IOException) { }
        catch (ObjectDisposedException) { }
    }

    private void StopUnresponsiveProcess()
    {
        try
        {
            if (process is { HasExited: false }) process.Kill(true);
        }
        catch (Exception error) when (error is InvalidOperationException or Win32Exception or NotSupportedException)
        {
            StartupLog.WriteException("Could not stop an unresponsive sync engine", error);
        }
    }

    private static string ReadExecutableArchitecture(string path)
    {
        try
        {
            using var stream = File.Open(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            using var reader = new BinaryReader(stream, Encoding.UTF8, leaveOpen: true);
            if (stream.Length < 64 || reader.ReadUInt16() != 0x5A4D) return "unknown";
            stream.Position = 0x3C;
            var headerOffset = reader.ReadInt32();
            if (headerOffset < 0 || headerOffset > stream.Length - 6) return "unknown";
            stream.Position = headerOffset;
            if (reader.ReadUInt32() != 0x00004550) return "unknown";
            return reader.ReadUInt16() switch
            {
                0x8664 => "X64",
                0xAA64 => "Arm64",
                0x014C => "X86",
                _ => "unknown"
            };
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException)
        {
            StartupLog.WriteException("Could not inspect sync engine architecture", error);
            return "unknown";
        }
    }

    public async ValueTask DisposeAsync()
    {
        if (disposed) return;
        disposed = true;
        if (process is not null)
        {
            try { process.StandardInput.Close(); }
            catch (InvalidOperationException) { }
            if (!process.HasExited) await process.WaitForExitAsync().ConfigureAwait(false);
            if (readerTask is not null) await readerTask.ConfigureAwait(false);
            if (errorTask is not null) await errorTask.ConfigureAwait(false);
            process.Dispose();
        }
        writer.Dispose();
    }
}
