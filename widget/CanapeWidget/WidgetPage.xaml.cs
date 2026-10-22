using System;
using System.Globalization;
using System.Linq;
using System.Threading;
using Microsoft.Gaming.XboxGameBar;
using Windows.ApplicationModel.Resources;
using Windows.Data.Json;
using Windows.UI;
using Windows.UI.Core;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Media;
using Windows.UI.Xaml.Navigation;

namespace CanapeWidget
{
    /// <summary>Quick settings in Game Bar, mirroring the shell's Performance page.</summary>
    public sealed partial class WidgetPage : Page
    {
        private const string Dash = "\u2014";
        // ERROR_FILE_NOT_FOUND: no pipe server, i.e. the service isn't running.
        private const string ErrorFileNotFound = "2";

        // The service pushes a status once per second: 60 samples = the last minute.
        private const int HistoryLength = 60;

        // Stops of the auto TDP target, as on the shell's Performance page.
        private static readonly int[] TargetStops = { 30, 40, 45, 60, 90, 120 };

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

        public WidgetPage()
        {
            InitializeComponent();
            powerGraph = new LiveGraph(PowerPlot, PowerLine, null, PowerLimitLine, HistoryLength) { MinimumScale = 10 };
            link.StatusReceived += status => OnUi(() => OnStatus(status));
            link.Disconnected += problem => OnUi(() => ShowDisconnected(problem));
            ApplyAccent(null);
            ShowWaiting();
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
            ProfileText.Visibility = Visibility.Collapsed;
            PowerValue.Text = Dash;
            PowerUnit.Text = "";
            FpsValue.Text = Dash;
            powerGraph.Clear();
            BenchCard.Visibility = Visibility.Collapsed;
            ConflictText.Visibility = Visibility.Collapsed;
            foreach (var button in new[] { SilentButton, PerformanceButton, TurboButton, AutoButton })
            {
                button.IsChecked = false;
                button.IsEnabled = false;
            }
            ShowModeIcons();
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
            FooterText.Text = "";
            Warn(FooterText, false);
            BatteryText.Text = "";
            BatteryGlyph.Visibility = Visibility.Collapsed;
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

        /// <summary>A chosen tile is white: its icon turns dark like its name.</summary>
        private void ShowModeIcons()
        {
            var ink = new SolidColorBrush(Palette.Ink);
            var soft = (Brush)Resources["SoftTextBrush"];
            SilentIcon.Stroke = SilentButton.IsChecked == true ? ink : soft;
            PerformanceIcon.Stroke = PerformanceButton.IsChecked == true ? ink : soft;
            TurboIcon.Stroke = TurboButton.IsChecked == true ? ink : soft;
            AutoIcon.Stroke = AutoButton.IsChecked == true ? ink : soft;
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

        /// <summary>
        /// Segoe MDL2 Assets battery glyphs, by tenths of charge: Battery0-9
        /// (E850-E859), Battery10 (E83F), BatteryCharging0-8 (E85A-E862),
        /// BatteryCharging9 (E83E, also used at 100 %).
        /// </summary>
        private static string BatteryGlyphFor(int percent, bool charging)
        {
            var level = Math.Min(Math.Max((int)Math.Round(percent / 10.0), 0), 10);
            int code;
            if (charging)
            {
                code = level >= 9 ? 0xE83E : 0xE85A + level;
            }
            else
            {
                code = level == 10 ? 0xE83F : 0xE850 + level;
            }
            return ((char)code).ToString();
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
            ProfileText.Visibility = profile != null ? Visibility.Visible : Visibility.Collapsed;
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

            // Bench run (driven from the shell).
            var bench = Child(status, "bench");
            BenchCard.Visibility = bench != null ? Visibility.Visible : Visibility.Collapsed;
            if (bench != null)
            {
                var step = Integer(bench, "step") ?? 0;
                var steps = Math.Max(Integer(bench, "steps") ?? 1, 1);
                var left = Math.Max(Integer(bench, "seconds_left") ?? 0, 0);
                BenchStepText.Text = F("BenchStep", step, steps, F("FormatWatts", Integer(bench, "watts") ?? 0));
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
                StepValue.Text = F("FormatFps", target);
                ShowHint(StepHint, "");
                StepDown.IsEnabled = target > TargetStops[0];
                StepUp.IsEnabled = target < TargetStops[TargetStops.Length - 1];
            }
            else
            {
                StepLabel.Text = S("PowerLimitLabel");
                StepValue.Text = tdp.HasValue ? F("FormatWatts", tdp.Value) : Dash;
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

            // Footer: temperature (or the thermal limit), GPU activity, battery.
            var temp = Integer(status, "cpu_temp_c");
            var throttled = Flag(status, "thermal_limited") == true;
            var gpu = Number(status, "gpu_util");
            var parts = new[]
            {
                throttled ? S("CaptionThrottled") : null,
                temp.HasValue ? F("FormatCelsius", temp.Value) : null,
                gpu.HasValue ? F("FooterGpu", F("FormatPercent", (int)Math.Round(Math.Min(Math.Max(gpu.Value, 0), 1) * 100))) : null,
            };
            FooterText.Text = string.Join(" · ", parts.Where(p => p != null));
            Warn(FooterText, throttled);
            var battery = Integer(status, "battery_percent");
            BatteryText.Text = battery.HasValue ? F("FormatPercent", battery.Value) : "";
            BatteryGlyph.Visibility = battery.HasValue ? Visibility.Visible : Visibility.Collapsed;
            if (battery.HasValue)
            {
                BatteryGlyph.Text = BatteryGlyphFor(battery.Value, onBattery == false);
            }

            var conflicts = status.GetNamedArray("conflicts", new JsonArray())
                .Where(v => v.ValueType == JsonValueType.String)
                .Select(v => v.GetString())
                .ToArray();
            ConflictText.Visibility = conflicts.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            ConflictText.Text = conflicts.Length > 0 ? F("Conflicts", string.Join(", ", conflicts)) : "";
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
            var problem = await link.SendAsync(request);
            ShowMessage(problem == null ? null : Describe(problem));
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
