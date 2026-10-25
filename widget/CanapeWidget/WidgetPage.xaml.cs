using System;
using System.Globalization;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Gaming.XboxGameBar;
using Windows.ApplicationModel.Resources;
using Windows.Data.Json;
using Windows.System;
using Windows.UI;
using Windows.UI.Core;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Controls.Primitives;
using Windows.UI.Xaml.Media;
using Windows.UI.Xaml.Media.Animation;
using Windows.UI.Xaml.Navigation;

namespace CanapeWidget
{
    /// <summary>
    /// Quick settings in Game Bar: the shell's Performance page on one tab,
    /// its Hardware page (fans, frame rate cap, controller) on the other.
    /// </summary>
    public sealed partial class WidgetPage : Page
    {
        private const string Dash = "\u2014";
        // ERROR_FILE_NOT_FOUND: no pipe server, i.e. the service isn't running.
        private const string ErrorFileNotFound = "2";

        // The service pushes a status once per second: 60 samples = the last minute.
        private const int HistoryLength = 60;

        // Stops of the auto TDP target, as on the shell's Performance page.
        private static readonly int[] TargetStops = { 30, 40, 45, 60, 90, 120 };

        // Stops of the frame rate cap, as on the shell's Hardware page; 0 is none.
        private static readonly int[] LimitStops = { 0, 30, 40, 45, 60, 90, 120 };

        // Opens the shell (see the identity package's canape protocol).
        private static readonly Uri ShellUri = new Uri("canape://home");

        // Rows and lines slide in from the left, one after the other, like
        // the title on the shell's games page.
        private const double RevealShift = -14;
        private static readonly TimeSpan RevealDuration = TimeSpan.FromMilliseconds(460);
        private static readonly TimeSpan RevealStagger = TimeSpan.FromMilliseconds(70);

        // The power bar's scale when the service does not report the model's range.
        private const int DefaultFloorWatts = 5;
        private const int DefaultCeilingWatts = 35;

        private readonly ResourceLoader strings = ResourceLoader.GetForCurrentView();
        private readonly CanapeLink link = new CanapeLink();
        private readonly LiveGraph powerGraph;
        private CancellationTokenSource follow;
        private JsonObject lastStatus;
        private XboxGameBarWidget widget;
        private Color? appliedAccent;
        // Graphs are not redrawn while Game Bar hides the widget.
        private bool graphsStale;
        // The power bar, in watts: its scale, the knobs (NaN hides one) and the live draw.
        private double rangeFloor = DefaultFloorWatts;
        private double rangeCeiling = DefaultCeilingWatts;
        private double rangeLow = double.NaN;
        private double rangeHigh = double.NaN;
        private double livePower = double.NaN;
        // The pane in front: "power" or "hardware".
        private string tab = "power";
        // A save just went through: the button says so for a moment.
        private bool saved;
        private DispatcherTimer savedTimer;

        public WidgetPage()
        {
            InitializeComponent();
            powerGraph = new LiveGraph(PowerPlot, PowerLine, null, PowerLimitLine, HistoryLength) { MinimumScale = 10 };
            link.StatusReceived += status => OnUi(() => OnStatus(status));
            link.Disconnected += problem => OnUi(() => ShowDisconnected(problem));
            ApplyAccent(null);
            ShowWaiting();
            Loaded += (sender, e) => Reveal(Rows.Children);
        }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            if (widget != null)
            {
                widget.RequestedOpacityChanged += OnRequestedOpacityChanged;
                ApplyOpacity();
            }
            var window = Window.Current;
            if (window != null)
            {
                window.VisibilityChanged += OnWindowVisibilityChanged;
            }
            follow = new CancellationTokenSource();
            _ = link.FollowAsync(follow.Token);
        }

        protected override void OnNavigatedFrom(NavigationEventArgs e)
        {
            follow?.Cancel();
            var window = Window.Current;
            if (window != null)
            {
                window.VisibilityChanged -= OnWindowVisibilityChanged;
            }
            if (widget != null)
            {
                widget.RequestedOpacityChanged -= OnRequestedOpacityChanged;
                widget = null;
            }
        }

        private void OnWindowVisibilityChanged(object sender, VisibilityChangedEventArgs e)
        {
            if (e.Visible && graphsStale)
            {
                RenderGraphs();
            }
        }

        private void OnUi(Action action)
        {
            _ = Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () => action());
        }

        // Game Bar's "widget transparency" setting: only the panel fades, text stays opaque.
        private void OnRequestedOpacityChanged(XboxGameBarWidget sender, object args)
        {
            OnUi(ApplyOpacity);
        }

        private void ApplyOpacity()
        {
            var panel = Resources["PanelBackground"] as SolidColorBrush;
            if (widget != null && panel != null)
            {
                panel.Opacity = widget.RequestedOpacity;
            }
        }

        // ---- Accent -------------------------------------------------------

        /// <summary>
        /// Recolours the accent brushes in place (Status.accent, "#rrggbb", or
        /// the default): the graph, the values and the power bar follow at once.
        /// </summary>
        private void ApplyAccent(string text)
        {
            Color accent;
            if (!Palette.TryParse(text, out accent))
            {
                accent = Palette.DefaultAccent;
            }
            if (appliedAccent.HasValue && appliedAccent.Value.Equals(accent))
            {
                return;
            }
            appliedAccent = accent;

            SetBrush(Resources, "AccentBrush", accent);
            SetBrush(Resources, "AccentTextBrush", Palette.TextOnPanel(accent));
            ProfileChipBorder.Background = new SolidColorBrush(Color.FromArgb(0x24, accent.R, accent.G, accent.B));
        }

        private static void SetBrush(ResourceDictionary dictionary, string key, Color color)
        {
            object value;
            if (dictionary != null && dictionary.TryGetValue(key, out value))
            {
                var brush = value as SolidColorBrush;
                if (brush != null)
                {
                    brush.Color = color;
                }
            }
        }

        // ---- Strings ------------------------------------------------------

        private string S(string key)
        {
            var value = strings.GetString(key);
            return string.IsNullOrEmpty(value) ? key : value;
        }

        private string F(string key, params object[] args)
        {
            return string.Format(CultureInfo.CurrentCulture, S(key), args);
        }

        private string Describe(LinkProblem problem)
        {
            switch (problem.Kind)
            {
                case LinkProblemKind.Unreachable:
                    return problem.Detail == ErrorFileNotFound ? S("ReasonNotRunning") : F("ReasonUnreachable", problem.Detail);
                case LinkProblemKind.Stopped:
                    return S("ReasonStopped");
                case LinkProblemKind.Rejected:
                    return F("ReasonRejected", problem.Detail);
                default:
                    return F("ReasonOther", problem.Detail);
            }
        }

        // ---- JSON helpers (serde writes None as null) ---------------------

        private static double? Number(JsonObject json, string name)
        {
            if (json == null)
            {
                return null;
            }
            var value = json.GetNamedValue(name, JsonValue.CreateNullValue());
            return value.ValueType == JsonValueType.Number ? value.GetNumber() : (double?)null;
        }

        private static int? Integer(JsonObject json, string name)
        {
            var value = Number(json, name);
            return value.HasValue ? (int)Math.Round(value.Value) : (int?)null;
        }

        private static string Text(JsonObject json, string name)
        {
            if (json == null)
            {
                return null;
            }
            var value = json.GetNamedValue(name, JsonValue.CreateNullValue());
            return value.ValueType == JsonValueType.String ? value.GetString() : null;
        }

        private static bool? Flag(JsonObject json, string name)
        {
            if (json == null)
            {
                return null;
            }
            var value = json.GetNamedValue(name, JsonValue.CreateNullValue());
            return value.ValueType == JsonValueType.Boolean ? value.GetBoolean() : (bool?)null;
        }

        private static JsonObject Child(JsonObject json, string name)
        {
            if (json == null)
            {
                return null;
            }
            var value = json.GetNamedValue(name, JsonValue.CreateNullValue());
            return value.ValueType == JsonValueType.Object ? value.GetObject() : null;
        }

        private JsonObject AutoTdp()
        {
            return Child(lastStatus, "auto_tdp");
        }

        /// <summary>"C:\Games\EldenRing.exe" or "eldenring.exe" -> "eldenring".</summary>
        private static string GameName(string exe)
        {
            var name = exe;
            var slash = Math.Max(name.LastIndexOf('\\'), name.LastIndexOf('/'));
            if (slash >= 0)
            {
                name = name.Substring(slash + 1);
            }
            if (name.EndsWith(".exe", StringComparison.OrdinalIgnoreCase) && name.Length > 4)
            {
                name = name.Substring(0, name.Length - 4);
            }
            return name;
        }

        // ---- Display ------------------------------------------------------

        private void ShowWaiting()
        {
            GameText.Text = S("Connecting");
            GameText.Opacity = 0.6;
            ClearValues();
        }

        private void ShowDisconnected(LinkProblem problem)
        {
            lastStatus = null;
            GameText.Text = S("NotConnected");
            GameText.Opacity = 0.6;
            ClearValues();
            ShowMessage(problem == null ? null : Describe(problem));
        }

        private void ClearValues()
        {
            ProfileChipBorder.Visibility = Visibility.Collapsed;
            PowerValue.Text = Dash;
            PowerUnit.Text = "";
            FpsValue.Text = Dash;
            TempValue.Text = Dash;
            powerGraph.Clear();
            BenchCard.Visibility = Visibility.Collapsed;
            ConflictText.Visibility = Visibility.Collapsed;
            foreach (var button in Tiles())
            {
                button.IsChecked = false;
                button.IsEnabled = false;
            }
            ShowModeIcons();
            FansHint.Text = "";
            LimitValue.Text = Dash;
            LimitDown.IsEnabled = false;
            LimitUp.IsEnabled = false;
            ShowSaveButtons(false);
            StepLabel.Text = S("PowerLimitLabel");
            StepValue.Text = Dash;
            ShowHint(StepHint, "");
            StepDown.IsEnabled = false;
            StepUp.IsEnabled = false;
            RangeText.Text = "";
            rangeLow = double.NaN;
            rangeHigh = double.NaN;
            livePower = double.NaN;
            ShowRange();
        }

        private void ShowMessage(string message)
        {
            MessageText.Text = message ?? "";
            MessageText.Visibility = string.IsNullOrEmpty(message) ? Visibility.Collapsed : Visibility.Visible;
        }

        private static void ShowHint(TextBlock hint, string text)
        {
            hint.Text = text ?? "";
            hint.Visibility = string.IsNullOrEmpty(text) ? Visibility.Collapsed : Visibility.Visible;
        }

        /// <summary>Warning text colour, or back to the style's dimmed look.</summary>
        private void Warn(TextBlock text, bool warning)
        {
            if (warning)
            {
                text.Foreground = (Brush)Resources["WarningBrush"];
            }
            else
            {
                text.ClearValue(TextBlock.ForegroundProperty);
            }
        }

        /// <summary>Every tile of both panes.</summary>
        private ToggleButton[] Tiles()
        {
            return new[]
            {
                SilentButton, PerformanceButton, TurboButton, AutoButton,
                FanAutoButton, FanBalancedButton, FanMaxButton,
                GamepadButton, KeyboardButton, MouseButton,
            };
        }

        /// <summary>A chosen tile is white: its icon turns dark like its name.</summary>
        private void ShowModeIcons()
        {
            var ink = new SolidColorBrush(Palette.Ink);
            var soft = (Brush)Resources["SoftTextBrush"];
            var icons = new[]
            {
                SilentIcon, PerformanceIcon, TurboIcon, AutoIcon,
                FanAutoIcon, FanBalancedIcon, FanMaxIcon,
                GamepadIcon, KeyboardIcon, MouseIcon,
            };
            var tiles = Tiles();
            for (var i = 0; i < tiles.Length; i++)
            {
                icons[i].Stroke = tiles[i].IsChecked == true ? ink : soft;
            }
        }

        // ---- Motion -------------------------------------------------------

        /// <summary>
        /// Slides each visible element in from the left and fades it in, one
        /// after the other.
        /// </summary>
        private static void Reveal(UIElementCollection elements)
        {
            var storyboard = new Storyboard();
            var ease = new ExponentialEase { EasingMode = EasingMode.EaseOut, Exponent = 6 };
            var order = 0;
            foreach (var element in elements)
            {
                if (element.Visibility != Visibility.Visible)
                {
                    continue;
                }
                var shift = element.RenderTransform as TranslateTransform;
                if (shift == null)
                {
                    shift = new TranslateTransform();
                    element.RenderTransform = shift;
                }
                var begin = TimeSpan.FromTicks(RevealStagger.Ticks * order++);
                var slide = new DoubleAnimation { From = RevealShift, To = 0, Duration = RevealDuration, BeginTime = begin, EasingFunction = ease };
                Storyboard.SetTarget(slide, shift);
                Storyboard.SetTargetProperty(slide, "X");
                var fade = new DoubleAnimation { From = 0, To = 1, Duration = RevealDuration, BeginTime = begin, EasingFunction = ease };
                Storyboard.SetTarget(fade, element);
                Storyboard.SetTargetProperty(fade, "Opacity");
                element.Opacity = 0;
                storyboard.Children.Add(slide);
                storyboard.Children.Add(fade);
            }
            storyboard.Begin();
        }

        /// <summary>A value that changed slides a little from the left.</summary>
        private static void Bump(TextBlock text, string value)
        {
            if (text.Text == value)
            {
                return;
            }
            var changed = text.Text != Dash && value != Dash;
            text.Text = value;
            var shift = text.RenderTransform as TranslateTransform;
            if (!changed || shift == null)
            {
                return;
            }
            var ease = new ExponentialEase { EasingMode = EasingMode.EaseOut, Exponent = 6 };
            var duration = TimeSpan.FromMilliseconds(260);
            var slide = new DoubleAnimation { From = -6, To = 0, Duration = duration, EasingFunction = ease };
            Storyboard.SetTarget(slide, shift);
            Storyboard.SetTargetProperty(slide, "X");
            var fade = new DoubleAnimation { From = 0.2, To = 1, Duration = duration, EasingFunction = ease };
            Storyboard.SetTarget(fade, text);
            Storyboard.SetTargetProperty(fade, "Opacity");
            var storyboard = new Storyboard();
            storyboard.Children.Add(slide);
            storyboard.Children.Add(fade);
            storyboard.Begin();
        }

        // ---- Tabs ---------------------------------------------------------

        private void OnTab(object sender, RoutedEventArgs e)
        {
            ShowTab((string)((FrameworkElement)sender).Tag);
        }

        private void ShowTab(string name)
        {
            var changed = name != tab;
            tab = name;
            var hardware = name == "hardware";
            TabPower.Foreground = (Brush)Resources[hardware ? "DimTextBrush" : "TextBrush"];
            TabHardware.Foreground = (Brush)Resources[hardware ? "TextBrush" : "DimTextBrush"];
            PowerPane.Visibility = hardware ? Visibility.Collapsed : Visibility.Visible;
            HardwarePane.Visibility = hardware ? Visibility.Visible : Visibility.Collapsed;
            MoveTabPill(true);
            if (changed)
            {
                Reveal((hardware ? HardwarePane : PowerPane).Children);
            }
        }

        private void OnTabsSize(object sender, SizeChangedEventArgs e)
        {
            MoveTabPill(false);
        }

        /// <summary>The pill sits under the tab in front; it slides there.</summary>
        private void MoveTabPill(bool animate)
        {
            var width = (Tabs.ActualWidth - Tabs.Padding.Left - Tabs.Padding.Right) / 2;
            if (width <= 0)
            {
                return;
            }
            TabPill.Width = width;
            var to = tab == "hardware" ? width : 0;
            if (!animate)
            {
                TabPillShift.X = to;
                return;
            }
            // Eased at both ends: a sharp start reads as a jump on a short way.
            var slide = new DoubleAnimation
            {
                To = to,
                Duration = TimeSpan.FromMilliseconds(320),
                EasingFunction = new CubicEase { EasingMode = EasingMode.EaseInOut },
            };
            Storyboard.SetTarget(slide, TabPillShift);
            Storyboard.SetTargetProperty(slide, "X");
            var storyboard = new Storyboard();
            storyboard.Children.Add(slide);
            storyboard.Begin();
        }

        private string Watts(double watts)
        {
            return watts.ToString("0.0", CultureInfo.CurrentCulture);
        }

        // ---- Power bar ----------------------------------------------------

        private void OnRangeSize(object sender, SizeChangedEventArgs e)
        {
            ShowRange();
        }

        /// <summary>Places the track, the knobs and the live mark on the model's scale.</summary>
        private void ShowRange()
        {
            var width = RangePlot.ActualWidth;
            if (width <= 0)
            {
                return;
            }
            Func<double, double> x = w => width * Math.Min(Math.Max((w - rangeFloor) / Math.Max(rangeCeiling - rangeFloor, 1), 0), 1);
            RangeTrack.Width = width;

            var hasHigh = !double.IsNaN(rangeHigh);
            var hasLow = !double.IsNaN(rangeLow);
            var from = hasLow ? x(rangeLow) : 0;
            var to = hasHigh ? x(rangeHigh) : 0;
            RangeFill.Visibility = hasHigh ? Visibility.Visible : Visibility.Collapsed;
            Canvas.SetLeft(RangeFill, from);
            RangeFill.Width = Math.Max(to - from, 0);
            LowKnob.Visibility = hasLow ? Visibility.Visible : Visibility.Collapsed;
            Canvas.SetLeft(LowKnob, from - LowKnob.Width / 2);
            HighKnob.Visibility = hasHigh ? Visibility.Visible : Visibility.Collapsed;
            Canvas.SetLeft(HighKnob, to - HighKnob.Width / 2);

            var live = !double.IsNaN(livePower);
            LiveMark.Visibility = live ? Visibility.Visible : Visibility.Collapsed;
            LiveText.Visibility = LiveMark.Visibility;
            if (live)
            {
                var at = x(livePower);
                Canvas.SetLeft(LiveMark, at - LiveMark.Width / 2);
                LiveText.Text = Watts(livePower) + " " + S("UnitWatts");
                LiveText.Measure(new Windows.Foundation.Size(double.PositiveInfinity, double.PositiveInfinity));
                var textWidth = LiveText.DesiredSize.Width;
                Canvas.SetLeft(LiveText, Math.Min(Math.Max(at - textWidth / 2, 0), Math.Max(width - textWidth, 0)));
            }
        }

        /// <summary>A status push: one more sample in the graphs, then the display.</summary>
        private void OnStatus(JsonObject status)
        {
            powerGraph.Add(Number(status, "power_watts"), Number(status, "tdp_watts"));
            Show(status);
            RenderGraphs();
        }

        private void RenderGraphs()
        {
            var window = Window.Current;
            if (window != null && !window.Visible)
            {
                graphsStale = true;
                return;
            }
            graphsStale = false;
            powerGraph.Render();
        }

        /// <summary>Text only: also replayed by RestoreChecks, so it adds no graph sample.</summary>
        private void Show(JsonObject status)
        {
            lastStatus = status;
            ShowMessage(null);
            ApplyAccent(Text(status, "accent"));

            // Header: foreground game and the profile applied to it.
            var game = Text(status, "foreground");
            GameText.Text = game != null ? GameName(game) : S("NoGame");
            GameText.Opacity = game != null ? 1 : 0.6;
            var profile = Text(status, "active_profile");
            ProfileChipBorder.Visibility = profile != null ? Visibility.Visible : Visibility.Collapsed;
            if (profile != null)
            {
                ProfileText.Text = game != null && string.Equals(profile, game, StringComparison.OrdinalIgnoreCase)
                    ? S("ProfileChip")
                    : F("ProfileNamed", GameName(profile));
            }

            // Live power draw and frame rate.
            var mode = Text(status, "performance_mode");
            var tdp = Integer(status, "tdp_watts");
            var power = Number(status, "power_watts");
            PowerValue.Text = power.HasValue ? Watts(Math.Max(power.Value, 0)) : Dash;
            PowerUnit.Text = power.HasValue ? " " + S("UnitWatts") : "";
            var fps = Integer(status, "fps");
            FpsValue.Text = fps.HasValue ? fps.Value.ToString(CultureInfo.CurrentCulture) : Dash;
            var temp = Integer(status, "cpu_temp_c");
            var throttled = Flag(status, "thermal_limited") == true;
            TempValue.Text = temp.HasValue ? temp.Value.ToString(CultureInfo.CurrentCulture) : Dash;
            // Held down by the temperature limit: the caption says so.
            TempCaption.Text = throttled ? S("CaptionThrottled") : S("TempCaption/Text");
            Warn(TempCaption, throttled);

            // Bench run (driven from the shell).
            var bench = Child(status, "bench");
            BenchCard.Visibility = bench != null ? Visibility.Visible : Visibility.Collapsed;
            if (bench != null)
            {
                var step = Integer(bench, "step") ?? 0;
                var steps = Math.Max(Integer(bench, "steps") ?? 1, 1);
                var left = Math.Max(Integer(bench, "seconds_left") ?? 0, 0);
                // Step 0: the bench waits for the game to come back to the front.
                BenchStepText.Text = step == 0
                    ? S("BenchWaiting")
                    : F("BenchStep", step, steps, F("FormatWatts", Integer(bench, "watts") ?? 0));
                BenchLeftText.Text = F("BenchLeft", string.Format(CultureInfo.InvariantCulture, "{0}:{1:00}", left / 60, left % 60));
                // Completed steps: the current one counts once it is over.
                BenchBar.Maximum = steps;
                BenchBar.Value = Math.Min(Math.Max(step - 1, 0), steps);
            }

            // Modes: auto TDP, or the firmware mode in use.
            var auto = Child(status, "auto_tdp");
            var autoOn = Flag(auto, "enabled") == true;
            foreach (var button in new[] { SilentButton, PerformanceButton, TurboButton })
            {
                button.IsEnabled = true;
                button.IsChecked = !autoOn && mode == (string)button.Tag;
            }
            AutoButton.IsEnabled = auto != null;
            AutoButton.IsChecked = autoOn;
            ShowModeIcons();

            // Auto TDP steps its target; otherwise ±1 W, only from a known
            // wattage: in a firmware mode the service reports no TDP.
            if (autoOn)
            {
                var target = Integer(auto, "target_fps") ?? 0;
                StepLabel.Text = S("TargetLabel");
                Bump(StepValue, F("FormatFps", target));
                ShowHint(StepHint, "");
                StepDown.IsEnabled = target > TargetStops[0];
                StepUp.IsEnabled = target < TargetStops[TargetStops.Length - 1];
            }
            else
            {
                StepLabel.Text = S("PowerLimitLabel");
                Bump(StepValue, tdp.HasValue ? F("FormatWatts", tdp.Value) : Dash);
                ShowHint(StepHint, !tdp.HasValue && mode != null && mode != "manual" ? S("TdpHintFirmware") : "");
                StepDown.IsEnabled = tdp.HasValue && tdp.Value > 1;
                StepUp.IsEnabled = tdp.HasValue;
            }

            // Power bar: the auto TDP range, or the manual limit.
            var onBattery = Flag(status, "on_battery");
            rangeFloor = Integer(status, "tdp_min_watts") ?? DefaultFloorWatts;
            rangeCeiling = Integer(status, "tdp_max_watts") ?? DefaultCeilingWatts;
            var min = Integer(auto, "min_watts");
            var max = Integer(auto, "max_watts");
            var batteryMax = Integer(auto, "battery_max_watts");
            if (autoOn && min.HasValue && max.HasValue)
            {
                var high = onBattery == true && batteryMax.HasValue && batteryMax.Value < max.Value ? batteryMax.Value : max.Value;
                rangeLow = min.Value;
                rangeHigh = high;
                RangeText.Text = high < max.Value
                    ? F("AutoRangeBattery", F("FormatWatts", min.Value + " – " + high))
                    : F("FormatWatts", min.Value + " – " + max.Value);
            }
            else if (!autoOn && tdp.HasValue)
            {
                rangeLow = double.NaN;
                rangeHigh = tdp.Value;
                RangeText.Text = F("FormatWatts", tdp.Value);
            }
            else
            {
                rangeLow = double.NaN;
                rangeHigh = double.NaN;
                RangeText.Text = "";
            }
            livePower = power.HasValue ? Math.Max(power.Value, 0) : double.NaN;
            ShowRange();

            ShowHardware(status);
            ShowSaveButtons(game != null);

            var conflicts = status.GetNamedArray("conflicts", new JsonArray())
                .Where(v => v.ValueType == JsonValueType.String)
                .Select(v => v.GetString())
                .ToArray();
            ConflictText.Visibility = conflicts.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            ConflictText.Text = conflicts.Length > 0 ? F(conflicts.Length == 1 ? "ConflictOne" : "Conflicts", string.Join(", ", conflicts)) : "";
        }

        /// <summary>Fans, frame rate cap and controller, as the service has them.</summary>
        private void ShowHardware(JsonObject status)
        {
            // No custom curve: the firmware's own (Auto). Custom curves that
            // are no preset light no tile.
            var curves = Child(status, "fan_curves");
            var firmware = curves == null || new[] { "cpu", "gpu", "mid" }.All(fan =>
                curves.GetNamedValue(fan, JsonValue.CreateNullValue()).ValueType == JsonValueType.Null);
            var preset = Text(status, "fan_preset");
            foreach (var button in new[] { FanAutoButton, FanBalancedButton, FanMaxButton })
            {
                var tag = (string)button.Tag;
                button.IsEnabled = true;
                button.IsChecked = tag == "" ? firmware : !firmware && preset == tag;
            }
            FansHint.Text = !firmware && preset == null ? S("FansCustom") : "";

            var limit = Integer(status, "fps_limit") ?? 0;
            Bump(LimitValue, limit > 0 ? F("FormatFps", limit) : S("FpsLimitNone"));
            LimitDown.IsEnabled = limit > 0;
            LimitUp.IsEnabled = limit < LimitStops[LimitStops.Length - 1];

            var controller = Text(status, "controller_mode");
            foreach (var button in new[] { GamepadButton, KeyboardButton, MouseButton })
            {
                button.IsEnabled = true;
                button.IsChecked = controller == (string)button.Tag;
            }
            ShowModeIcons();
        }

        /// <summary>The save button: only with a game in front.</summary>
        private void ShowSaveButtons(bool game)
        {
            SaveButton.IsEnabled = game;
            if (!saved)
            {
                SaveButton.Content = S("SaveForGame");
            }
        }

        /// <summary>
        /// A ToggleButton flips itself on click; the service's next status is the
        /// truth, so put every check mark back to the last known state.
        /// (Without a status every button is disabled, so there is no click.)
        /// </summary>
        private void RestoreChecks()
        {
            if (lastStatus != null)
            {
                Show(lastStatus);
            }
        }

        // ---- Commands -----------------------------------------------------

        private async void Send(JsonObject request)
        {
            await SendAsync(request);
        }

        /// <summary>True when the service accepted the request.</summary>
        private async Task<bool> SendAsync(JsonObject request)
        {
            var problem = await link.SendAsync(request);
            ShowMessage(problem == null ? null : Describe(problem));
            return problem == null;
        }

        private static JsonObject Request(string type)
        {
            return new JsonObject { ["type"] = JsonValue.CreateStringValue(type) };
        }

        private void OnMode(object sender, RoutedEventArgs e)
        {
            RestoreChecks();
            var request = Request("set_performance_mode");
            request["mode"] = JsonValue.CreateStringValue((string)((FrameworkElement)sender).Tag);
            Send(request);
        }

        /// <summary>‹ or ›: the next target stop under auto TDP, else ±1 W.</summary>
        private void OnStep(object sender, RoutedEventArgs e)
        {
            var delta = int.Parse((string)((FrameworkElement)sender).Tag, CultureInfo.InvariantCulture);
            var auto = AutoTdp();
            if (Flag(auto, "enabled") == true)
            {
                var target = Integer(auto, "target_fps") ?? 60;
                var next = delta > 0
                    ? TargetStops.Where(t => t > target).DefaultIfEmpty(TargetStops[TargetStops.Length - 1]).First()
                    : TargetStops.Where(t => t < target).DefaultIfEmpty(TargetStops[0]).Last();
                SendAutoTdp(true, next);
                return;
            }
            var current = Integer(lastStatus, "tdp_watts");
            if (!current.HasValue)
            {
                // Firmware mode: no wattage to step from (buttons are disabled anyway).
                return;
            }
            var watts = current.Value + delta;
            // The model's range, when the service reports it.
            var min = Integer(lastStatus, "tdp_min_watts") ?? 1;
            var max = Integer(lastStatus, "tdp_max_watts") ?? int.MaxValue;
            if (watts < min || watts > max)
            {
                return;
            }
            var request = Request("set_tdp");
            request["watts"] = JsonValue.CreateNumberValue(watts);
            Send(request);
        }

        /// <summary>
        /// Sends the whole current auto TDP object with only enabled/target_fps
        /// changed, so fields the widget doesn't edit (battery_max_watts,
        /// max_temp_c, future ones) are kept as they are.
        /// </summary>
        private void SendAutoTdp(bool enabled, double? target)
        {
            var current = AutoTdp();
            JsonObject settings;
            if (current != null)
            {
                settings = JsonObject.Parse(current.Stringify());
            }
            else
            {
                // Same defaults as AutoTdpSettings::default().
                settings = new JsonObject
                {
                    ["target_fps"] = JsonValue.CreateNumberValue(60),
                    ["min_watts"] = JsonValue.CreateNumberValue(7),
                    ["max_watts"] = JsonValue.CreateNumberValue(25),
                };
            }
            settings["enabled"] = JsonValue.CreateBooleanValue(enabled);
            if (target.HasValue)
            {
                settings["target_fps"] = JsonValue.CreateNumberValue(target.Value);
            }
            var request = Request("set_auto_tdp");
            request["settings"] = settings;
            Send(request);
        }

        private void OnFan(object sender, RoutedEventArgs e)
        {
            RestoreChecks();
            var tag = (string)((FrameworkElement)sender).Tag;
            var request = Request("set_fan_preset");
            request["preset"] = tag == "" ? JsonValue.CreateNullValue() : JsonValue.CreateStringValue(tag);
            Send(request);
        }

        /// <summary>‹ or ›: the next stop of the frame rate cap.</summary>
        private void OnLimitStep(object sender, RoutedEventArgs e)
        {
            var delta = int.Parse((string)((FrameworkElement)sender).Tag, CultureInfo.InvariantCulture);
            var limit = Integer(lastStatus, "fps_limit") ?? 0;
            var next = delta > 0
                ? LimitStops.Where(t => t > limit).DefaultIfEmpty(LimitStops[LimitStops.Length - 1]).First()
                : LimitStops.Where(t => t < limit).DefaultIfEmpty(0).Last();
            var request = Request("set_fps_limit");
            request["fps"] = next > 0 ? JsonValue.CreateNumberValue(next) : JsonValue.CreateNullValue();
            Send(request);
        }

        private void OnController(object sender, RoutedEventArgs e)
        {
            RestoreChecks();
            var request = Request("set_controller_mode");
            request["mode"] = JsonValue.CreateStringValue((string)((FrameworkElement)sender).Tag);
            Send(request);
        }

        /// <summary>Saves the tab in front into the profile of the game in front.</summary>
        private async void OnSave(object sender, RoutedEventArgs e)
        {
            var request = Request("save_foreground_profile");
            request["part"] = JsonValue.CreateStringValue(tab);
            if (!await SendAsync(request))
            {
                return;
            }
            saved = true;
            SaveButton.Content = S("Saved");
            if (savedTimer == null)
            {
                savedTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(2) };
                savedTimer.Tick += (s, args) =>
                {
                    savedTimer.Stop();
                    saved = false;
                    ShowSaveButtons(Text(lastStatus, "foreground") != null);
                };
            }
            savedTimer.Stop();
            savedTimer.Start();
        }

        /// <summary>
        /// Opens the shell through Game Bar's own launcher: Windows' Launcher
        /// does nothing from inside Game Bar.
        /// </summary>
        private async void OnOpen(object sender, RoutedEventArgs e)
        {
            bool opened;
            try
            {
                opened = widget != null
                    ? await widget.LaunchUriAsync(ShellUri)
                    : await Launcher.LaunchUriAsync(ShellUri);
            }
            catch (Exception)
            {
                opened = false;
            }
            ShowMessage(opened ? null : S("OpenFailed"));
        }

        private void OnAuto(object sender, RoutedEventArgs e)
        {
            var enabled = Flag(AutoTdp(), "enabled") == true;
            RestoreChecks();
            if (!enabled)
            {
                SendAutoTdp(true, null);
            }
        }
    }
}
