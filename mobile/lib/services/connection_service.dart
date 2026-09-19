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
  static const defaultSecureUrl = 'wss://127.0.0.1:8765';

  final AgentDeskState state;
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

  String _url = defaultSecureUrl;
  String _token = '';
  String _deviceId = 'flutter-client-1';
  String? _fingerprint;

  String get url => _url;
  String get token => _token;
  String get deviceId => _deviceId;
  String? get fingerprint => _fingerprint;

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
      _url = stored.url?.trim() ?? defaultSecureUrl;
      _deviceId = stored.deviceId?.trim().isEmpty ?? true
          ? 'flutter-client-1'
          : stored.deviceId!.trim();
      _fingerprint = _blankToNull(stored.fingerprint);
      _token = token?.trim() ?? '';
      _setConfigurationStatus();
    } catch (_) {
      _isActive = false;
      _setStatus(
        ConnectionStatus.configurationRequired,
        'Unable to read saved connection configuration. Open Settings and save it again.',
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
    _cleanupChannel();
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
    final url = _url.trim();
    if (url.isEmpty) return 'Server WebSocket URL is required.';
    final uri = Uri.tryParse(url);
    if (uri == null || !uri.hasScheme || uri.host.isEmpty) {
      return 'Enter a valid server WebSocket URL.';
    }
    if (uri.scheme != 'wss' && uri.scheme != 'ws') {
      return 'Server URL must use wss://, or loopback ws:// for insecure development.';
    }
    if (uri.scheme == 'ws' && !_isLoopbackHost(uri.host)) {
      return 'Plain ws:// is allowed only for loopback insecure-development servers.';
    }
    if (_token.trim().isEmpty) return 'Authentication token is required.';
    if (_deviceId.trim().isEmpty) return 'Device identifier is required.';
    if (uri.scheme == 'wss' &&
        (_fingerprint == null || _fingerprint!.isEmpty)) {
      return 'TLS connection requires a pinned SHA-256 certificate fingerprint in Settings.';
    }
    return null;
  }

  static bool _isLoopbackHost(String host) =>
      host == '127.0.0.1' || host == '::1' || host.toLowerCase() == 'localhost';

  static String? _blankToNull(String? value) {
    final trimmed = value?.trim() ?? '';
    return trimmed.isEmpty ? null : trimmed;
  }

  Future<void> _doConnect() async {
    if (!_isActive) return;
    _cleanupChannel();
    _setStatus(ConnectionStatus.connecting);

    try {
      final uri = Uri.parse(_url);
      WebSocket ws;
      if (uri.scheme == 'wss') {
        if (_fingerprint == null || _fingerprint!.trim().isEmpty) {
          _onFailure(
            'TLS connection requires a pinned SHA-256 certificate fingerprint in Settings',
          );
          return;
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
        ws = await WebSocket.connect(uri.toString(), customClient: client);
      } else if (uri.scheme == 'ws' && _isLoopbackHost(uri.host)) {
        ws = await WebSocket.connect(uri.toString());
      } else {
        _onFailure(
          'Plain ws:// is only permitted for loopback insecure-development servers',
        );
        return;
      }

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
                state.setWelcome(message.payload as Welcome);
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
    }
  }

  void _onFailure(String error) {
    _cleanupChannel();
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
    Decision decision,
  ) async {
    final res = await sendRequest(
      'respond_request',
      RespondRequest(eventId: eventId, decision: decision),
    );
    if (res.type == 'error') {
      return CommandResult(ok: false, error: CommandError.invalid);
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
