using System.Text.Json.Nodes;

namespace SpiceRoute.Windows;

internal static class SelectionSummary
{
    internal static string Mode(JsonObject config, string id) => Wire.Text(Wire.Object(Wire.Object(config, "selection"), "projectModes"), id, Wire.Text(Wire.Object(config, "selection"), "defaultProjectMode", "full"));
    internal static string ModeLabel(string mode) => mode == "full" ? "Full project" : mode == "historyOnly" ? "Chat history only" : "Excluded";
    internal static bool Includes(JsonObject config, JsonObject thread)
    {
        var selection = Wire.Object(config, "selection");
        var projectId = Wire.Text(thread, "projectId");
        var projectRules = Wire.Object(Wire.Object(selection, "projectContent"), projectId);
        var archived = projectId.Length == 0 ? Wire.Bool(selection, "includeArchived", true)
            : projectRules.Count > 0 ? Wire.Bool(projectRules, "includeArchived", true)
            : Wire.Bool(selection, "projectModesInitialized") || Wire.Bool(selection, "includeArchived", true);
        return !Wire.Array(selection, "excludedThreadIds").Any(id => id?.ToString() == Wire.Text(thread, "id"))
            && (archived || !Wire.Bool(thread, "archived"))
            && (projectId.Length == 0 || Mode(config, projectId) != "excluded");
    }
    internal static double ProjectBytes(JsonObject config, JsonObject catalog, JsonObject project)
    {
        var id = Wire.Text(project, "id");
        var mode = Mode(config, id);
        if (mode == "excluded") return 0;
        return Wire.Array(catalog, "threads").OfType<JsonObject>().Where(thread => Wire.Text(thread, "projectId") == id && Includes(config, thread)).Sum(thread => Wire.Number(thread, "estimatedBytes"))
            + (mode == "full" ? Wire.Number(project, "estimatedBytes") : 0);
    }
    internal static (int Chats, int Full, int History, double Bytes) Count(JsonObject config, JsonObject catalog)
    {
        var threads = Wire.Array(catalog, "threads").OfType<JsonObject>().Where(thread => Includes(config, thread)).ToList();
        var projects = Wire.Array(catalog, "projects").OfType<JsonObject>().ToList();
        var full = projects.Where(project => Mode(config, Wire.Text(project, "id")) == "full").ToList();
        return (threads.Count, full.Count, projects.Count(project => Mode(config, Wire.Text(project, "id")) == "historyOnly"),
            threads.Sum(thread => Wire.Number(thread, "estimatedBytes")) + full.Sum(project => Wire.Number(project, "estimatedBytes")));
    }
}
