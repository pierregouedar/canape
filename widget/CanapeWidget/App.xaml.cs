using System;
using Microsoft.Gaming.XboxGameBar;
using Windows.ApplicationModel;
using Windows.ApplicationModel.Activation;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;

namespace CanapeWidget
{
    /// <summary>
    /// Game Bar activates the widget through the ms-gamebarwidget protocol.
    /// Activation pattern from Microsoft's WidgetSampleCS.
    /// </summary>
    sealed partial class App : Application
    {
        // Must be kept alive for as long as the widget window lives.
        private XboxGameBarWidget widget;

        public App()
        {
            InitializeComponent();
            Suspending += OnSuspending;
        }

        protected override void OnActivated(IActivatedEventArgs args)
        {
            if (args.Kind != ActivationKind.Protocol)
            {
                return;
            }
            var protocolArgs = (IProtocolActivatedEventArgs)args;
            if (!protocolArgs.Uri.Scheme.Equals("ms-gamebarwidget"))
            {
                return;
            }
            var widgetArgs = args as XboxGameBarWidgetActivatedEventArgs;
            // Later activations only carry a URI: keep the existing widget.
            if (widgetArgs == null || !widgetArgs.IsLaunchActivation)
            {
                return;
            }

            var rootFrame = new Frame();
            Window.Current.Content = rootFrame;
            widget = new XboxGameBarWidget(widgetArgs, Window.Current.CoreWindow, rootFrame);
            rootFrame.Navigate(typeof(WidgetPage), widget);
            Window.Current.Closed += OnWidgetClosed;
            Window.Current.Activate();
        }

        private void OnWidgetClosed(object sender, Windows.UI.Core.CoreWindowEventArgs e)
        {
            widget = null;
            Window.Current.Closed -= OnWidgetClosed;
        }

        // Game Bar never suspends an app with a live widget, so cleanup is safe here.
        private void OnSuspending(object sender, SuspendingEventArgs e)
        {
            var deferral = e.SuspendingOperation.GetDeferral();
            widget = null;
            deferral.Complete();
        }
    }
}
