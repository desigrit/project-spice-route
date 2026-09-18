// Headless contract tests. Never opens WinUI, closes Codex, or transfers live data.
using SpiceRoute.Windows;
using System.Text.Json.Nodes;

if (args.Length != 1 || !File.Exists(args[0])) throw new ArgumentException("Pass the compiled Rust sidecar path.");
var enginePath = Path.GetFullPath(args[0]);
Require(Wire.ConflictChoice(1) == "local" && Wire.ConflictChoice(2) == "incoming", "Explicit conflict choices");
foreach (var index in new[] { -1, 0, 3 })
{
    try { Wire.ConflictChoice(index); throw new Exception("An unset conflict choice was accepted"); }
    catch (InvalidOperationException) { }
}
var temporary = Path.Combine(Path.GetTempPath(), "SpiceRoute.Contract." + Guid.NewGuid().ToString("N"));
var profile = Path.Combine(temporary, "profile");
Directory.CreateDirectory(temporary);
using var deadline = new CancellationTokenSource(TimeSpan.FromSeconds(30));
await using (var client = new EngineClient(enginePath, profile))
{
    var hello = await client.CallAsync("get_protocol_info", cancellationToken: deadline.Token);
    Require(Wire.Number(hello, "protocolVersion") == 1, "Protocol version handshake");
    var concurrent = await Task.WhenAll(Enumerable.Range(0, 8).Select(_ => client.CallAsync("get_protocol_info", cancellationToken: deadline.Token)));
    Require(concurrent.All(item => Wire.Number(item, "protocolVersion") == 1), "Concurrent request correlation");

    try { await client.CallAsync("open_codex", cancellationToken: deadline.Token); throw new Exception("Removed command unexpectedly succeeded"); }
    catch (InvalidOperationException error) { Require(error.Message.Contains("Unsupported engine operation"), "Actionable operation errors"); }

    var config = await client.CallAsync("load_config", cancellationToken: deadline.Token) as JsonObject ?? throw new Exception("Missing configuration");
    var deviceId = Wire.Text(config, "deviceId");
    var codex = Path.Combine(temporary, "codex");
    var workspaces = Path.Combine(temporary, "workspaces");
    Directory.CreateDirectory(codex);
    Directory.CreateDirectory(workspaces);
    config["codexHome"] = codex;
    config["projectlessRoot"] = workspaces;
    config["projectsRoot"] = "";
    config["cloudRoot"] = "";
    config["deviceName"] = "Disposable IPC test";
    config["onboardingComplete"] = false;
    config["theme"] = "dark";
    var saved = await client.CallAsync("save_config", new() { ["config"] = config.DeepClone() }, deadline.Token);
    Require(Wire.Text(saved, "deviceId") == deviceId, "Device identity retained on save");
    var loaded = await client.CallAsync("load_config", cancellationToken: deadline.Token);
    Require(Wire.Text(loaded, "deviceId") == deviceId && Wire.Text(loaded, "theme") == "dark", "Settings round trip");

    var progress = await client.CallAsync("get_operation_progress", new() { ["operationId"] = "not-running" }, deadline.Token);
    Require(progress is null, "Unknown progress remains null");
    await client.CallAsync("cancel_operation", new() { ["operationId"] = "not-running" }, deadline.Token);

    await using var second = new EngineClient(enginePath, profile);
    try { await second.CallAsync("get_protocol_info", cancellationToken: deadline.Token); throw new Exception("Two engines acquired the same profile"); }
    catch (Exception error) when (error is IOException or InvalidOperationException)
    { Require(error.Message.Contains("already running", StringComparison.OrdinalIgnoreCase), "Profile-lock errors reach the frontend"); }
}
// A new client must be able to take the lock after normal EOF shutdown.
await using (var restarted = new EngineClient(enginePath, profile))
{
    var result = await restarted.CallAsync("get_protocol_info", cancellationToken: deadline.Token);
    Require(Wire.Number(result, "protocolVersion") == 1, "EOF releases the profile lock");
}
await using (var holder = new EngineClient(enginePath, profile))
await using (var retry = new EngineClient(enginePath, profile))
{
    await holder.CallAsync("get_protocol_info", cancellationToken: deadline.Token);
    try { await retry.CallAsync("get_protocol_info", cancellationToken: deadline.Token); throw new Exception("A locked profile unexpectedly opened"); }
    catch (Exception error) when (error is IOException or InvalidOperationException) { }
    await holder.DisposeAsync();
    await retry.RestartIfStoppedAsync();
    var resumed = await retry.CallAsync("get_protocol_info", cancellationToken: deadline.Token);
    Require(Wire.Number(resumed, "protocolVersion") == 1, "Retry recovers after the other app closes");
}
Console.WriteLine("11 headless engine-client contract checks passed. Only a disposable profile was used.");

static void Require(bool passed, string check)
{
    if (!passed) throw new Exception("Failed: " + check);
}
