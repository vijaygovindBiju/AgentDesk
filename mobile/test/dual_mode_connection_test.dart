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
  group('Dual-Mode Connection Tests (Direct LAN + Relay Fallback)', () {
    late HttpServer lanServer;
    late int lanPort;
    late HttpServer relayServer;
    late int relayPort;

    setUp(() async {
      lanServer = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      lanPort = lanServer.port;
      relayServer = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      relayPort = relayServer.port;
    });

    tearDown(() async {
      await lanServer.close(force: true);
      await relayServer.close(force: true);
    });

    test('connects directly to LAN when LAN endpoint is reachable', () async {
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
          ws.listen((data) {
            final map = jsonDecode(data.toString()) as Map<String, dynamic>;
            if (map['type'] == 'hello') {
              ws.add(jsonEncode({
                'type': 'welcome',
                'payload': {
                  'daemon_version': '0.1.0',
                  'schema_version': 1,
                  'pipeline_mode': 'agentdesk',
                  'transport': 'tls',
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

      await conn.savePairing(
        instanceId: 'inst-lan-test',
        deviceToken: 'device-token-lan',
        deviceId: 'phone-test',
        lanAddress: 'ws://127.0.0.1:$lanPort',
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

      expect(lanConnected, isTrue);
      expect(conn.status, ConnectionStatus.connected);
      expect(conn.activeTransport, TransportMode.tls);

      conn.disconnect();
    });

    test('falls back to Relay when LAN endpoint is unreachable', () async {
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

      // Close LAN server to simulate dead/unreachable LAN
      await lanServer.close(force: true);

      await conn.savePairing(
        instanceId: 'inst-relay-fallback',
        deviceToken: 'device-token-relay',
        deviceId: 'phone-test-relay',
        lanAddress: '127.0.0.1:$lanPort', // Closed port -> connection refused immediately
        fingerprint: 'dummy-fingerprint',
        relayUrl: 'ws://127.0.0.1:$relayPort',
        autoConnect: false,
      );

      expect(conn.isPaired, isTrue);

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
      expect(requestedRelayPath, contains('inst=inst-relay-fallback'));
      expect(conn.status, ConnectionStatus.connected);
      expect(conn.activeTransport, TransportMode.relay);

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
        lanAddress: '192.168.1.50:8765',
        fingerprint: 'fingerprint-abc',
        relayUrl: 'wss://relay.example.com',
        autoConnect: false,
      );

      expect(conn.isPaired, isTrue);
      expect(await tokenStore.readToken(), 'token-unpair');

      await conn.unpair();

      expect(conn.isPaired, isFalse);
      expect(conn.instanceId, isNull);
      expect(conn.lanAddress, isNull);
      expect(conn.relayUrl, isNull);
      expect(conn.token, isEmpty);
      expect(await tokenStore.readToken(), isNull);
      expect(conn.status, ConnectionStatus.configurationRequired);
    });
  });
}
