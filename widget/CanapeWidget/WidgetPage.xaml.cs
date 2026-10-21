using System;
using System.Linq;
using System.Threading;
using Windows.Data.Json;
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
        private static readonly Brush Selected = new SolidColorBrush(Windows.UI.Color.FromArgb(255, 0x25, 0x63, 0xEB));

        private readonly CanapeLink link = new CanapeLink();
        private CancellationTokenSource follow;
        private JsonObject lastStatus;

        public WidgetPage()
        {
            InitializeComponent();
            link.StatusReceived += status => OnUi(() => Show(status));
            link.Disconnected += message => OnUi(() =>
            {
                lastStatus = null;
                MessageText.Text = "Service non connecté : " + message;
            });
        }

        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            follow = new CancellationTokenSource();
            _ = link.FollowAsync(follow.Token);
        }

        protected override void OnNavigatedFrom(NavigationEventArgs e)
        {
            follow?.Cancel();
        }

        private void OnUi(Action action)
        {
            _ = Dispatcher.RunAsync(CoreDispatcherPriority.Normal, () => action());
        }

        private static double? Number(JsonObject json, string name)
        {
            var value = json.GetNamedValue(name, JsonValue.CreateNullValue());
            return value.ValueType == JsonValueType.Number ? value.GetNumber() : (double?)null;
        }

        private static string Text(JsonObject json, string name)
        {
            var value = json.GetNamedValue(name, JsonValue.CreateNullValue());
            return value.ValueType == JsonValueType.String ? value.GetString() : null;
        }

        private JsonObject AutoTdp()
        {
            return lastStatus?.GetNamedObject("auto_tdp", null);
        }

        private void Show(JsonObject status)
        {
            lastStatus = status;
            MessageText.Text = "";

            var mode = Text(status, "performance_mode");
            var tdp = Number(status, "tdp_watts");
            TdpText.Text = tdp.HasValue ? tdp + " W" : (mode != null && mode != "manual" ? "Auto" : "—");
            var fps = Number(status, "fps");
            FpsText.Text = fps.HasValue ? Math.Round(fps.Value).ToString() : "—";
            var temp = Number(status, "cpu_temp_c");
            TempText.Text = temp.HasValue ? Math.Round(temp.Value) + " °C" : "—";
            var battery = Number(status, "battery_percent");
            BatteryText.Text = battery.HasValue ? battery + " %" : "—";

            Highlight(SilentButton, mode == "silent");
            Highlight(PerformanceButton, mode == "performance");
            Highlight(TurboButton, mode == "turbo");

            var auto = AutoTdp();
            var enabled = auto != null && auto.GetNamedBoolean("enabled", false);
            var target = auto == null ? 0 : (int)auto.GetNamedNumber("target_fps", 0);
            AutoButton.Content = enabled ? "On" : "Off";
            Highlight(AutoButton, enabled);
            Highlight(Fps30Button, enabled && target == 30);
            Highlight(Fps40Button, enabled && target == 40);
            Highlight(Fps60Button, enabled && target == 60);

            var conflicts = status.GetNamedArray("conflicts", new JsonArray())
                .Select(v => v.GetString())
                .ToArray();
            ConflictText.Visibility = conflicts.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            ConflictText.Text = "⚠ Pilote aussi le matériel : " + string.Join(", ", conflicts);
        }

        private static void Highlight(Button button, bool on)
        {
            if (on)
            {
                button.Background = Selected;
            }
            else
            {
                button.ClearValue(BackgroundProperty);
            }
        }

        private async void Send(JsonObject request)
        {
            var error = await link.SendAsync(request);
            MessageText.Text = error ?? "";
        }

        private static JsonObject Request(string type)
        {
            return new JsonObject { ["type"] = JsonValue.CreateStringValue(type) };
        }

        private void OnMode(object sender, RoutedEventArgs e)
        {
            var request = Request("set_performance_mode");
            request["mode"] = JsonValue.CreateStringValue((string)((Button)sender).Tag);
            Send(request);
        }

        private void OnTdp(object sender, RoutedEventArgs e)
        {
            var delta = int.Parse((string)((Button)sender).Tag);
            var current = lastStatus == null ? null : Number(lastStatus, "tdp_watts");
            var request = Request("set_tdp");
            request["watts"] = JsonValue.CreateNumberValue((current ?? 15) + delta);
            Send(request);
        }

        private void SendAutoTdp(bool enabled, double? target)
        {
            var current = AutoTdp();
            var settings = new JsonObject
            {
                ["enabled"] = JsonValue.CreateBooleanValue(enabled),
                ["target_fps"] = JsonValue.CreateNumberValue(target ?? current?.GetNamedNumber("target_fps", 60) ?? 60),
                ["min_watts"] = JsonValue.CreateNumberValue(current?.GetNamedNumber("min_watts", 7) ?? 7),
                ["max_watts"] = JsonValue.CreateNumberValue(current?.GetNamedNumber("max_watts", 25) ?? 25),
            };
            var request = Request("set_auto_tdp");
            request["settings"] = settings;
            Send(request);
        }

        private void OnAutoToggle(object sender, RoutedEventArgs e)
        {
            var enabled = AutoTdp()?.GetNamedBoolean("enabled", false) ?? false;
            SendAutoTdp(!enabled, null);
        }

        private void OnTarget(object sender, RoutedEventArgs e)
        {
            SendAutoTdp(true, double.Parse((string)((Button)sender).Tag));
        }
    }
}
