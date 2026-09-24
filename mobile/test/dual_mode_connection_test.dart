import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/connection_configuration_store.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/services/secure_token_store.dart';
import 'package:agentdesk/state/agentdesk_state.dart';

class MemoryConfigStore implements ConnectionConfigurationStore {
  ConnectionConfiguration value = const ConnectionConfiguration();
  @override
  Future<ConnectionConfiguration> load() async => value;
  @override
  Future<void> save(ConnectionConfiguration configuration) async {
    value = configuration;
  }
}

class MemoryTokenStore implements SecureTokenStore {
  String? value;
  @override
  Future<void> deleteToken() async => value = null;
  @override
  Future<String?> readToken() async => value;
  @override
  Future<void> writeToken(String token) async => value = token;
}

void main() {
  group('Relay-Only Connection Tests (No LAN Fallback)', () {
    late HttpServer lanServer;
    late HttpServer relayServer;
    late int relayPort;

    setUp(() async {
      lanServer = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      relayServer = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      relayPort = relayServer.port;
    });

    tearDown(() async {
      await lanServer.close(force: true);
      await relayServer.close(force: true);
    });

    test('connects to Relay as sole remote transport when relay endpoint is reachable', () async {
      final state = AgentDeskState();
      final configStore = MemoryConfigStore();
      final tokenStore = MemoryTokenStore();
      final conn = ConnectionService(
        state: state,
        configurationStore: configStore,
        tokenStore: tokenStore,
      );

      bool relayConnected = false;
      String? requestedRelayPath;
      relayServer.listen((HttpRequest request) async {
        requestedRelayPath = request.uri.toString();
        if (WebSocketTransformer.isUpgradeRequest(request)) {
          final ws = await WebSocketTransformer.upgrade(request);
          relayConnected = true;
          ws.listen((data) {
            final map = jsonDecode(data.toString()) as Map<String, dynamic>;
            if (map['type'] == 'hello') {
              ws.add(jsonEncode({
                'type': 'welcome',
                'payload': {
                  'daemon_version': '0.1.0',
                  'schema_version': 1,
                  'pipeline_mode': 'agentdesk',
                  'transport': 'relay',
                  'server_time': '2026-09-21T12:00:00.000Z',
                }
              }));
              ws.add(jsonEncode({
                'type': 'snapshot',
                'payload': {
                  'entries': [],
                  'events': [],
                }
              }));
            }
          });
        }
      });

      bool lanConnected = false;
      lanServer.listen((HttpRequest request) async {
        if (WebSocketTransformer.isUpgradeRequest(request)) {
          final ws = await WebSocketTransformer.upgrade(request);
          lanConnected = true;
          ws.listen((_) {});
        }
      });

      await conn.savePairing(
        instanceId: 'inst-relay-primary',
        deviceToken: 'device-token-primary',
        deviceId: 'phone-test',
        fingerprint: 'dummy-fingerprint',
        relayUrl: 'ws://127.0.0.1:$relayPort',
        autoConnect: false,
      );

      expect(conn.isPaired, isTrue);
      expect(conn.isConfigurationComplete, isTrue);

      conn.connect();

      // Wait until connected
      final connectedCompleter = Completer<void>();
      void listener() {
        if (conn.status == ConnectionStatus.connected) {
          if (!connectedCompleter.isCompleted) {
            connectedCompleter.complete();
          }
        }
      }
      conn.addListener(listener);
      await connectedCompleter.future.timeout(const Duration(seconds: 4));
      conn.removeListener(listener);

      expect(relayConnected, isTrue);
      expect(requestedRelayPath, contains('role=client'));
      expect(requestedRelayPath, contains('inst=inst-relay-primary'));
      expect(lanConnected, isFalse, reason: 'Relay succeeded; no direct LAN connection attempted');
      expect(conn.status, ConnectionStatus.connected);
      expect(conn.activeTransport, TransportMode.relay);

      conn.disconnect();
    });

    test('does NOT fall back to direct LAN when Relay is unreachable', () async {
      final state = AgentDeskState();
      final configStore = MemoryConfigStore();
      final tokenStore = MemoryTokenStore();
      final conn = ConnectionService(
        state: state,
        configurationStore: configStore,
        tokenStore: tokenStore,
      );

      bool lanConnected = false;
      lanServer.listen((HttpRequest request) async {
        if (WebSocketTransformer.isUpgradeRequest(request)) {
          final ws = await WebSocketTransformer.upgrade(request);
          lanConnected = true;
          ws.listen((_) {});
        }
      });

      // Close Relay server to simulate unreachable relay
      await relayServer.close(force: true);

      await conn.savePairing(
        instanceId: 'inst-relay-down',
        deviceToken: 'device-token-relay-down',
        deviceId: 'phone-test',
        fingerprint: 'dummy-fingerprint',
        relayUrl: 'ws://127.0.0.1:$relayPort', // Closed port -> connection refused immediately
        autoConnect: false,
      );

      expect(conn.isPaired, isTrue);

      conn.connect();

      // Wait until failure state (reconnecting or error)
      final failureCompleter = Completer<void>();
      void listener() {
        if (conn.status == ConnectionStatus.reconnecting ||
            conn.status == ConnectionStatus.error) {
          if (!failureCompleter.isCompleted) {
            failureCompleter.complete();
          }
        }
      }
      conn.addListener(listener);
      await failureCompleter.future.timeout(const Duration(seconds: 4));
      conn.removeListener(listener);

      // Verify that LAN was NEVER attempted
      expect(lanConnected, isFalse, reason: 'Direct LAN fallback must NEVER be attempted');
      expect(
        conn.status == ConnectionStatus.reconnecting || conn.status == ConnectionStatus.error,
        isTrue,
      );
      expect(conn.lastError, contains('Could not connect to AgentDesk Relay'));

      conn.disconnect();
    });

    test('unpair clears credentials and returns to unconfigured state', () async {
      final state = AgentDeskState();
      final configStore = MemoryConfigStore();
      final tokenStore = MemoryTokenStore();
      final conn = ConnectionService(
        state: state,
        configurationStore: configStore,
        tokenStore: tokenStore,
      );

      await conn.savePairing(
        instanceId: 'inst-unpair-test',
        deviceToken: 'token-unpair',
        deviceId: 'phone-unpair',
        fingerprint: 'fingerprint-abc',
        relayUrl: 'wss://relay.example.com',
        autoConnect: false,
      );

      expect(conn.isPaired, isTrue);
      expect(await tokenStore.readToken(), 'token-unpair');

      await conn.unpair();

      expect(conn.isPaired, isFalse);
      expect(conn.instanceId, isNull);
      expect(conn.relayUrl, isNull);
      expect(conn.token, isEmpty);
      expect(await tokenStore.readToken(), isNull);
      expect(conn.status, ConnectionStatus.configurationRequired);
    });
  });
}
