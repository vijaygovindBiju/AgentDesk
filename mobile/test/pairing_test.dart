import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/message.dart';
import 'package:agentdesk/services/connection_configuration_store.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/services/pairing_client.dart';
import 'package:agentdesk/services/secure_token_store.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:agentdesk/ui/qr_scan_screen.dart';
import 'package:agentdesk/ui/settings_screen.dart';

class MockSecureTokenStore implements SecureTokenStore {
  String? _token;
  @override
  Future<String?> readToken() async => _token;
  @override
  Future<void> writeToken(String token) async => _token = token;
  @override
  Future<void> deleteToken() async => _token = null;
}

class MockConnectionConfigurationStore implements ConnectionConfigurationStore {
  ConnectionConfiguration _config = const ConnectionConfiguration();
  @override
  Future<ConnectionConfiguration> load() async => _config;
  @override
  Future<void> save(ConnectionConfiguration configuration) async =>
      _config = configuration;
}

class FakePairingClient extends PairingClient {
  final PairingResult result;
  PairingPayload? lastPayload;

  FakePairingClient({required this.result});

  @override
  Future<PairingResult> pair({
    required PairingPayload payload,
    required String deviceId,
    required String deviceName,
    Duration timeout = const Duration(seconds: 15),
  }) async {
    lastPayload = payload;
    return result;
  }
}

void main() {
  group('PairingPayload URI parsing', () {
    test('round trips valid URI with lan and relay', () {
      const payload = PairingPayload(
        version: 1,
        instanceId: 'inst-laptop-1',
        sessionId: 'sess-uuid-1',
        pairingSecret:
            '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
        fingerprint: 'AA:BB:CC:DD',
        lanAddress: '192.168.1.100:8765',
        relayUrl: 'wss://relay.agentdesk.net',
      );

      final uri = payload.toUriString();
      expect(uri, startsWith('agentdesk://pair?'));

      final parsed = PairingPayload.parse(uri);
      expect(parsed.version, 1);
      expect(parsed.instanceId, 'inst-laptop-1');
      expect(parsed.sessionId, 'sess-uuid-1');
      expect(parsed.pairingSecret, payload.pairingSecret);
      expect(parsed.fingerprint, 'AA:BB:CC:DD');
      expect(parsed.lanAddress, '192.168.1.100:8765');
      expect(parsed.relayUrl, 'wss://relay.agentdesk.net');
    });

    test('rejects invalid scheme', () {
      const badScheme =
          'http://pair?v=1&inst=i&sess=s&sec=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef&fp=AA';
      expect(() => PairingPayload.parse(badScheme), throwsFormatException);
    });

    test('rejects unsupported protocol version', () {
      const badVersion =
          'agentdesk://pair?v=2&inst=i&sess=s&sec=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef&fp=AA';
      expect(() => PairingPayload.parse(badVersion), throwsFormatException);
    });

    test('rejects invalid hex secret', () {
      const badSecret =
          'agentdesk://pair?v=1&inst=i&sess=s&sec=not-a-valid-hex-secret&fp=AA';
      expect(() => PairingPayload.parse(badSecret), throwsFormatException);
    });

    test('rejects missing required fields', () {
      const missingFp =
          'agentdesk://pair?v=1&inst=i&sess=s&sec=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';
      expect(() => PairingPayload.parse(missingFp), throwsFormatException);
    });
  });

  group('QRScanScreen and Pairing Flow', () {
    testWidgets('simulates QR scan and completes pairing', (tester) async {
      final tokenStore = MockSecureTokenStore();
      final configStore = MockConnectionConfigurationStore();
      final state = AgentDeskState();
      final connection = ConnectionService(
        state: state,
        tokenStore: tokenStore,
        configurationStore: configStore,
      );

      final fakeClient = FakePairingClient(
        result: const PairingResult.success(
          instanceId: 'inst-laptop-1',
          deviceToken: 'dev-token-999',
          lanAddress: '192.168.1.50:8765',
          fingerprint: '11:22:33:44:55:66',
        ),
      );

      await tester.pumpWidget(
        MaterialApp(
          home: QRScanScreen(
            connection: connection,
            pairingClient: fakeClient,
            autoConnect: false,
          ),
        ),
      );

      expect(find.text('Pair with Laptop'), findsOneWidget);

      // Open manual simulation dialog
      await tester.tap(find.byKey(const Key('simulate_qr_button')));
      await tester.pumpAndSettle();

      const sampleUri =
          'agentdesk://pair?v=1&inst=inst-laptop-1&sess=sess-123&sec=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef&fp=11:22:33:44:55:66&lan=192.168.1.50:8765';

      await tester.enterText(
        find.byKey(const Key('simulated_qr_input')),
        sampleUri,
      );
      await tester.tap(find.byKey(const Key('simulated_qr_submit')));
      await tester.pumpAndSettle();

      // Verify credentials were saved to stores and connection updated
      expect(connection.isPaired, isTrue);
      expect(connection.instanceId, 'inst-laptop-1');
      expect(connection.url, 'wss://192.168.1.50:8765');
      expect(connection.token, 'dev-token-999');
      expect(await tokenStore.readToken(), 'dev-token-999');
    });

    testWidgets('SettingsScreen displays paired state and unpairs', (tester) async {
      final tokenStore = MockSecureTokenStore();
      final configStore = MockConnectionConfigurationStore();
      final state = AgentDeskState();
      final connection = ConnectionService(
        state: state,
        tokenStore: tokenStore,
        configurationStore: configStore,
      );

      // Pre-pair device
      await connection.savePairing(
        instanceId: 'inst-macbook-pro',
        deviceToken: 'token-abc',
        deviceId: 'pixel-8',
        lanAddress: '10.0.0.42:8765',
        fingerprint: 'AA:BB:CC',
        autoConnect: false,
      );

      await tester.pumpWidget(
        MaterialApp(
          home: SettingsScreen(connection: connection),
        ),
      );

      expect(find.text('Paired with Laptop'), findsOneWidget);
      expect(find.text('Instance: inst-macbook-pro'), findsOneWidget);
      expect(find.byKey(const Key('unpair_button')), findsOneWidget);

      // Tap unpair and confirm
      await tester.tap(find.byKey(const Key('unpair_button')));
      await tester.pumpAndSettle();

      expect(find.text('Unpair Phone?'), findsOneWidget);
      await tester.tap(find.byKey(const Key('confirm_unpair_button')));
      await tester.pumpAndSettle();

      expect(connection.isPaired, isFalse);
      expect(connection.token, isEmpty);
      expect(await tokenStore.readToken(), isNull);
      expect(find.text('Pair with Laptop'), findsOneWidget);
      expect(find.byKey(const Key('scan_qr_button')), findsOneWidget);
    });
  });
}
