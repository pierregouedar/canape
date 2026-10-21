using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;
using Windows.Data.Json;

namespace CanapeWidget
{
    /// <summary>
    /// Client of the Canape service: JSON lines over \\.\pipe\canape
    /// (crates/canape-core/src/protocol.rs). A subscription connection only
    /// receives status pushes; each command uses a short-lived connection.
    /// </summary>
    sealed class CanapeLink
    {
        private const string PipeName = @"\\.\pipe\canape";
        private const int ProtocolVersion = 1;
        private const uint GenericRead = 0x80000000;
        private const uint GenericWrite = 0x40000000;
        private const uint OpenExisting = 3;

        // Allowed from an AppContainer since Windows 10 2004
        // (XboxGameBarSamples issue #44).
        [DllImport("api-ms-win-core-file-fromapp-l1-1-0.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafeFileHandle CreateFileFromAppW(
            string fileName, uint desiredAccess, uint shareMode, IntPtr securityAttributes,
            uint creationDisposition, uint flagsAndAttributes, IntPtr templateFile);

        public event Action<JsonObject> StatusReceived;
        public event Action<string> Disconnected;

        private sealed class Connection : IDisposable
        {
            private readonly Stream stream;
            private readonly StreamReader reader;
            private readonly StreamWriter writer;

            public Connection()
            {
                var handle = CreateFileFromAppW(PipeName, GenericRead | GenericWrite, 0, IntPtr.Zero, OpenExisting, 0, IntPtr.Zero);
                if (handle.IsInvalid)
                {
                    throw new IOException("service injoignable (erreur " + Marshal.GetLastWin32Error() + ")");
                }
                stream = new FileStream(handle, FileAccess.ReadWrite, 1);
                reader = new StreamReader(stream, new UTF8Encoding(false));
                writer = new StreamWriter(stream, new UTF8Encoding(false)) { AutoFlush = true, NewLine = "\n" };

                var hello = new JsonObject
                {
                    ["type"] = JsonValue.CreateStringValue("hello"),
                    ["protocol_version"] = JsonValue.CreateNumberValue(ProtocolVersion),
                };
                Send(hello);
                Receive();
            }

            public void Send(JsonObject message)
            {
                writer.WriteLine(message.Stringify());
            }

            /// <summary>Next message, or null once the service hung up.</summary>
            public JsonObject Receive()
            {
                var line = reader.ReadLine();
                return line == null ? null : JsonObject.Parse(line);
            }

            public void Dispose()
            {
                reader.Dispose();
                writer.Dispose();
                stream.Dispose();
            }
        }

        /// <summary>Sends one request; returns the error message, or null on success.</summary>
        public Task<string> SendAsync(JsonObject request)
        {
            return Task.Run(() =>
            {
                try
                {
                    using (var connection = new Connection())
                    {
                        connection.Send(request);
                        var response = connection.Receive();
                        if (response != null && response.GetNamedString("type", "") == "error")
                        {
                            return response.GetNamedString("message", "erreur");
                        }
                        return null;
                    }
                }
                catch (Exception e)
                {
                    return e.Message;
                }
            });
        }

        /// <summary>Follows status pushes until cancelled, reconnecting every 2 s.</summary>
        public Task FollowAsync(CancellationToken cancel)
        {
            return Task.Run(async () =>
            {
                while (!cancel.IsCancellationRequested)
                {
                    try
                    {
                        using (var connection = new Connection())
                        using (cancel.Register(connection.Dispose))
                        {
                            connection.Send(new JsonObject { ["type"] = JsonValue.CreateStringValue("subscribe") });
                            JsonObject message;
                            while ((message = connection.Receive()) != null)
                            {
                                if (message.GetNamedString("type", "") == "status")
                                {
                                    StatusReceived?.Invoke(message);
                                }
                            }
                        }
                        Disconnected?.Invoke("service arrêté");
                    }
                    catch (Exception e)
                    {
                        if (!cancel.IsCancellationRequested)
                        {
                            Disconnected?.Invoke(e.Message);
                        }
                    }
                    try
                    {
                        await Task.Delay(2000, cancel);
                    }
                    catch (TaskCanceledException)
                    {
                    }
                }
            });
        }
    }
}
