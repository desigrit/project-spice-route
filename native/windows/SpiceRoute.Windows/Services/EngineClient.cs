using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;

namespace SpiceRoute.Windows;

public sealed class EngineClient : IAsyncDisposable
{
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

    public EngineClient(string? executablePath = null, string? dataDirectory = null)
    {
        this.executablePath = executablePath ?? Path.Combine(System.AppContext.BaseDirectory, "SpiceRoute.Engine.exe");
        this.dataDirectory = dataDirectory ?? Path.Combine(System.Environment.GetFolderPath(System.Environment.SpecialFolder.LocalApplicationData), "com.spiceroute.codexsync");
    }

    public async Task RestartIfStoppedAsync()
    {
        ObjectDisposedException.ThrowIf(disposed, this);
        if (process is null || !process.HasExited) return;
        if (readerTask is not null) await readerTask.ConfigureAwait(false);
        if (errorTask is not null) await errorTask.ConfigureAwait(false);
        process.Dispose(); process = null; readerTask = null; errorTask = null;
        diagnostics.Clear();
    }

    public void Start()
    {
        if (process is not null) return;
        ObjectDisposedException.ThrowIf(disposed, this);
        var executable = executablePath;
        if (!File.Exists(executable)) throw new FileNotFoundException("The sync engine is missing. Reinstall Spice Route.");
        var start = new ProcessStartInfo(executable)
        {
            UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden,
            RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
            StandardInputEncoding = new UTF8Encoding(false), StandardOutputEncoding = new UTF8Encoding(false), StandardErrorEncoding = new UTF8Encoding(false),
            WorkingDirectory = System.AppContext.BaseDirectory
        };
        start.ArgumentList.Add("--data-dir");
        start.ArgumentList.Add(dataDirectory);
        process = Process.Start(start) ?? throw new InvalidOperationException("The sync engine could not start.");
        errorTask = DrainDiagnosticsAsync(process);
        readerTask = ReadResponsesAsync(process);
    }

    public async Task<JsonNode?> CallAsync(string method, JsonObject? parameters = null, CancellationToken cancellationToken = default)
    {
        Start();
        var id = Interlocked.Increment(ref nextId).ToString(System.Globalization.CultureInfo.InvariantCulture);
        var completion = new TaskCompletionSource<JsonNode?>(TaskCreationOptions.RunContinuationsAsynchronously);
        pending[id] = completion;
        using var registration = cancellationToken.Register(() => { if (pending.TryRemove(id, out var request)) request.TrySetCanceled(cancellationToken); });
        try
        {
            var request = new JsonObject { ["id"] = id, ["method"] = method, ["params"] = parameters?.DeepClone() ?? new JsonObject() };
            await writer.WaitAsync(cancellationToken).ConfigureAwait(false);
            try
            {
                if (process is null || process.HasExited) throw new IOException(StoppedMessage());
                await process.StandardInput.WriteLineAsync(request.ToJsonString().AsMemory(), cancellationToken).ConfigureAwait(false);
                await process.StandardInput.FlushAsync(cancellationToken).ConfigureAwait(false);
            }
            finally { writer.Release(); }
            return await completion.Task.ConfigureAwait(false);
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

    public async ValueTask DisposeAsync()
    {
        if (disposed) return;
        disposed = true;
        if (process is not null)
        {
            process.StandardInput.Close();
            await process.WaitForExitAsync().ConfigureAwait(false);
            if (readerTask is not null) await readerTask.ConfigureAwait(false);
            if (errorTask is not null) await errorTask.ConfigureAwait(false);
            process.Dispose();
        }
        writer.Dispose();
    }
}
