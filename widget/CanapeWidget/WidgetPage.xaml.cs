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
    /// <summary>Quick settings in Game Bar, mirroring the shell's home screen.</summary>
    public sealed partial class WidgetPage : Page
    {
        private const string Dash = "\u2014";
        // ERROR_FILE_NOT_FOUND: no pipe server, i.e. the service isn't running.
        private const string ErrorFileNotFound = "2";

        // The service pushes a status once per second: 60 samples = the last minute.
        private const int HistoryLength = 60;

        private readonly ResourceLoader strings = ResourceLoader.GetForCurrentView();
        private readonly CanapeLink link = new CanapeLink();
        private readonly LiveGraph powerGraph;
        private readonly LiveGraph gpuGraph;
        private CancellationTokenSource follow;
        private JsonObject lastStatus;
        private XboxGameBarWidget widget;
        private Color? appliedAccent;
        // Graphs are not redrawn while Game Bar hides the widget.
        private bool graphsStale;

        public WidgetPage()
        {
            InitializeComponent();
            powerGraph = new LiveGraph(PowerPlot, PowerLine, PowerArea, PowerLimitLine, HistoryLength) { MinimumScale = 10 };
            gpuGraph = new LiveGraph(GpuPlot, GpuLine, GpuArea, null, HistoryLength) { FixedMax = 1 };
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
        /// the default blue): every element using them follows at once.
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

            var onAccent = Palette.OnAccent(accent);
            SetBrush(Resources, "AccentBrush", accent);
            SetBrush(Resources, "AccentFillBrush", Palette.WithAlpha(accent, 0x38));
            SetBrush(Resources, "AccentTextBrush", Palette.TextOnPanel(accent));

            object value;
            var theme = Resources.ThemeDictionaries.TryGetValue("Default", out value) ? value as ResourceDictionary : null;
            SetBrush(theme, "ToggleButtonBackgroundChecked", accent);
            SetBrush(theme, "ToggleButtonBackgroundCheckedPointerOver", Palette.Mix(accent, Palette.White, 0.12));
            SetBrush(theme, "ToggleButtonBackgroundCheckedPressed", Palette.Mix(accent, Palette.Ink, 0.15));
            SetBrush(theme, "ToggleButtonBackgroundCheckedDisabled", Palette.WithAlpha(accent, 0x59));
            SetBrush(theme, "ToggleButtonForegroundChecked", onAccent);
            SetBrush(theme, "ToggleButtonForegroundCheckedPointerOver", onAccent);
            SetBrush(theme, "ToggleButtonForegroundCheckedPressed", Palette.WithAlpha(onAccent, 0xE6));
            SetBrush(theme, "ToggleButtonForegroundCheckedDisabled", Palette.WithAlpha(onAccent, 0x99));
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
            PowerText.Text = Dash;
            PowerLimitText.Text = "";
            GpuText.Text = Dash;
            powerGraph.Clear();
            gpuGraph.Clear();
            TdpText.Text = Dash;
            FpsText.Text = Dash;
            TempText.Text = Dash;
            BatteryText.Text = Dash;
            BatteryGlyph.Visibility = Visibility.Collapsed;
            TempCaption.Text = S("CaptionTemp");
            BatteryCaption.Text = S("CaptionBattery");
            Warn(TempCaption, false);
            Warn(TempText, false);
            BenchCard.Visibility = Visibility.Collapsed;
            ConflictText.Visibility = Visibility.Collapsed;
            ShowHint(TdpHint, "");
            AutoRangeText.Text = "";
            AutoText.Text = S("AutoOff");
            foreach (var button in new[] { SilentButton, PerformanceButton, TurboButton, AutoButton, Fps30Button, Fps40Button, Fps60Button })
            {
                button.IsChecked = false;
                button.IsEnabled = false;
            }
            TdpMinusButton.IsEnabled = false;
            TdpPlusButton.IsEnabled = false;
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

        /// <summary>Warning text colour, or back to the style's look (captions are dimmed).</summary>
        private void Warn(TextBlock text, bool warning)
        {
            if (warning)
            {
                text.Foreground = (Brush)Resources["WarningBrush"];
                text.Opacity = 1;
            }
            else
            {
                text.ClearValue(TextBlock.ForegroundProperty);
                text.ClearValue(OpacityProperty);
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
            var gpu = Number(status, "gpu_util");
            powerGraph.Add(Number(status, "power_watts"), Number(status, "tdp_watts"));
            gpuGraph.Add(gpu.HasValue ? Math.Min(Math.Max(gpu.Value, 0), 1) : (double?)null, null);
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
            gpuGraph.Render();
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

            // Live power against the limit, GPU activity.
            var mode = Text(status, "performance_mode");
            var tdp = Integer(status, "tdp_watts");
            var power = Number(status, "power_watts");
            PowerText.Text = power.HasValue ? F("FormatWatts", (int)Math.Round(Math.Max(power.Value, 0))) : Dash;
            PowerLimitText.Text = tdp.HasValue ? F("PowerLimit", F("FormatWatts", tdp.Value)) : "";
            var gpu = Number(status, "gpu_util");
            GpuText.Text = gpu.HasValue ? F("FormatPercent", (int)Math.Round(Math.Min(Math.Max(gpu.Value, 0), 1) * 100)) : Dash;

            var fps = Integer(status, "fps");
            FpsText.Text = fps.HasValue ? fps.Value.ToString(CultureInfo.CurrentCulture) : Dash;

            var temp = Integer(status, "cpu_temp_c");
            TempText.Text = temp.HasValue ? F("FormatCelsius", temp.Value) : Dash;
            var throttled = Flag(status, "thermal_limited") == true;
            TempCaption.Text = S(throttled ? "CaptionThrottled" : "CaptionTemp");
            Warn(TempCaption, throttled);
            Warn(TempText, throttled);

            var battery = Integer(status, "battery_percent");
            var onBattery = Flag(status, "on_battery");
            BatteryText.Text = battery.HasValue ? F("FormatPercent", battery.Value) : Dash;
            BatteryCaption.Text = S(onBattery == false ? "CaptionPluggedIn" : "CaptionBattery");
            BatteryGlyph.Visibility = battery.HasValue ? Visibility.Visible : Visibility.Collapsed;
            if (battery.HasValue)
            {
                BatteryGlyph.Text = BatteryGlyphFor(battery.Value, onBattery == false);
            }

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

            // Modes.
            foreach (var button in new[] { SilentButton, PerformanceButton, TurboButton })
            {
                button.IsEnabled = true;
                button.IsChecked = mode == (string)button.Tag;
            }

            // ±1 W only from a known wattage: in a firmware mode (silent,
            // performance, turbo) the service reports no TDP.
            TdpText.Text = tdp.HasValue ? F("FormatWatts", tdp.Value) : Dash;
            TdpMinusButton.IsEnabled = tdp.HasValue && tdp.Value > 1;
            TdpPlusButton.IsEnabled = tdp.HasValue;
            ShowHint(TdpHint, !tdp.HasValue && mode != null && mode != "manual" ? S("TdpHintFirmware") : "");

            // Auto TDP.
            var auto = Child(status, "auto_tdp");
            var enabled = Flag(auto, "enabled") == true;
            var target = Integer(auto, "target_fps") ?? 0;
            AutoText.Text = S(enabled ? "AutoOn" : "AutoOff");
            AutoButton.IsChecked = enabled;
            foreach (var button in new[] { Fps30Button, Fps40Button, Fps60Button })
            {
                button.IsChecked = enabled && target == int.Parse((string)button.Tag, CultureInfo.InvariantCulture);
            }
            foreach (var button in new[] { AutoButton, Fps30Button, Fps40Button, Fps60Button })
            {
                button.IsEnabled = auto != null;
            }
            var min = Integer(auto, "min_watts");
            var max = Integer(auto, "max_watts");
            var batteryMax = Integer(auto, "battery_max_watts");
            if (min.HasValue && max.HasValue)
            {
                AutoRangeText.Text = onBattery == true && batteryMax.HasValue && batteryMax.Value < max.Value
                    ? F("AutoRangeBattery", F("FormatWatts", min.Value + "–" + batteryMax.Value))
                    : F("FormatWatts", min.Value + "–" + max.Value);
            }
            else
            {
                AutoRangeText.Text = "";
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

        private void OnTdp(object sender, RoutedEventArgs e)
        {
            var current = Integer(lastStatus, "tdp_watts");
            if (!current.HasValue)
            {
                // Firmware mode: no wattage to step from (buttons are disabled anyway).
                return;
            }
            var delta = int.Parse((string)((FrameworkElement)sender).Tag, CultureInfo.InvariantCulture);
            var watts = current.Value + delta;
            if (watts < 1)
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

        private void OnAutoToggle(object sender, RoutedEventArgs e)
        {
            var enabled = Flag(AutoTdp(), "enabled") == true;
            RestoreChecks();
            SendAutoTdp(!enabled, null);
        }

        private void OnTarget(object sender, RoutedEventArgs e)
        {
            RestoreChecks();
            SendAutoTdp(true, double.Parse((string)((FrameworkElement)sender).Tag, CultureInfo.InvariantCulture));
        }
    }
}
