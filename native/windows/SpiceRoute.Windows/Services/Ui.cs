using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace SpiceRoute.Windows;

internal static class Ui
{
    internal static TextBlock Text(string text, double size = 14, bool semibold = false) => new() { Text = text, FontSize = size, FontWeight = semibold ? Microsoft.UI.Text.FontWeights.SemiBold : Microsoft.UI.Text.FontWeights.Normal, Style = (Style)Application.Current.Resources["SpiceBodyTextStyle"] };
    internal static TextBlock Muted(string text, double size = 11) => new() { Text = text, FontSize = size, Style = (Style)Application.Current.Resources["SpiceMetaTextStyle"] };
    internal static TextBlock PageTitle(string text) => new() { Text = text, Style = (Style)Application.Current.Resources["SpicePageTitleStyle"] };
    internal static TextBlock SectionTitle(string text) => new() { Text = text, Style = (Style)Application.Current.Resources["SpiceSectionTitleStyle"] };
    internal static FontIcon Icon(string glyph, double size = 18) => new() { FontFamily = new FontFamily("Segoe Fluent Icons"), Glyph = glyph, FontSize = size };
    internal static Button Button(string label, string? glyph = null, bool primary = false)
    {
        var content = new StackPanel { Orientation = Orientation.Horizontal, Spacing = 8 };
        if (glyph is not null) content.Children.Add(Icon(glyph, 16));
        // Let the native button presenter supply foreground for every visual state.
        content.Children.Add(new TextBlock { Text = label, FontSize = 13, VerticalAlignment = VerticalAlignment.Center });
        var button = new Button
        {
            Content = content,
            HorizontalAlignment = HorizontalAlignment.Left,
            Style = (Style)Application.Current.Resources[primary ? "SpicePrimaryButtonStyle" : "SpiceSecondaryButtonStyle"]
        };
        AutomationProperties.SetName(button, label);
        return button;
    }
    internal static Button TextButton(string label, string? glyph = null)
    {
        var button = Button(label, glyph);
        button.Style = (Style)Application.Current.Resources["SpiceTextButtonStyle"];
        return button;
    }
    internal static Button IconButton(string label, string glyph)
    {
        var button = new Button
        {
            Content = Icon(glyph, 16),
            Style = (Style)Application.Current.Resources["SpiceIconButtonStyle"],
            HorizontalAlignment = HorizontalAlignment.Left
        };
        AutomationProperties.SetName(button, label);
        ToolTipService.SetToolTip(button, label);
        return button;
    }
    internal static Border StatusPill(string text, bool positive)
    {
        var label = Text(text, 10, true);
        label.Style = Style(positive ? "SpiceSuccessTextStyle" : "SpiceWarningTextStyle");
        return new Border
        {
            Style = Style(positive ? "SpiceSuccessBorderStyle" : "SpiceSubtleBorderStyle"),
            CornerRadius = new CornerRadius(4),
            Padding = new Thickness(7, 3, 7, 3),
            Child = label,
            HorizontalAlignment = HorizontalAlignment.Right,
            VerticalAlignment = VerticalAlignment.Center
        };
    }
    internal static StackPanel Stack(double spacing = 12) => new() { Spacing = spacing };
    internal static Style Style(string name) => (Style)Application.Current.Resources[name];
    internal static Border Rule(double top = 0, double bottom = 0) => new() { Height = 1, Margin = new Thickness(0, top, 0, bottom), Style = Style("SpiceRuleStyle") };
    internal static Grid Columns(params GridLength[] lengths)
    {
        var grid = new Grid { ColumnSpacing = 16 };
        foreach (var length in lengths) grid.ColumnDefinitions.Add(new() { Width = length });
        return grid;
    }
    internal static Grid ColumnsWithSpacing(double spacing, params GridLength[] lengths)
    {
        var grid = new Grid { ColumnSpacing = spacing };
        foreach (var length in lengths) grid.ColumnDefinitions.Add(new() { Width = length });
        return grid;
    }
    internal static T WithMargin<T>(T element, Thickness margin) where T : FrameworkElement
    {
        element.Margin = margin;
        return element;
    }
    internal static T WithAlignment<T>(T element, VerticalAlignment vertical, HorizontalAlignment horizontal = HorizontalAlignment.Stretch) where T : FrameworkElement
    {
        element.VerticalAlignment = vertical;
        element.HorizontalAlignment = horizontal;
        return element;
    }
    internal static void Add(Grid parent, FrameworkElement child, int row = 0, int column = 0) { Grid.SetRow(child, row); Grid.SetColumn(child, column); parent.Children.Add(child); }
    internal static async Task GuardAsync(Func<Task> action, SpiceRouteContext context) { try { await action(); } catch (Exception error) { context.ShowMessage(error.Message, true); } }
}
