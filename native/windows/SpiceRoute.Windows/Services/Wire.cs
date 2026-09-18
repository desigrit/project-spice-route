using System.Globalization;
using System.Text.Json.Nodes;

namespace SpiceRoute.Windows;

public static class Wire
{
    public static string Text(JsonNode? node, string key, string fallback = "") => node?[key]?.GetValueKind() == System.Text.Json.JsonValueKind.String ? node[key]!.GetValue<string>() : fallback;
    public static bool Bool(JsonNode? node, string key, bool fallback = false) => node?[key] is JsonValue value && value.TryGetValue<bool>(out var result) ? result : fallback;
    public static double Number(JsonNode? node, string key, double fallback = 0) => node?[key] is JsonValue value && value.TryGetValue<double>(out var result) ? result : fallback;
    public static JsonArray Array(JsonNode? node, string key) => node?[key] as JsonArray ?? new JsonArray();
    public static JsonObject Object(JsonNode? node, string key) => node?[key] as JsonObject ?? new JsonObject();
    public static JsonObject Clone(JsonObject value) => (JsonObject)value.DeepClone();
    public static string ConflictChoice(int index) => index switch { 1 => "local", 2 => "incoming", _ => throw new InvalidOperationException("Choose a version for every conflict before continuing.") };
    public static string Bytes(double bytes)
    {
        if (!double.IsFinite(bytes) || bytes <= 0) return "0 B";
        var units = new[] { "B", "KB", "MB", "GB", "TB" };
        var unit = Math.Min((int)(Math.Log(bytes) / Math.Log(1024)), units.Length - 1);
        var value = bytes / Math.Pow(1024, unit);
        return value.ToString(unit == 0 || value >= 10 ? "0" : "0.0", CultureInfo.CurrentCulture) + " " + units[unit];
    }
    public static string Time(string? value) => DateTimeOffset.TryParse(value, CultureInfo.InvariantCulture, DateTimeStyles.None, out var timestamp) ? timestamp.ToLocalTime().ToString("MMM d, yyyy · h:mm tt", CultureInfo.CurrentCulture) : value ?? "Not yet";
}
