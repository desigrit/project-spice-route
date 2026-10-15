using System.Net;
using System.Security.Cryptography;
using System.Text.Json.Nodes;
using SpiceRoute.Windows;

var count = 0;
void Assert(bool condition, string message) { if (!condition) throw new Exception(message); count++; }
void Reject(Action action, string message) { try { action(); } catch (InvalidDataException) { count++; return; } throw new Exception(message); }
var bytes = System.Text.Encoding.UTF8.GetBytes("A bounded update download fixture.");
JsonObject Manifest(string version = "1.6.9", string arch = "x64") => new()
{
    ["version"] = version,
    ["platforms"] = new JsonObject {
        [arch == "x64" ? "windows-x86_64" : "windows-aarch64"] = new JsonObject {
            ["url"] = $"{AppUpdates.Repository}/releases/download/v{version}/Spice-Route-{version}-windows-{arch}-setup.exe",
            ["size"] = bytes.Length, ["sha256"] = Convert.ToHexString(SHA256.HashData(bytes)), ["signature"] = new string('A', 200)
        }
    }
};
Assert(AppUpdates.ParseManifest(Manifest().ToJsonString(), "1.6.8", "x64")?.Version == "1.6.9", "Offer a newer release");
Assert(AppUpdates.ParseManifest(Manifest("1.6.8").ToJsonString(), "1.6.8", "x64") is null, "Equal versions must not reinstall");
Assert(AppUpdates.ParseManifest(Manifest("1.6.7").ToJsonString(), "1.6.8", "x64") is null, "Never downgrade");
Assert(AppUpdates.ParseManifest(Manifest("1.10.0").ToJsonString(), "1.9.9", "x64") is not null, "Compare version numbers numerically");
Assert(AppUpdates.ParseManifest(Manifest(arch: "arm64").ToJsonString(), "1.6.8", "arm64")?.Architecture == "arm64", "Choose ARM64 package");
Reject(() => AppUpdates.ParseManifest(Manifest().ToJsonString(), "1.6.8", "arm64"), "Missing architecture is not up to date");
Reject(() => AppUpdates.ParseManifest(Manifest("1.6.9-beta.1").ToJsonString(), "1.6.8", "x64"), "Reject prereleases");
foreach (var url in new[] { "http://github.com/file.exe", "https://github.com/another/repo/releases/download/v1.6.9/update.exe", "https://evil.example/update.exe", AppUpdates.Repository + "/releases/download/v1.6.9/Spice-Route-1.6.9-windows-arm64-setup.exe" })
{
    var manifest = Manifest(); manifest["platforms"]!["windows-x86_64"]!["url"] = url;
    Reject(() => AppUpdates.ParseManifest(manifest.ToJsonString(), "1.6.8", "x64"), "Reject mismatched download URL");
}
var oversized = Manifest(); oversized["platforms"]!["windows-x86_64"]!["size"] = AppUpdates.MaxPackageBytes + 1;
Reject(() => AppUpdates.ParseManifest(oversized.ToJsonString(), "1.6.8", "x64"), "Bound package size");
var root = Path.Combine(Path.GetTempPath(), "spice-updater-test-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);
var update = AppUpdates.ParseManifest(Manifest().ToJsonString(), "1.6.8", "x64")!;
try
{
    var verified = false;
    using (var service = new AppUpdates(new FakeHandler(_ => new(HttpStatusCode.OK) { Content = new ByteArrayContent(bytes) })))
    using (var downloaded = await service.DownloadAsync(update, (path, signature) => { verified = File.ReadAllBytes(path).SequenceEqual(bytes) && signature == update.Signature; return Task.CompletedTask; }, cacheRoot: root))
        Assert(verified && File.Exists(downloaded.Path), "Verify exact downloaded bytes before returning installer");
    Assert(!Directory.EnumerateFileSystemEntries(root).Any(), "Clean up unused verified downloads");
    foreach (var mode in new[] { "truncated", "corrupt", "bad-signature", "redirect", "cancel" })
    {
        var content = mode == "truncated" ? bytes[..^1] : mode == "corrupt" ? bytes.Select(b => (byte)(b ^ 1)).ToArray() : bytes;
        using var service = new AppUpdates(new FakeHandler(_ => mode == "redirect" ? new(HttpStatusCode.Redirect) { Headers = { Location = new Uri("https://evil.example/update.exe") } } : new(HttpStatusCode.OK) { Content = new ByteArrayContent(content) }));
        using var cancellation = new CancellationTokenSource();
        if (mode == "cancel") cancellation.Cancel();
        try
        {
            using var unexpected = await service.DownloadAsync(update, (_, _) => mode == "bad-signature" ? Task.FromException(new InvalidDataException("bad signature")) : Task.CompletedTask, cancellation: cancellation.Token, cacheRoot: root);
            throw new Exception($"Accepted {mode} download");
        }
        catch (Exception error) when (error is InvalidDataException or OperationCanceledException) { count++; }
        Assert(!Directory.EnumerateFileSystemEntries(root).Any(), $"Clean up {mode} download");
    }
}
finally { Directory.Delete(root); }
Console.WriteLine($"Passed {count} updater checks. No app or installer was launched.");

if (args.Length == 3)
{
    var profile = Path.Combine(Path.GetTempPath(), "spice-update-signature-" + Guid.NewGuid().ToString("N"));
    try
    {
        await using var engine = new EngineClient(Path.GetFullPath(args[0]), profile);
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
        await engine.CallAsync("verify_update_artifact", new() {
            ["path"] = Path.GetFullPath(args[1]), ["signature"] = File.ReadAllText(args[2]).Trim()
        }, timeout.Token);
        Console.WriteLine("The release package signature matches the application's embedded public key.");
    }
    finally { if (Directory.Exists(profile)) Directory.Delete(profile, true); }
}
else if (args.Length != 0) throw new ArgumentException("Optional arguments: engine package signature-file");

sealed class FakeHandler(Func<HttpRequestMessage, HttpResponseMessage> respond) : HttpMessageHandler
{
    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        return Task.FromResult(respond(request));
    }
}
