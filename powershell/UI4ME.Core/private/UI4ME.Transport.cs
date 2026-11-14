// Runtime-compiled helper for the UI4ME.Core PowerShell module. No compiled
// assembly is shipped; Add-Type builds this source once per process.
//
// The source must compile as C# 5 against .NET Framework 4.x (Windows
// PowerShell 5.1, CodeDOM csc) and with Roslyn against modern .NET
// (PowerShell 7.2+). UI4ME_NETFX is defined by the module on .NET Framework.
//
// Threading contract: the reader thread only parses frames and queues them.
// It never invokes PowerShell. All application callbacks run when the
// PowerShell runspace pumps Receive().
using System;
using System.Collections;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.IO.MemoryMappedFiles;
using Microsoft.Win32.SafeHandles;
using System.IO.Pipes;
using System.Management.Automation;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;

namespace UI4ME {
    /// <summary>A malformed or unexpected peer message. Fatal to the connection.</summary>
    public sealed class ProtocolException : Exception {
        public ProtocolException(string message) : base(message) { }
    }

    /// <summary>The host answered a request with a JSON-RPC error.</summary>
    public sealed class RpcException : Exception {
        public RpcException(string method, long code, string message, object data)
            : base("UI4ME " + method + " failed (" + code.ToString(CultureInfo.InvariantCulture) + "): " + message) {
            Method = method; Code = code; RpcMessage = message; RpcData = data;
            Data["RpcCode"] = code;
            Data["RpcData"] = data;
        }
        public string Method { get; private set; }
        public long Code { get; private set; }
        public string RpcMessage { get; private set; }
        public object RpcData { get; private set; }
    }
    /// <summary>The host connection is closed; no further requests can succeed.</summary>
    public sealed class DisconnectedException : IOException {
        public DisconnectedException(string message) : base(message) { }
        public DisconnectedException(string message, Exception inner) : base(message, inner) { }
    }
    /// <summary>A wait expired. The host may still complete the operation.</summary>
    public sealed class RpcTimeoutException : TimeoutException {
        public RpcTimeoutException(string message) : base(message) { }
    }
    /// <summary>A window, document, node or bitmap handle is no longer current.</summary>
    public sealed class StaleObjectException : InvalidOperationException {
        public StaleObjectException(string message) : base(message) { }
    }

    /// <summary>
    /// Strict JSON for the host protocol. Objects become PSObjects with note
    /// properties (no date coercion, no PowerShell 5.1 array wrapping quirks).
    /// </summary>
    public static class Json {
        private const int MaximumDepth = 128;

        public static object Parse(string text) {
            if (text == null) throw new ArgumentNullException("text");
            Parser parser = new Parser(text);
            parser.SkipWhitespace();
            object value = parser.ReadValue(0);
            parser.SkipWhitespace();
            if (parser.Position != text.Length) throw new ProtocolException("Trailing data after JSON value");
            return value;
        }

        private sealed class Parser {
            private readonly string text;
            public int Position;
            public Parser(string text) { this.text = text; }

            public void SkipWhitespace() {
                while (Position < text.Length) {
                    char c = text[Position];
                    if (c == ' ' || c == '\t' || c == '\r' || c == '\n') Position++;
                    else break;
                }
            }
            private char Peek() {
                if (Position >= text.Length) throw new ProtocolException("Unexpected end of JSON");
                return text[Position];
            }
            private void Expect(string literal) {
                if (string.CompareOrdinal(text, Position, literal, 0, literal.Length) != 0)
                    throw new ProtocolException("Invalid JSON literal");
                Position += literal.Length;
            }
            public object ReadValue(int depth) {
                if (depth > MaximumDepth) throw new ProtocolException("JSON nesting is too deep");
                char c = Peek();
                switch (c) {
                    case '{': return ReadObject(depth);
                    case '[': return ReadArray(depth);
                    case '"': return ReadString();
                    case 't': Expect("true"); return true;
                    case 'f': Expect("false"); return false;
                    case 'n': Expect("null"); return null;
                    default:
                        if (c == '-' || (c >= '0' && c <= '9')) return ReadNumber();
                        throw new ProtocolException("Invalid JSON value");
                }
            }
            private object ReadObject(int depth) {
                Position++;
                List<KeyValuePair<string, object>> members = new List<KeyValuePair<string, object>>();
                HashSet<string> exact = new HashSet<string>(StringComparer.Ordinal);
                HashSet<string> folded = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                bool caseCollision = false;
                SkipWhitespace();
                if (Peek() == '}') { Position++; return new PSObject(); }
                while (true) {
                    SkipWhitespace();
                    if (Peek() != '"') throw new ProtocolException("Expected JSON member name");
                    string name = ReadString();
                    if (!exact.Add(name)) throw new ProtocolException("Duplicate JSON member: " + name);
                    if (!folded.Add(name)) caseCollision = true;
                    SkipWhitespace();
                    if (Peek() != ':') throw new ProtocolException("Expected ':' in JSON object");
                    Position++;
                    SkipWhitespace();
                    members.Add(new KeyValuePair<string, object>(name, ReadValue(depth + 1)));
                    SkipWhitespace();
                    char next = Peek();
                    Position++;
                    if (next == '}') break;
                    if (next != ',') throw new ProtocolException("Expected ',' or '}' in JSON object");
                }
                if (caseCollision) {
                    // PowerShell member names are case-insensitive. Preserve
                    // distinct keys such as {"A":1,"a":2} in an ordinal table.
                    Hashtable table = new Hashtable(StringComparer.Ordinal);
                    foreach (KeyValuePair<string, object> member in members) table[member.Key] = member.Value;
                    return table;
                }
                PSObject result = new PSObject();
                foreach (KeyValuePair<string, object> member in members)
                    result.Properties.Add(new PSNoteProperty(member.Key, member.Value));
                return result;
            }
            private object ReadArray(int depth) {
                Position++;
                List<object> items = new List<object>();
                SkipWhitespace();
                if (Peek() == ']') { Position++; return items.ToArray(); }
                while (true) {
                    SkipWhitespace();
                    items.Add(ReadValue(depth + 1));
                    SkipWhitespace();
                    char next = Peek();
                    Position++;
                    if (next == ']') break;
                    if (next != ',') throw new ProtocolException("Expected ',' or ']' in JSON array");
                }
                return items.ToArray();
            }
            private static int Hex(char c) {
                if (c >= '0' && c <= '9') return c - '0';
                if (c >= 'a' && c <= 'f') return c - 'a' + 10;
                if (c >= 'A' && c <= 'F') return c - 'A' + 10;
                throw new ProtocolException("Invalid JSON unicode escape");
            }
            private string ReadString() {
                Position++;
                StringBuilder builder = null;
                int start = Position;
                while (true) {
                    if (Position >= text.Length) throw new ProtocolException("Unterminated JSON string");
                    char c = text[Position];
                    if (c == '"') {
                        string tail = text.Substring(start, Position - start);
                        Position++;
                        if (builder == null) return tail;
                        builder.Append(tail);
                        return builder.ToString();
                    }
                    if (c < ' ') throw new ProtocolException("Control character in JSON string");
                    if (c != '\\') { Position++; continue; }
                    if (builder == null) builder = new StringBuilder();
                    builder.Append(text, start, Position - start);
                    Position++;
                    char e = Peek();
                    Position++;
                    switch (e) {
                        case '"': builder.Append('"'); break;
                        case '\\': builder.Append('\\'); break;
                        case '/': builder.Append('/'); break;
                        case 'b': builder.Append('\b'); break;
                        case 'f': builder.Append('\f'); break;
                        case 'n': builder.Append('\n'); break;
                        case 'r': builder.Append('\r'); break;
                        case 't': builder.Append('\t'); break;
                        case 'u':
                            if (Position + 4 > text.Length) throw new ProtocolException("Truncated JSON unicode escape");
                            int code = (Hex(text[Position]) << 12) | (Hex(text[Position + 1]) << 8) |
                                       (Hex(text[Position + 2]) << 4) | Hex(text[Position + 3]);
                            builder.Append((char)code);
                            Position += 4;
                            break;
                        default: throw new ProtocolException("Invalid JSON escape");
                    }
                    start = Position;
                }
            }
            private object ReadNumber() {
                int start = Position;
                if (text[Position] == '-') Position++;
                if (Position >= text.Length) throw new ProtocolException("Invalid JSON number");
                if (text[Position] == '0') Position++;
                else if (text[Position] >= '1' && text[Position] <= '9') {
                    while (Position < text.Length && char.IsDigit(text[Position]) && text[Position] < 128) Position++;
                } else throw new ProtocolException("Invalid JSON number");
                bool integral = true;
                if (Position < text.Length && text[Position] == '.') {
                    integral = false;
                    Position++;
                    int digits = Position;
                    while (Position < text.Length && text[Position] >= '0' && text[Position] <= '9') Position++;
                    if (Position == digits) throw new ProtocolException("Invalid JSON fraction");
                }
                if (Position < text.Length && (text[Position] == 'e' || text[Position] == 'E')) {
                    integral = false;
                    Position++;
                    if (Position < text.Length && (text[Position] == '+' || text[Position] == '-')) Position++;
                    int digits = Position;
                    while (Position < text.Length && text[Position] >= '0' && text[Position] <= '9') Position++;
                    if (Position == digits) throw new ProtocolException("Invalid JSON exponent");
                }
                string token = text.Substring(start, Position - start);
                if (integral) {
                    int small;
                    if (int.TryParse(token, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out small)) return small;
                    long large;
                    if (long.TryParse(token, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out large)) return large;
                    ulong huge;
                    if (ulong.TryParse(token, NumberStyles.None, CultureInfo.InvariantCulture, out huge)) return huge;
                }
                double value = double.Parse(token, NumberStyles.Float, CultureInfo.InvariantCulture);
                if (double.IsInfinity(value) || double.IsNaN(value)) throw new ProtocolException("Non-finite JSON number");
                return value;
            }
        }

        public static string Serialize(object value) {
            StringBuilder builder = new StringBuilder();
            Write(builder, value, 0);
            return builder.ToString();
        }

        private static void WriteString(StringBuilder builder, string text) {
            builder.Append('"');
            foreach (char c in text) {
                switch (c) {
                    case '"': builder.Append("\\\""); break;
                    case '\\': builder.Append("\\\\"); break;
                    case '\n': builder.Append("\\n"); break;
                    case '\r': builder.Append("\\r"); break;
                    case '\t': builder.Append("\\t"); break;
                    case '\b': builder.Append("\\b"); break;
                    case '\f': builder.Append("\\f"); break;
                    default:
                        if (c < ' ') builder.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        else builder.Append(c);
                        break;
                }
            }
            builder.Append('"');
        }

        private static void Write(StringBuilder builder, object value, int depth) {
            if (depth > MaximumDepth) throw new ArgumentException("Value is nested too deeply for JSON");
            PSObject wrapped = value as PSObject;
            if (wrapped != null) {
                if (wrapped.BaseObject is PSCustomObject) {
                    builder.Append('{');
                    bool firstMember = true;
                    foreach (PSPropertyInfo property in wrapped.Properties) {
                        if (!firstMember) builder.Append(',');
                        firstMember = false;
                        WriteString(builder, property.Name);
                        builder.Append(':');
                        Write(builder, property.Value, depth + 1);
                    }
                    builder.Append('}');
                    return;
                }
                value = wrapped.BaseObject;
            }
            if (value == null || value is DBNull) { builder.Append("null"); return; }
            string text = value as string;
            if (text != null) { WriteString(builder, text); return; }
            if (value is char) { WriteString(builder, value.ToString()); return; }
            if (value is bool) { builder.Append((bool)value ? "true" : "false"); return; }
            if (value is int || value is long || value is short || value is sbyte || value is byte ||
                value is uint || value is ulong || value is ushort) {
                builder.Append(Convert.ToString(value, CultureInfo.InvariantCulture));
                return;
            }
            if (value is double || value is float) {
                double number = Convert.ToDouble(value, CultureInfo.InvariantCulture);
                if (double.IsNaN(number) || double.IsInfinity(number))
                    throw new ArgumentException("JSON cannot represent NaN or infinity");
                builder.Append(number.ToString("R", CultureInfo.InvariantCulture));
                return;
            }
            if (value is decimal) {
                builder.Append(((decimal)value).ToString(CultureInfo.InvariantCulture));
                return;
            }
            IDictionary dictionary = value as IDictionary;
            if (dictionary != null) {
                builder.Append('{');
                bool firstEntry = true;
                foreach (DictionaryEntry entry in dictionary) {
                    object key = entry.Key;
                    PSObject wrappedKey = key as PSObject;
                    if (wrappedKey != null) key = wrappedKey.BaseObject;
                    if (!(key is string) && !(key is char))
                        throw new ArgumentException("JSON object keys must be strings");
                    if (!firstEntry) builder.Append(',');
                    firstEntry = false;
                    WriteString(builder, key.ToString());
                    builder.Append(':');
                    Write(builder, entry.Value, depth + 1);
                }
                builder.Append('}');
                return;
            }
            if (value is ScriptBlock || value is Enum || value is DateTime || value is Delegate)
                throw new ArgumentException("Cannot send a " + value.GetType().FullName + " to the native host; convert it to a string or number");
            IEnumerable sequence = value as IEnumerable;
            if (sequence != null) {
                builder.Append('[');
                bool firstItem = true;
                foreach (object item in sequence) {
                    if (!firstItem) builder.Append(',');
                    firstItem = false;
                    Write(builder, item, depth + 1);
                }
                builder.Append(']');
                return;
            }
            throw new ArgumentException("Cannot send a " + value.GetType().FullName + " to the native host as JSON");
        }

        /// <summary>Returns a member of a parsed object, or null.</summary>
        public static object Field(object container, string name) {
            if (container == null) return null;
            PSObject wrapped = container as PSObject;
            if (wrapped != null) {
                if (wrapped.BaseObject is PSCustomObject) {
                    PSPropertyInfo property = wrapped.Properties[name];
                    return property == null ? null : property.Value;
                }
                container = wrapped.BaseObject;
            }
            IDictionary dictionary = container as IDictionary;
            if (dictionary != null) return dictionary.Contains(name) ? dictionary[name] : null;
            return null;
        }
        public static bool HasField(object container, string name) {
            if (container == null) return false;
            PSObject wrapped = container as PSObject;
            if (wrapped != null) {
                if (wrapped.BaseObject is PSCustomObject) return wrapped.Properties[name] != null;
                container = wrapped.BaseObject;
            }
            IDictionary dictionary = container as IDictionary;
            return dictionary != null && dictionary.Contains(name);
        }
        public static bool IsObject(object value) {
            PSObject wrapped = value as PSObject;
            if (wrapped != null) return wrapped.BaseObject is PSCustomObject || wrapped.BaseObject is IDictionary;
            return value is IDictionary;
        }
    }

    /// <summary>One validated JSON-RPC message received from the host.</summary>
    public sealed class Message {
        public string Kind { get; internal set; }   // response | request | notification
        public string Id { get; internal set; }
        public string Method { get; internal set; }
        public object Params { get; internal set; }
        public object Result { get; internal set; }
        public bool IsError { get; internal set; }
        public long ErrorCode { get; internal set; }
        public string ErrorMessage { get; internal set; }
        public object ErrorData { get; internal set; }
        internal int Size;
    }

    public sealed class Transport : IDisposable {
        public const int MaximumFrame = 2 * 1024 * 1024;
        public const int MaximumQueuedBytes = 8 * 1024 * 1024;
        public const int MaximumQueuedMessages = 128;
        public const int MaximumPending = 128;

        private readonly BlockingCollection<Message> incoming = new BlockingCollection<Message>();
        private readonly HashSet<string> pending = new HashSet<string>(StringComparer.Ordinal);
        private readonly object stateLock = new object();
        private readonly object writeLock = new object();
        private readonly UTF8Encoding utf8 = new UTF8Encoding(false, true);
        private readonly Stream stream;
        private readonly Thread reader;
        // Rejections of host requests that arrive while the callback queue is
        // full. The reader thread must never block on a write: a peer that is
        // itself busy writing would fill the pipe and deadlock both sides.
        private readonly BlockingCollection<Hashtable> rejections = new BlockingCollection<Hashtable>(MaximumPending * 2);
        private readonly Thread rejecter;
        private volatile bool disposed;
        private volatile bool disconnected;
        private volatile string failure;
        private int queuedMessages;
        private int queuedBytes;
        private long droppedNotifications;
        private long rejectedRequests;
        private long receivedMessages;

        public Transport(string endpoint, int connectTimeoutMs) {
            if (string.IsNullOrEmpty(endpoint)) throw new ArgumentException("Endpoint must not be empty.");
            if (connectTimeoutMs < 1 || connectTimeoutMs > 120000) throw new ArgumentOutOfRangeException("connectTimeoutMs");
            if (endpoint.StartsWith(@"\\.\pipe\", StringComparison.OrdinalIgnoreCase)) {
                string pipeName = endpoint.Substring(9);
                if (pipeName.Length == 0 || pipeName.IndexOf('\\') >= 0 || pipeName.IndexOf('/') >= 0)
                    throw new ArgumentException("Expected a private local Windows named pipe.");
                // Asynchronous (overlapped) so a blocked read on the reader
                // thread never serializes writes from the PowerShell thread.
                NamedPipeClientStream pipe = new NamedPipeClientStream(".", pipeName, PipeDirection.InOut,
                    PipeOptions.Asynchronous);
                try { pipe.Connect(connectTimeoutMs); stream = pipe; }
                catch { pipe.Dispose(); throw; }
            } else {
                // .NET Framework has no UnixDomainSocketEndPoint. Resolve it at
                // runtime so this source also compiles for Windows PowerShell.
                Type unixEndpointType = Type.GetType("System.Net.Sockets.UnixDomainSocketEndPoint, System.Net.Sockets", false);
                if (unixEndpointType == null)
                    unixEndpointType = typeof(Socket).Assembly.GetType("System.Net.Sockets.UnixDomainSocketEndPoint", false);
                if (unixEndpointType == null)
                    throw new PlatformNotSupportedException("Unix-domain sockets require PowerShell 7.2+.");
                if (!Path.IsPathRooted(endpoint)) throw new ArgumentException("Unix socket path must be absolute.");
                object resolved = Activator.CreateInstance(unixEndpointType, new object[] { endpoint });
                Socket socket = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
                try {
                    // EndConnect still uses the wait handle; never dispose it first.
                    IAsyncResult connecting = socket.BeginConnect((EndPoint)resolved, null, null);
                    if (!connecting.AsyncWaitHandle.WaitOne(connectTimeoutMs))
                        throw new TimeoutException("Timed out connecting to the native host.");
                    socket.EndConnect(connecting);
                    stream = new NetworkStream(socket, true);
                } catch { socket.Dispose(); throw; }
            }
            reader = new Thread(ReadLoop);
            reader.Name = "ui4me-powershell-reader";
            reader.IsBackground = true;
            rejecter = new Thread(RejectLoop);
            rejecter.Name = "ui4me-powershell-rejecter";
            rejecter.IsBackground = true;
            rejecter.Start();
            reader.Start();
        }

        private void RejectLoop() {
            try {
                foreach (Hashtable response in rejections.GetConsumingEnumerable()) {
                    if (disposed || disconnected) return;
                    try { Send(response); } catch (DisconnectedException) { return; }
                }
            } catch (ObjectDisposedException) { }
        }

        public bool IsDisconnected { get { return disconnected || disposed; } }
        public string Failure { get { return failure ?? "The native host disconnected"; } }
        public long DroppedNotifications { get { return Interlocked.Read(ref droppedNotifications); } }
        public long RejectedRequests { get { return Interlocked.Read(ref rejectedRequests); } }
        public long ReceivedMessages { get { return Interlocked.Read(ref receivedMessages); } }
        public int QueuedMessages { get { return Volatile.Read(ref queuedMessages); } }
        public int PendingCount { get { lock (stateLock) return pending.Count; } }

        /// <summary>Registers an outgoing request ID before its frame is sent.</summary>
        public void Register(string id) {
            if (string.IsNullOrEmpty(id)) throw new ArgumentException("Request ID must not be empty");
            lock (stateLock) {
                if (disposed || disconnected) throw new DisconnectedException(Failure);
                if (pending.Count >= MaximumPending) throw new InvalidOperationException("UI4ME has 128 outstanding host requests");
                if (!pending.Add(id)) throw new InvalidOperationException("Duplicate request ID");
            }
        }
        /// <summary>Forgets a request; a late response for it is discarded.</summary>
        public void Forget(string id) {
            if (id == null) return;
            lock (stateLock) pending.Remove(id);
        }

        private static bool IsLifecycle(string method, object parameters) {
            if (method == "app.shutdown" || method == "window.before_close" || method == "app.before_quit") return true;
            return method == "window.event" && (Json.Field(parameters, "type") as string) == "closed";
        }

        private byte[] ReadExact(int count, bool frameStart) {
            byte[] data = new byte[count];
            int consumed = 0;
            while (consumed < count) {
                int n = stream.Read(data, consumed, count - consumed);
                if (n <= 0) {
                    if (frameStart && consumed == 0) return null;
                    throw new EndOfStreamException("The native host closed the connection inside a frame.");
                }
                consumed += n;
            }
            return data;
        }

        private Message Decode(string json, int size) {
            object value = Json.Parse(json);
            if (!Json.IsObject(value) || !"2.0".Equals(Json.Field(value, "jsonrpc")))
                throw new ProtocolException("Expected a JSON-RPC 2.0 object");
            Message message = new Message();
            message.Size = size;
            object id = Json.Field(value, "id");
            bool hasId = Json.HasField(value, "id");
            if (Json.HasField(value, "method")) {
                string method = Json.Field(value, "method") as string;
                if (method == null) throw new ProtocolException("Host method must be a string");
                object parameters = Json.Field(value, "params");
                if (parameters == null) parameters = new PSObject();
                else if (!Json.IsObject(parameters)) throw new ProtocolException("Host params must be an object");
                if (hasId && !(id is string)) throw new ProtocolException("Host request IDs must be strings");
                message.Kind = hasId ? "request" : "notification";
                message.Id = id as string;
                message.Method = method;
                message.Params = parameters;
                return message;
            }
            if (!(id is string)) throw new ProtocolException("Host response has an invalid request ID");
            bool hasResult = Json.HasField(value, "result");
            bool hasError = Json.HasField(value, "error");
            if (hasResult == hasError) throw new ProtocolException("Host response must contain result or error");
            message.Kind = "response";
            message.Id = (string)id;
            if (hasError) {
                object error = Json.Field(value, "error");
                object code = Json.Field(error, "code");
                string text = Json.Field(error, "message") as string;
                if (!Json.IsObject(error) || !(code is int || code is long) || text == null)
                    throw new ProtocolException("Invalid JSON-RPC error object");
                message.IsError = true;
                message.ErrorCode = Convert.ToInt64(code, CultureInfo.InvariantCulture);
                message.ErrorMessage = text;
                message.ErrorData = Json.Field(error, "data");
            } else {
                message.Result = Json.Field(value, "result");
            }
            return message;
        }

        private void ReadLoop() {
            try {
                while (!disposed) {
                    byte[] header = ReadExact(4, true);
                    if (header == null) throw new EndOfStreamException("The native host closed the connection.");
                    long size = ((long)header[0] << 24) | ((long)header[1] << 16) | ((long)header[2] << 8) | header[3];
                    if (size < 1 || size > MaximumFrame) throw new ProtocolException("Invalid frame length");
                    byte[] body = ReadExact((int)size, false);
                    string json;
                    try { json = utf8.GetString(body); }
                    catch (DecoderFallbackException) { throw new ProtocolException("Frame is not valid UTF-8"); }
                    Message message = Decode(json, (int)size);
                    Interlocked.Increment(ref receivedMessages);
                    if (message.Kind == "response") {
                        bool expected;
                        lock (stateLock) expected = pending.Remove(message.Id);
                        if (expected) incoming.Add(message);  // bounded by MaximumPending
                        continue;                              // late reply to a cancelled request
                    }
                    bool admit;
                    lock (stateLock) {
                        admit = IsLifecycle(message.Method, message.Params) ||
                            (queuedMessages < MaximumQueuedMessages && queuedBytes + message.Size <= MaximumQueuedBytes);
                        if (admit) { queuedMessages++; queuedBytes += message.Size; }
                    }
                    if (admit) { incoming.Add(message); continue; }
                    if (message.Kind == "request") {
                        Interlocked.Increment(ref rejectedRequests);
                        Hashtable error = new Hashtable();
                        error["code"] = -32005;
                        error["message"] = "PowerShell callback queue is full";
                        Hashtable response = new Hashtable();
                        response["jsonrpc"] = "2.0";
                        response["id"] = message.Id;
                        response["error"] = error;
                        if (!rejections.TryAdd(response))
                            throw new ProtocolException("The host keeps sending requests that the backend cannot accept");
                    } else {
                        Interlocked.Increment(ref droppedNotifications);
                    }
                }
            } catch (Exception ex) {
                if (!disposed) failure = ex.GetType().Name + ": " + ex.Message;
            } finally {
                disconnected = true;
                try { incoming.CompleteAdding(); } catch (ObjectDisposedException) { }
                try { rejections.CompleteAdding(); } catch (ObjectDisposedException) { }
            }
        }

        /// <summary>
        /// Returns the next host message, or null after the timeout. Throws an
        /// IOException once the host disconnected and every message was consumed.
        /// </summary>
        public Message Receive(int timeoutMs) {
            if (timeoutMs < -1) throw new ArgumentOutOfRangeException("timeoutMs");
            Message message;
            bool taken;
            try { taken = incoming.TryTake(out message, timeoutMs); }
            catch (ObjectDisposedException) { throw new DisconnectedException(Failure); }
            if (taken) {
                if (message.Kind != "response") {
                    lock (stateLock) { queuedMessages--; queuedBytes -= message.Size; }
                }
                return message;
            }
            if (incoming.IsCompleted) throw new DisconnectedException(Failure);
            return null;
        }

        /// <summary>Serializes and writes one frame. Thread-safe.</summary>
        public void Send(object envelope) {
            if (disposed || disconnected) throw new DisconnectedException(Failure);
            byte[] body = utf8.GetBytes(Json.Serialize(envelope));
            if (body.Length == 0 || body.Length > MaximumFrame)
                throw new InvalidDataException("A JSON-RPC frame must be 1 byte to 2 MiB");
            byte[] frame = new byte[body.Length + 4];
            frame[0] = (byte)(body.Length >> 24);
            frame[1] = (byte)(body.Length >> 16);
            frame[2] = (byte)(body.Length >> 8);
            frame[3] = (byte)body.Length;
            Buffer.BlockCopy(body, 0, frame, 4, body.Length);
            lock (writeLock) {
                if (disposed || disconnected) throw new DisconnectedException(Failure);
                try {
                    stream.Write(frame, 0, frame.Length);
                    stream.Flush();
                } catch (Exception ex) {
                    if (ex is IOException || ex is ObjectDisposedException || ex is SocketException) {
                        failure = failure ?? (ex.GetType().Name + ": " + ex.Message);
                        disconnected = true;
                        throw new DisconnectedException(Failure, ex);
                    }
                    throw;
                }
            }
        }

        public void Dispose() {
            lock (stateLock) {
                if (disposed) return;
                disposed = true;
                pending.Clear();
            }
            try { stream.Dispose(); } catch (IOException) { } catch (ObjectDisposedException) { }
            if (Thread.CurrentThread != reader) reader.Join(2000);
            try { incoming.CompleteAdding(); } catch (ObjectDisposedException) { }
            try { rejections.CompleteAdding(); } catch (ObjectDisposedException) { }
            if (Thread.CurrentThread != rejecter) rejecter.Join(2000);
        }
    }

    /// <summary>A writable view of one host-created live-bitmap file.</summary>
    public sealed class MappedBitmap : IDisposable {
        private FileStream file;
        private MemoryMappedFile map;
        private MemoryMappedViewAccessor view;
        private readonly long capacity;

        public MappedBitmap(string path, long capacity, bool deleteOnClose) {
            if (string.IsNullOrEmpty(path) || !Path.IsPathRooted(path)) throw new ArgumentException("Bitmap path must be absolute");
            if (capacity < 1) throw new ArgumentOutOfRangeException("capacity");
            this.capacity = capacity;
            // Delete sharing lets the host unlink the transport after the first
            // present; Windows delete-on-close removes it if both sides die.
            FileOptions options = deleteOnClose ? FileOptions.DeleteOnClose : FileOptions.None;
            file = new FileStream(path, FileMode.Open, FileAccess.ReadWrite,
                FileShare.ReadWrite | FileShare.Delete, 4096, options);
            try {
                if (file.Length < capacity) throw new InvalidDataException("Host bitmap file is smaller than its geometry");
#if UI4ME_NETFX
                map = MemoryMappedFile.CreateFromFile(file, null, 0, MemoryMappedFileAccess.ReadWrite,
                    null, HandleInheritability.None, true);
#else
                map = MemoryMappedFile.CreateFromFile(file, null, 0, MemoryMappedFileAccess.ReadWrite,
                    HandleInheritability.None, true);
#endif
                view = map.CreateViewAccessor(0, capacity, MemoryMappedFileAccess.ReadWrite);
            } catch {
                Dispose();
                throw;
            }
        }
        public long Capacity { get { return capacity; } }
        public bool IsClosed { get { return view == null; } }

        public void Write(long offset, byte[] data) {
            if (data == null) throw new ArgumentNullException("data");
            if (view == null) throw new ObjectDisposedException("MappedBitmap");
            if (offset < 0 || offset + data.LongLength > capacity) throw new ArgumentOutOfRangeException("offset");
            // One bulk copy into the mapped view. UnmanagedMemoryAccessor.WriteArray
            // copies element by element on .NET Framework (Windows PowerShell 5.1),
            // which made a 1024x768 frame take ~150 ms there.
            SafeMemoryMappedViewHandle handle = view.SafeMemoryMappedViewHandle;
            bool added = false;
            try {
                handle.DangerousAddRef(ref added);
                long address = handle.DangerousGetHandle().ToInt64() + view.PointerOffset + offset;
                System.Runtime.InteropServices.Marshal.Copy(data, 0, new IntPtr(address), data.Length);
            } finally {
                if (added) handle.DangerousRelease();
            }
        }
        public void Dispose() {
            if (view != null) { view.Dispose(); view = null; }
            if (map != null) { map.Dispose(); map = null; }
            if (file != null) { file.Dispose(); file = null; }
        }
    }

    /// <summary>
    /// Runs the native host for Start-UI4MEScript. Host log records on stderr
    /// are relayed as plain text: backend output to the matching console stream,
    /// host warnings/errors summarized (all records when verbose). The host's
    /// lifetime is bound to this process: a kill-on-close job on Windows; on
    /// Linux the caller starts it through setpriv --pdeathsig when available.
    /// </summary>
    public sealed class HostProcess : IDisposable {
        private readonly System.Diagnostics.Process process;
        private readonly Thread relay;
        private readonly bool verbose;
        private IntPtr job = IntPtr.Zero;

        public HostProcess(string fileName, string[] arguments, string workingDirectory, bool verbose) {
            if (string.IsNullOrEmpty(fileName)) throw new ArgumentException("fileName");
            this.verbose = verbose;
            System.Diagnostics.ProcessStartInfo info = new System.Diagnostics.ProcessStartInfo();
            info.FileName = fileName;
            info.UseShellExecute = false;
            info.RedirectStandardError = true;
            info.StandardErrorEncoding = new UTF8Encoding(false);
            info.WorkingDirectory = workingDirectory;
#if UI4ME_NETFX
            StringBuilder line = new StringBuilder();
            foreach (string argument in arguments) {
                if (line.Length > 0) line.Append(' ');
                line.Append(QuoteWindowsArgument(argument));
            }
            info.Arguments = line.ToString();
#else
            foreach (string argument in arguments) info.ArgumentList.Add(argument);
#endif
            process = System.Diagnostics.Process.Start(info);
            if (Environment.OSVersion.Platform == PlatformID.Win32NT) job = BindToThisProcess(process);
            relay = new Thread(Relay);
            relay.IsBackground = true;
            relay.Name = "ui4me-host-output";
            relay.Start();
        }

        public int Id { get { return process.Id; } }
        public bool HasExited { get { return process.HasExited; } }

        /// <summary>True when the host exited (and its output was relayed) within the timeout.</summary>
        public bool WaitForExit(int milliseconds) {
            if (!process.WaitForExit(milliseconds)) return false;
            relay.Join(2000);
            return true;
        }
        public int ExitCode { get { return process.ExitCode; } }

        public void Stop() {
            try { if (!process.HasExited) process.Kill(); } catch (InvalidOperationException) { }
            process.WaitForExit(10000);
            relay.Join(2000);
        }

        public void Dispose() {
            process.Dispose();
            if (job != IntPtr.Zero) { CloseHandle(job); job = IntPtr.Zero; }
        }

        private void Relay() {
            try {
                string line;
                while ((line = process.StandardError.ReadLine()) != null) RelayLine(line, verbose, Console.Out, Console.Error);
            } catch (IOException) { } catch (ObjectDisposedException) { }
        }

        public static void RelayLine(string line, bool verbose, TextWriter output, TextWriter error) {
            object record = null;
            try { if (line.StartsWith("{", StringComparison.Ordinal)) record = Json.Parse(line); } catch (Exception) { record = null; }
            object fields = Json.IsObject(record) ? Json.Field(record, "fields") : null;
            if (!Json.IsObject(fields)) {
                if (line.Length > 0) Write(error, line);
                return;
            }
            string text = Json.Field(fields, "text") as string;
            if ("backend".Equals(Json.Field(fields, "source")) && text != null) {
                Write("stdout".Equals(Json.Field(fields, "stream")) ? output : error, text);
                return;
            }
            string level = (Json.Field(record, "level") as string ?? "INFO").ToUpperInvariant();
            if (!verbose && level != "WARN" && level != "ERROR") return;
            StringBuilder extra = new StringBuilder();
            PSObject wrapped = fields as PSObject;
            if (wrapped != null) {
                foreach (PSPropertyInfo property in wrapped.Properties) {
                    if (property.Name == "message") continue;
                    if (extra.Length > 0) extra.Append(", ");
                    extra.Append(property.Name).Append('=').Append(Convert.ToString(property.Value, CultureInfo.InvariantCulture));
                }
            }
            string message = Convert.ToString(Json.Field(fields, "message"), CultureInfo.InvariantCulture);
            Write(error, "ui4me-host " + level.ToLowerInvariant() + ": " + message + (extra.Length > 0 ? " (" + extra + ")" : ""));
        }

        private static void Write(TextWriter writer, string text) {
            try { lock (writer) { writer.WriteLine(text); writer.Flush(); } } catch (IOException) { } catch (ObjectDisposedException) { }
        }

        /// <summary>CommandLineToArgvW quoting, for .NET Framework's single argument string.</summary>
        public static string QuoteWindowsArgument(string argument) {
            if (argument == null) throw new ArgumentNullException("argument");
            if (argument.Length > 0 && argument.IndexOfAny(new char[] { ' ', '\t', '\n', '\v', '"' }) < 0) return argument;
            StringBuilder quoted = new StringBuilder("\"");
            int slashes = 0;
            foreach (char c in argument) {
                if (c == '\\') { slashes++; continue; }
                if (c == '"') quoted.Append('\\', slashes * 2 + 1);
                else quoted.Append('\\', slashes);
                slashes = 0;
                quoted.Append(c);
            }
            quoted.Append('\\', slashes * 2);
            return quoted.Append('"').ToString();
        }

        // Kill-on-close job: if this PowerShell process ends for any reason,
        // Windows terminates the host, whose own job ends the backend. Processes
        // the host opens for the user break away silently and stay open.
        private static IntPtr BindToThisProcess(System.Diagnostics.Process child) {
            IntPtr handle = CreateJobObject(IntPtr.Zero, null);
            if (handle == IntPtr.Zero) return IntPtr.Zero;
            ExtendedLimits limits = new ExtendedLimits();
            limits.BasicLimitInformation.LimitFlags = 0x2000 | 0x1000; // KILL_ON_JOB_CLOSE | SILENT_BREAKAWAY_OK
            int size = System.Runtime.InteropServices.Marshal.SizeOf(typeof(ExtendedLimits));
            IntPtr buffer = System.Runtime.InteropServices.Marshal.AllocHGlobal(size);
            try {
                System.Runtime.InteropServices.Marshal.StructureToPtr(limits, buffer, false);
                if (SetInformationJobObject(handle, 9, buffer, (uint)size) && AssignProcessToJobObject(handle, child.Handle)) return handle;
            } finally {
                System.Runtime.InteropServices.Marshal.FreeHGlobal(buffer);
            }
            CloseHandle(handle);
            return IntPtr.Zero;
        }

        [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
        private struct BasicLimits {
            public long PerProcessUserTimeLimit; public long PerJobUserTimeLimit; public uint LimitFlags;
            public UIntPtr MinimumWorkingSetSize; public UIntPtr MaximumWorkingSetSize; public uint ActiveProcessLimit;
            public UIntPtr Affinity; public uint PriorityClass; public uint SchedulingClass;
        }
        [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
        private struct IoCounters {
            public ulong ReadOperationCount; public ulong WriteOperationCount; public ulong OtherOperationCount;
            public ulong ReadTransferCount; public ulong WriteTransferCount; public ulong OtherTransferCount;
        }
        [System.Runtime.InteropServices.StructLayout(System.Runtime.InteropServices.LayoutKind.Sequential)]
        private struct ExtendedLimits {
            public BasicLimits BasicLimitInformation; public IoCounters IoInfo;
            public UIntPtr ProcessMemoryLimit; public UIntPtr JobMemoryLimit;
            public UIntPtr PeakProcessMemoryUsed; public UIntPtr PeakJobMemoryUsed;
        }
        [System.Runtime.InteropServices.DllImport("kernel32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr CreateJobObject(IntPtr attributes, string name);
        [System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetInformationJobObject(IntPtr job, int infoClass, IntPtr info, uint length);
        [System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [System.Runtime.InteropServices.DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool CloseHandle(IntPtr handle);
    }
}
