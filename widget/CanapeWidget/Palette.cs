using System;
using System.Globalization;
using Windows.UI;

namespace CanapeWidget
{
    /// <summary>
    /// Accent colour helpers: parsing the service's "#rrggbb", WCAG contrast,
    /// and the derived colours (hover, pressed, text on the panel, text on the
    /// accent) so any accent picked in the shell stays readable here.
    /// </summary>
    static class Palette
    {
        /// <summary>Accent when the status carries none.</summary>
        public static readonly Color DefaultAccent = Color.FromArgb(255, 0x25, 0x63, 0xEB);

        /// <summary>Panel surface; keep in sync with PanelBackground in WidgetPage.xaml.</summary>
        public static readonly Color Panel = Color.FromArgb(255, 0x1A, 0x1B, 0x1F);

        public static readonly Color White = Color.FromArgb(255, 255, 255, 255);

        /// <summary>Dark text for light accents (not pure black, to match the panel).</summary>
        public static readonly Color Ink = Color.FromArgb(255, 0x11, 0x12, 0x15);

        /// <summary>"#rrggbb" (either case) to a colour; false for anything else.</summary>
        public static bool TryParse(string text, out Color color)
        {
            color = DefaultAccent;
            if (text == null || text.Length != 7 || text[0] != '#')
            {
                return false;
            }
            for (var i = 1; i < 7; i++)
            {
                var c = text[i];
                var hex = (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
                if (!hex)
                {
                    return false;
                }
            }
            var rgb = int.Parse(text.Substring(1), NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture);
            color = Color.FromArgb(255, (byte)((rgb >> 16) & 0xFF), (byte)((rgb >> 8) & 0xFF), (byte)(rgb & 0xFF));
            return true;
        }

        public static Color WithAlpha(Color color, byte alpha)
        {
            return Color.FromArgb(alpha, color.R, color.G, color.B);
        }

        /// <summary>Opaque blend: t = 0 gives a, t = 1 gives b.</summary>
        public static Color Mix(Color a, Color b, double t)
        {
            return Color.FromArgb(255, Lerp(a.R, b.R, t), Lerp(a.G, b.G, t), Lerp(a.B, b.B, t));
        }

        /// <summary>WCAG 2 contrast ratio, from 1 to 21.</summary>
        public static double Contrast(Color a, Color b)
        {
            var la = Luminance(a) + 0.05;
            var lb = Luminance(b) + 0.05;
            return la > lb ? la / lb : lb / la;
        }

        /// <summary>White or ink, whichever reads better on the accent.</summary>
        public static Color OnAccent(Color accent)
        {
            return Contrast(accent, White) >= Contrast(accent, Ink) ? White : Ink;
        }

        /// <summary>The accent, lightened just enough to be read as text on the panel (4.5:1).</summary>
        public static Color TextOnPanel(Color accent)
        {
            for (var step = 0; step <= 10; step++)
            {
                var candidate = Mix(accent, White, step / 10.0);
                if (Contrast(candidate, Panel) >= 4.5)
                {
                    return candidate;
                }
            }
            return White;
        }

        private static byte Lerp(byte a, byte b, double t)
        {
            var value = a + (b - a) * Math.Min(Math.Max(t, 0), 1);
            return (byte)Math.Round(value);
        }

        private static double Luminance(Color color)
        {
            return 0.2126 * Linear(color.R) + 0.7152 * Linear(color.G) + 0.0722 * Linear(color.B);
        }

        private static double Linear(byte channel)
        {
            var s = channel / 255.0;
            return s <= 0.03928 ? s / 12.92 : Math.Pow((s + 0.055) / 1.055, 2.4);
        }
    }
}
