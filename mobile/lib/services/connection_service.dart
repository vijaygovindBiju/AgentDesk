import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'package:flutter/foundation.dart';
import 'package:web_socket_channel/io.dart';
import 'package:web_socket_channel/web_socket_channel.dart';

import '../models/models.dart';
import '../state/agentdesk_state.dart';
import 'connection_configuration_store.dart';
import 'fingerprint.dart';
import 'relay_url.dart';
import 'secure_token_store.dart';

enum ConnectionStatus {
  configurationRequired,
  disconnected,
  connecting,
  authenticating,
  connected,
  reconnecting,
  error,
}

class ConnectionService extends ChangeNotifier {
  /// Connect timeout sized for high-latency mobile (4G/5G) networks.
  static const relayConnectTimeout = Duration(seconds: 15);

  /// Keepalive so a dead socket (Wi-Fi <-> mobile handover, NAT rebinding)
  /// is detected and the reconnect loop kicks in.
  static const relayPingInterval = Duration(seconds: 20);

  final AgentDeskState state;

  /// Test-harness only: permits connecting directly to a loopback AgentDesk
  /// daemon without a relay. Never enabled by the app; production traffic is
  /// relay-only (no LAN, no USB/adb reverse, no direct laptop sockets).
  final bool allowLoopbackTestServer;

  final ConnectionConfigurationStore _configurationStore;
  final SecureTokenStore _tokenStore;
  final Random _random = Random();

  WebSocketChannel? _channel;
  StreamSubscription? _subscription;
  Timer? _reconnectTimer;

  ConnectionStatus _status = ConnectionStatus.configurationRequired;
  ConnectionStatus get status => _status;

  String? _lastError;
  String? get lastError => _lastError;

  String _url = '';
  String _token = '';
  String _deviceId = 'flutter-client-1';
  String? _fingerprint;
  String? _instanceId;
  String? _relayUrl;
  TransportMode? _activeTransport;
  String? _connectedEndpoint;

  String get url => _url;
  String get token => _token;
  String get deviceId => _deviceId;
  String? get fingerprint => _fingerprint;
  String? get instanceId => _instanceId;
  String? get relayUrl => _relayUrl;
  TransportMode? get activeTransport => _activeTransport;
  String? get connectedEndpoint => _connectedEndpoint;
  bool get isPaired => _instanceId != null && _instanceId!.isNotEmpty;

  /// A user-facing explanation when the app cannot safely connect yet.
  String? get configurationError => _configurationError();
  bool get isConfigurationComplete => configurationError == null;

  bool _isActive = false;
  double _backoffSeconds = 1.0;
  int _reqCounter = 0;
  final Map<String, Completer<Message>> _pendingRequests = {};

  ConnectionService({
    required this.state,
    ConnectionConfigurationStore? configurationStore,
    SecureTokenStore? tokenStore,
    this.allowLoopbackTestServer = false,
  }) : _configurationStore =
           configurationStore ??
           SharedPreferencesConnectionConfigurationStore(),
       _tokenStore = tokenStore ?? const FlutterSecureTokenStore();

  void configure({
    String? url,
    String? token,
    String? deviceId,
    String? fingerprint,
  }) {
    if (url != null) _url = url.trim();
    if (token != null) _token = token.trim();
    if (deviceId != null) _deviceId = deviceId.trim();
    if (fingerprint != null) {
      _fingerprint = fingerprint.trim().isEmpty ? null : fingerprint.trim();
    }
    notifyListeners();
  }

  /// Loads non-secret settings from ordinary preferences and the token from
  /// OS-backed secure storage. This must complete before a startup connection.
  Future<void> restoreConfiguration() async {
    try {
      final stored = await _configurationStore.load();
      final token = await _tokenStore.readToken();
      _url = stored.url?.trim() ?? '';
      _deviceId = stored.deviceId?.trim().isEmpty ?? true
          ? 'flutter-client-1'
          : stored.deviceId!.trim();
      _fingerprint = _blankToNull(stored.fingerprint);
      _instanceId = stored.instanceId?.trim();
      _relayUrl = stored.relayUrl?.trim();
      _token = token?.trim() ?? '';
      _setConfigurationStatus();
    } catch (error) {
      _isActive = false;
      _setStatus(
        ConnectionStatus.configurationRequired,
        'Unable to read saved connection configuration securely. '
        'Existing credentials were preserved; open Settings to retry or re-enter configuration. '
        '($error)',
      );
    }
  }

  /// Persists non-secret settings separately from the authentication token.
  /// An empty token removes it from secure storage and leaves the app in a
  /// configuration-required state rather than attempting an unauthenticated connection.
  Future<void> saveConfiguration({
    required String url,
    required String token,
    required String deviceId,
    required String fingerprint,
  }) async {
    final normalizedUrl = url.trim();
    final normalizedToken = token.trim();
    final normalizedDeviceId = deviceId.trim();
    final normalizedFingerprint = _blankToNull(fingerprint);

    await _configurationStore.save(
      ConnectionConfiguration(
        url: normalizedUrl.isEmpty ? null : normalizedUrl,
        deviceId: normalizedDeviceId.isEmpty ? null : normalizedDeviceId,
        fingerprint: normalizedFingerprint,
        instanceId: _instanceId,
        relayUrl: _relayUrl,
      ),
    );
    if (normalizedToken.isEmpty) {
      await _tokenStore.deleteToken();
    } else {
      await _tokenStore.writeToken(normalizedToken);
    }

    _url = normalizedUrl;
    _token = normalizedToken;
    _deviceId = normalizedDeviceId;
    _fingerprint = normalizedFingerprint;
    _setConfigurationStatus();
    notifyListeners();
  }

  /// Persists newly paired device credentials and optionally connects.
  Future<void> savePairing({
    required String instanceId,
    required String deviceToken,
    required String deviceId,
    required String fingerprint,
    required String relayUrl,
    bool autoConnect = true,
  }) async {
    final url = relayUrl.trim();
    _instanceId = instanceId.trim();
    _relayUrl = url;
    _url = url;
    _token = deviceToken.trim();
    _deviceId = deviceId.trim();
    _fingerprint = _blankToNull(fingerprint);

    await _tokenStore.writeToken(_token);
    await _configurationStore.save(
      ConnectionConfiguration(
        url: _url,
        deviceId: _deviceId,
        fingerprint: _fingerprint,
        instanceId: _instanceId,
        relayUrl: _relayUrl,
      ),
    );
    _setConfigurationStatus();
    notifyListeners();
    if (autoConnect) {
      connect();
    }
  }

  /// Revokes local pairing credentials and returns to unconfigured state.
  Future<void> unpair() async {
    disconnect();
    _instanceId = null;
    _relayUrl = null;
    _token = '';
    _fingerprint = null;
    await _tokenStore.deleteToken();
    await _configurationStore.save(
      const ConnectionConfiguration(),
    );
    _setConfigurationStatus();
    notifyListeners();
  }

  void connect() {
    final problem = _configurationError();
    if (problem != null) {
      _isActive = false;
      _setStatus(ConnectionStatus.configurationRequired, problem);
      return;
    }
    _isActive = true;
    _reconnectTimer?.cancel();
    _backoffSeconds = 1.0;
    _doConnect();
  }

  void disconnect() {
    _isActive = false;
    _reconnectTimer?.cancel();
    _reconnectTimer = null;
    // Reject any in-flight requests immediately rather than letting them time out.
    for (final completer in _pendingRequests.values) {
      if (!completer.isCompleted) {
        completer.completeError(StateError('Disconnected'));
      }
    }
    _pendingRequests.clear();
    _cleanupChannel();
    _activeTransport = null;
    _connectedEndpoint = null;
    _setStatus(
      isConfigurationComplete
          ? ConnectionStatus.disconnected
          : ConnectionStatus.configurationRequired,
      configurationError,
    );
  }

  void _setStatus(ConnectionStatus newStatus, [String? error]) {
    _status = newStatus;
    _lastError = error;
    notifyListeners();
  }

  void _setConfigurationStatus() {
    final problem = _configurationError();
    if (problem != null) {
      _isActive = false;
      _reconnectTimer?.cancel();
      _reconnectTimer = null;
      _cleanupChannel();
      _setStatus(ConnectionStatus.configurationRequired, problem);
    } else if (_status == ConnectionStatus.configurationRequired) {
      _setStatus(ConnectionStatus.disconnected);
    } else {
      notifyListeners();
    }
  }

  String? _configurationError() {
    if (_token.trim().isEmpty) return 'Authentication token is required.';
    if (_deviceId.trim().isEmpty) return 'Device identifier is required.';

    if (isPaired) {
      if (_relayUrl == null || _relayUrl!.trim().isEmpty) {
        return 'Paired device has no relay URL.';
      }
      return validateRelayUrl(_relayUrl!, allowLoopback: allowLoopbackRelay);
    }

    if (!allowLoopbackTestServer) {
      return 'Not paired. Scan the pairing QR code shown by agentdesk on your laptop.';
    }

    final url = _url.trim();
    if (url.isEmpty) return 'Server WebSocket URL is required.';
    final uri = Uri.tryParse(url);
    if (uri == null || !uri.hasScheme || uri.host.isEmpty) {
      return 'Enter a valid server WebSocket URL.';
    }
    if (uri.scheme != 'wss' && uri.scheme != 'ws') {
      return 'Server URL must use wss://, or loopback ws:// for insecure development.';
    }
    if (!isLoopbackHost(uri.host)) {
      return 'Direct connections are allowed only for loopback test servers; '
          'AgentDesk connects to laptops through the relay.';
    }
    if (uri.scheme == 'wss' &&
        (_fingerprint == null || _fingerprint!.isEmpty)) {
      return 'TLS connection requires a pinned SHA-256 certificate fingerprint in Settings.';
    }
    return null;
  }

  /// Loopback relays are a development-only configuration.
  bool get allowLoopbackRelay => allowLoopbackTestServer || kDebugMode;

  static String? _blankToNull(String? value) {
    final trimmed = value?.trim() ?? '';
    return trimmed.isEmpty ? null : trimmed;
  }

  Future<WebSocket> _connectTls(Uri uri) async {
    if (_fingerprint == null || _fingerprint!.trim().isEmpty) {
      throw const SocketException(
        'TLS connection requires a pinned SHA-256 certificate fingerprint in Settings',
      );
    }

    final client = HttpClient();
    client.badCertificateCallback = (cert, host, port) {
      final valid = verifyCertificateFingerprint(cert.der, _fingerprint!);
      if (!valid) {
        final presented = formatFingerprint(cert.der);
        debugPrint(
          'Rejecting certificate: fingerprint mismatch. Presented: $presented, Pinned: $_fingerprint',
        );
      }
      return valid;
    };
    return WebSocket.connect(uri.toString(), customClient: client);
  }

  Future<void> _doConnect() async {
    if (!_isActive) return;
    _cleanupChannel();
    _setStatus(ConnectionStatus.connecting);

    try {
      WebSocket? ws;
      TransportMode? currentTransport;
      String? endpoint;

      if (isPaired) {
        // Relay connection is the only transport
        if (_relayUrl != null && _relayUrl!.trim().isNotEmpty && _instanceId != null) {
          final cleanRelay = _relayUrl!.trim().replaceAll(RegExp(r'/+$'), '');
          final relayUri = Uri.parse(
            '$cleanRelay/ws?role=client'
            '&inst=${Uri.encodeQueryComponent(_instanceId!)}'
            '&device_id=${Uri.encodeQueryComponent(_deviceId)}'
            '&token=${Uri.encodeQueryComponent(_token)}',
          );
          try {
            debugPrint('Attempting relay connection to ${relayUri.host}...');
            if (relayUri.scheme == 'wss' && _fingerprint != null && _fingerprint!.trim().isNotEmpty) {
              try {
                ws = await WebSocket.connect(relayUri.toString()).timeout(relayConnectTimeout);
              } catch (_) {
                ws = await _connectTls(relayUri).timeout(relayConnectTimeout);
              }
            } else {
              ws = await WebSocket.connect(relayUri.toString()).timeout(relayConnectTimeout);
            }
            currentTransport = TransportMode.relay;
            endpoint = relayUri.replace(query: '').toString();
            debugPrint('Relay connection established');
          } catch (e) {
            debugPrint('Relay connection failed ($e)');
            ws = null;
          }
        }

        if (ws == null) {
          _onFailure('Could not connect to AgentDesk Relay.');
          return;
        }
      } else {
        // Test harness only: loopback daemon without a relay.
        final uri = Uri.parse(_url);
        if (!allowLoopbackTestServer || !isLoopbackHost(uri.host)) {
          _onFailure('AgentDesk connects only through the relay. Pair using the QR code.');
          return;
        }
        if (uri.scheme == 'wss') {
          ws = await _connectTls(uri);
          currentTransport = TransportMode.tls;
        } else {
          ws = await WebSocket.connect(uri.toString());
          currentTransport = TransportMode.insecureDev;
        }
        endpoint = uri.toString();
      }

      ws.pingInterval = relayPingInterval;

      _activeTransport = currentTransport;
      _connectedEndpoint = endpoint;
      _channel = IOWebSocketChannel(ws);
      _setStatus(ConnectionStatus.authenticating);

      // Listen to frames
      bool receivedWelcome = false;
      bool receivedSnapshot = false;

      _subscription = _channel!.stream.listen(
        (data) {
          try {
            final text = data is String ? data : utf8.decode(data as List<int>);
            final map = jsonDecode(text) as Map<String, dynamic>;
            final message = Message.fromJson(map);

            if (!receivedWelcome) {
              if (message.type == 'welcome') {
                receivedWelcome = true;
                final welcome = message.payload as Welcome;
                state.setWelcome(welcome);
                _activeTransport = welcome.transport;
                return;
              } else {
                _onFailure('Expected welcome, got ${message.type}');
                return;
              }
            }

            if (!receivedSnapshot) {
              if (message.type == 'snapshot') {
                receivedSnapshot = true;
                state.applySnapshot(message.payload as Snapshot);
                _backoffSeconds = 1.0;
                _setStatus(ConnectionStatus.connected);
                return;
              } else {
                _onFailure('Expected snapshot, got ${message.type}');
                return;
              }
            }

            _handleMessage(message);
          } catch (e) {
            // Ignore single malformed frames if connected
            debugPrint('Error parsing frame: $e');
          }
        },
        onError: (err) {
          _onFailure('WebSocket error: $err');
        },
        onDone: () {
          final closeCode = _channel?.closeCode;
          final closeReason = _channel?.closeReason;
          String msg = 'Connection closed';
          if (closeCode != null) {
            msg += ' ($closeCode: $closeReason)';
          }
          _onFailure(msg);
        },
        cancelOnError: true,
      );

      // Send hello
      final helloMsg = Message.push(
        'hello',
        Hello(
          token: _token,
          deviceId: _deviceId,
          clientVersion: '0.1.0',
          schemaVersion: schemaVersion,
        ),
      );
      _send(helloMsg);
    } catch (e) {
      _onFailure('Connection failed: $e');
    }
  }

  void _handleMessage(Message message) {
    if (message.requestId != null) {
      final completer = _pendingRequests.remove(message.requestId);
      if (completer != null && !completer.isCompleted) {
        completer.complete(message);
      }
    }

    switch (message.type) {
      case 'event':
        state.applyEvent(message.payload as EventPush);
        break;
      case 'score_update':
        state.applyScoreUpdate(message.payload as ScoreUpdate);
        break;
      case 'state_update':
        state.applyStateUpdate(message.payload as StateUpdate);
        break;
      case 'event_details':
        state.applyEventDetails(message.payload as EventPush);
        break;
      case 'snapshot':
        state.applySnapshot(message.payload as Snapshot);
        break;
      case 'welcome':
        state.setWelcome(message.payload as Welcome);
        break;
      case 'error':
        final err = message.payload as ErrorReply;
        debugPrint('Server error: ${err.code}: ${err.message}');
        break;
      case 'command_result':
        // Unsolicited command results (e.g. broadcast delivery confirmations)
        // are handled by the request_id matching above; nothing else to do.
        break;
    }
  }

  void _onFailure(String error) {
    _cleanupChannel();
    _activeTransport = null;
    _connectedEndpoint = null;
    for (final completer in _pendingRequests.values) {
      if (!completer.isCompleted) {
        completer.completeError(error);
      }
    }
    _pendingRequests.clear();

    if (!_isActive) {
      _setStatus(ConnectionStatus.disconnected);
      return;
    }

    _setStatus(ConnectionStatus.reconnecting, error);
    _scheduleReconnect();
  }

  void _scheduleReconnect() {
    _reconnectTimer?.cancel();
    // Jittered backoff between 1s and 30s
    final jitter = 0.8 + (0.4 * _random.nextDouble());
    final delayMs = (_backoffSeconds * jitter * 1000).toInt();
    _backoffSeconds = min(30.0, _backoffSeconds * 2.0);

    _reconnectTimer = Timer(Duration(milliseconds: delayMs), () {
      if (_isActive) {
        _doConnect();
      }
    });
  }

  void _cleanupChannel() {
    _subscription?.cancel();
    _subscription = null;
    try {
      _channel?.sink.close();
    } catch (_) {}
    _channel = null;
  }

  void _send(Message msg) {
    if (_channel != null) {
      final jsonStr = jsonEncode(msg.toJson());
      _channel!.sink.add(jsonStr);
    }
  }

  Future<Message> sendRequest(
    String type,
    dynamic payload, {
    Duration timeout = const Duration(seconds: 5),
  }) {
    if (_status != ConnectionStatus.connected) {
      return Future.error(StateError('Not connected to server'));
    }

    final reqId = 'r-${++_reqCounter}';
    final completer = Completer<Message>();
    _pendingRequests[reqId] = completer;

    final msg = Message.request(reqId, type, payload);
    _send(msg);

    return completer.future.timeout(
      timeout,
      onTimeout: () {
        _pendingRequests.remove(reqId);
        throw TimeoutException(
          'Request $type ($reqId) timed out after $timeout',
        );
      },
    );
  }

  Future<EventPush> getEventDetails(String eventId) async {
    final res = await sendRequest(
      'get_event_details',
      EventRef(eventId: eventId),
    );
    if (res.type == 'error') {
      final err = res.payload as ErrorReply;
      throw Exception('${err.code}: ${err.message}');
    }
    return res.payload as EventPush;
  }

  Future<EventLogs> getEventLogs(String eventId, int offset, int limit) async {
    final res = await sendRequest(
      'get_event_logs',
      GetEventLogs(eventId: eventId, offset: offset, limit: limit),
    );
    if (res.type == 'error') {
      final err = res.payload as ErrorReply;
      throw Exception('${err.code}: ${err.message}');
    }
    return res.payload as EventLogs;
  }

  Future<CommandResult> ack(String eventId) async {
    final res = await sendRequest('ack', EventRef(eventId: eventId));
    if (res.type == 'error') {
      return const CommandResult(ok: false, error: CommandError.invalid);
    }
    return res.payload as CommandResult;
  }

  Future<CommandResult> dismiss(String eventId) async {
    final res = await sendRequest('dismiss', EventRef(eventId: eventId));
    if (res.type == 'error') {
      return CommandResult(ok: false, error: CommandError.invalid);
    }
    return res.payload as CommandResult;
  }

  Future<CommandResult> respondRequest(
    String eventId,
    Decision decision, {
    List<String>? selectedOptions,
    String? textInput,
  }) async {
    final res = await sendRequest(
      'respond_request',
      RespondRequest(
        eventId: eventId,
        decision: decision,
        selectedOptions: selectedOptions,
        textInput: textInput,
      ),
    );
    if (res.type == 'error') {
      return CommandResult(ok: false, error: CommandError.invalid);
    }
    return res.payload as CommandResult;
  }

  Future<CommandResult> startTask({
    required String prompt,
    String? workspace,
    String? agentId,
  }) async {
    final res = await sendRequest(
      'start_task',
      StartTask(
        prompt: prompt,
        workspace: workspace,
        agentId: agentId,
      ),
    );
    if (res.type == 'error') {
      return const CommandResult(ok: false, error: CommandError.invalid);
    }
    return res.payload as CommandResult;
  }

  Future<Map<String, int>> getMetrics() async {
    final res = await sendRequest('get_metrics', const {});
    if (res.type == 'error') {
      final err = res.payload as ErrorReply;
      throw Exception('${err.code}: ${err.message}');
    }
    return res.payload as Map<String, int>;
  }

  @override
  void dispose() {
    disconnect();
    super.dispose();
  }
}
