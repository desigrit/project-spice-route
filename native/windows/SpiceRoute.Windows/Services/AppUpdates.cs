using System.Diagnostics;
using System.Net;
using System.Net.Http;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

namespace SpiceRoute.Windows;

public sealed record AppUpdate(string Version, Uri Url, string Signature, string Sha256, long Size, string Architecture);

// Independent of WinUI so the network, validation, and interrupted-download paths
// can be exercised without opening a window or changing an installation.
public sealed class AppUpdates : IDisposable
{
    internal const string Repository = "https://github.com/desigrit/project-spice-route";
    internal const string Feed = Repository + "/releases/latest/download/latest.json";
    internal const long MaxPackageBytes = 512L * 1024 * 1024;
    private readonly HttpClient _client;

    public AppUpdates(HttpMessageHandler? handler = null)
    {
        _client = new(handler ?? new HttpClientHandler { AllowAutoRedirect = false });
        _client.Timeout = Timeout.InfiniteTimeSpan;
        _client.DefaultRequestHeaders.UserAgent.ParseAdd("SpiceRoute-Updater/1.0");
        _client.DefaultRequestHeaders.CacheControl = new() { NoCache = true };
    }

    public static string CurrentVersion => typeof(AppUpdates).Assembly.GetName().Version?.ToString(3) ?? "0.0.0";
    public static string Architecture => RuntimeInformation.OSArchitecture switch
    {
        System.Runtime.InteropServices.Architecture.Arm64 => "arm64",
        System.Runtime.InteropServices.Architecture.X64 => "x64",
        _ => throw new InvalidOperationException("Updates are available for Windows x64 and Arm64.")
    };

    public async Task<AppUpdate?> CheckAsync(CancellationToken cancellation = default)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
        timeout.CancelAfter(TimeSpan.FromSeconds(30));
        using var response = await GetAsync(new Uri(Feed), timeout.Token);
        if (response.StatusCode == HttpStatusCode.NotFound)
            throw new InvalidOperationException("No update is published yet. Try again later.");
        response.EnsureSuccessStatusCode();
        await using var body = await response.Content.ReadAsStreamAsync(timeout.Token);
        using var data = new MemoryStream();
        var buffer = new byte[8192];
        int read;
        while ((read = await body.ReadAsync(buffer, timeout.Token)) > 0)
        {
            if (data.Length + read > 65536) throw new InvalidDataException("The update information is too large.");
            data.Write(buffer, 0, read);
        }
        return ParseManifest(System.Text.Encoding.UTF8.GetString(data.ToArray()), CurrentVersion, Architecture);
    }

    internal static AppUpdate? ParseManifest(string json, string currentVersion, string architecture)
    {
        var root = JsonNode.Parse(json) as JsonObject ?? throw new InvalidDataException("The update information is invalid.");
        var version = root["version"]?.GetValue<string>() ?? "";
        if (!TryVersion(version, out var latest) || !TryVersion(currentVersion, out var current))
            throw new InvalidDataException("The update version is invalid.");
        if (latest <= current) return null;
        var platform = architecture switch { "x64" => "windows-x86_64", "arm64" => "windows-aarch64", _ => "unsupported" };
        if (root["platforms"]?[platform] is not JsonObject package)
            throw new InvalidDataException("This release is not available for your device yet. Try again later.");
        var expectedUrl = $"{Repository}/releases/download/v{version}/Spice-Route-{version}-windows-{architecture}-setup.exe";
        var url = package["url"]?.GetValue<string>();
        var signature = package["signature"]?.GetValue<string>() ?? "";
        var sha = package["sha256"]?.GetValue<string>() ?? "";
        var size = package["size"]?.GetValue<long>() ?? 0;
        if (url != expectedUrl || signature.Length is < 100 or > 4096 ||
            !Regex.IsMatch(sha, "\\A[0-9a-fA-F]{64}\\z") || size <= 0 || size > MaxPackageBytes)
            throw new InvalidDataException("The update package information could not be verified.");
        return new(version, new Uri(url), signature, sha, size, architecture);
    }

    private static bool TryVersion(string value, out Version version)
    {
        version = new Version();
        return Regex.IsMatch(value, "\\A(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\z") && Version.TryParse(value, out version!);
    }

    public async Task<DownloadedUpdate> DownloadAsync(AppUpdate update, Func<string, string, Task> verifySignature,
        IProgress<int>? progress = null, CancellationToken cancellation = default, string? cacheRoot = null)
    {
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellation);
        timeout.CancelAfter(TimeSpan.FromMinutes(15));
        var root = cacheRoot ?? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "com.spiceroute.codexsync", "updates");
        var directory = Path.Combine(root, Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(directory);
        var path = Path.Combine(directory, "SpiceRoute-setup.exe");
        FileStream? locked = null;
        try
        {
            using var response = await GetAsync(update.Url, timeout.Token);
            response.EnsureSuccessStatusCode();
            if (response.Content.Headers.ContentLength is long length && length != update.Size)
                throw new InvalidDataException("The update download is incomplete. Try again.");
            await using (var input = await response.Content.ReadAsStreamAsync(timeout.Token))
            await using (var output = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.None, 65536, true))
            {
                var buffer = new byte[65536];
                long total = 0;
                var lastPercent = -1;
                using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
                int read;
                while ((read = await input.ReadAsync(buffer, timeout.Token)) > 0)
                {
                    total += read;
                    if (total > update.Size || total > MaxPackageBytes) throw new InvalidDataException("The update download has an unexpected size.");
                    hash.AppendData(buffer, 0, read);
                    await output.WriteAsync(buffer.AsMemory(0, read), timeout.Token);
                    var percent = (int)(total * 100 / update.Size);
                    if (lastPercent != percent) { progress?.Report(percent); lastPercent = percent; }
                }
                if (total != update.Size || !Convert.ToHexString(hash.GetHashAndReset()).Equals(update.Sha256, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("The update download could not be verified. Try again.");
                output.Flush(true);
            }
            // Keep a read-only sharing handle until the installer has started. This
            // prevents a package from being replaced between verification and launch.
            locked = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
            await verifySignature(path, update.Signature);
            timeout.Token.ThrowIfCancellationRequested();
            return new DownloadedUpdate(path, directory, locked, update);
        }
        catch
        {
            locked?.Dispose();
            Cleanup(directory, path);
            throw;
        }
    }

    private async Task<HttpResponseMessage> GetAsync(Uri uri, CancellationToken cancellation)
    {
        for (var redirect = 0; redirect < 6; redirect++)
        {
            if (uri.Scheme != "https" || !uri.IsDefaultPort || !string.IsNullOrEmpty(uri.UserInfo) ||
                uri.Host is not ("github.com" or "release-assets.githubusercontent.com" or "objects.githubusercontent.com"))
                throw new InvalidDataException("The update download redirected to an unexpected location.");
            var response = await _client.GetAsync(uri, HttpCompletionOption.ResponseHeadersRead, cancellation);
            if (response.StatusCode is not (HttpStatusCode.MovedPermanently or HttpStatusCode.Redirect or HttpStatusCode.RedirectMethod or HttpStatusCode.TemporaryRedirect or HttpStatusCode.PermanentRedirect))
                return response;
            var location = response.Headers.Location;
            response.Dispose();
            uri = location is null ? throw new InvalidDataException("The update download location is missing.") : new Uri(uri, location);
        }
        throw new InvalidDataException("The update download redirected too many times.");
    }

    internal static void Cleanup(string directory, string file)
    {
        try { File.Delete(file); Directory.Delete(directory); }
        catch (IOException) { /* A running installer may still hold the file. */ }
        catch (UnauthorizedAccessException) { }
    }

    public void Dispose() => _client.Dispose();
}

public sealed class DownloadedUpdate(string path, string directory, FileStream locked, AppUpdate update) : IDisposable
{
    public string Path { get; } = path;
    public void StartInstaller()
    {
        var info = FileVersionInfo.GetVersionInfo(Path);
        if (info.ProductVersion != update.Version || info.FileDescription != $"Spice Route Windows {update.Architecture} installer")
            throw new InvalidDataException("The signed installer does not match the selected release.");
        var start = new ProcessStartInfo(Path) { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden };
        start.ArgumentList.Add("/S");
        start.ArgumentList.Add($"/UPDATE_PID={Environment.ProcessId}");
        using var process = Process.Start(start) ?? throw new IOException("The update installer could not start.");
    }
    public void Dispose() { locked.Dispose(); AppUpdates.Cleanup(directory, Path); }
}
