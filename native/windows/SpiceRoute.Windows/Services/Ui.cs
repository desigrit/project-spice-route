using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace SpiceRoute.Windows;

internal static class Ui
{
    internal static TextBlock Text(string text, double size = 14, bool semibold = false) => new() { Text = text, FontSize = size, FontWeight = semibold ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal, TextWrapping = TextWrapping.Wrap };
    internal static FontIcon Icon(string glyph, double size = 18) => new() { FontFamily = new FontFamily("Segoe Fluent Icons"), Glyph = glyph, FontSize = size };
    internal static Button Button(string label, string? glyph = null, bool primary = false)
    {
        var content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (glyph is not null) content.Children.Add(Icon(glyph, 16));
        content.Children.Add(Text(label));
        var button = new Button { Content = content, MinHeight = 32, HorizontalAlignment = HorizontalAlignment.Left };
        AutomationProperties.SetName(button, label);
        if (primary) button.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        return button;
    }
    internal static StackPanel Stack(double spacing = 12) => new() { Spacing = spacing };
    internal static SolidColorBrush Resource(string name) => (SolidColorBrush)Application.Current.Resources[name];
    internal static Border Rule() => (Border)Microsoft.UI.Xaml.Markup.XamlReader.Load("<Border xmlns='http://schemas.microsoft.com/winfx/2006/xaml/presentation' Height='1' Margin='0,12' Background='{ThemeResource DividerStrokeColorDefaultBrush}'/>");
    internal static Grid Columns(params GridLength[] lengths)
    {
        var grid = new Grid { ColumnSpacing = 20 };
        foreach (var length in lengths) grid.ColumnDefinitions.Add(new() { Width = length });
        return grid;
    }
    internal static void Add(Grid parent, FrameworkElement child, int row = 0, int column = 0) { Grid.SetRow(child, row); Grid.SetColumn(child, column); parent.Children.Add(child); }
    internal static async Task GuardAsync(Func<Task> action, SpiceRouteContext context) { try { await action(); } catch (Exception error) { context.ShowMessage(error.Message, true); } }
}
