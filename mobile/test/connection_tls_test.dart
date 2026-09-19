import 'dart:convert';
import 'dart:io';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';

void main() {
  group('P8.T3 - TLS and certificate pinning integration test', () {
    late Directory tempDir;
    late HttpServer secureServer;
    late int securePort;
    late String certDerFingerprint;

    setUp(() async {
      tempDir = Directory.systemTemp.createTempSync('agentdesk_tls_test');
      final keyPath = '${tempDir.path}/key.pem';
      final certPath = '${tempDir.path}/cert.pem';
      final derPath = '${tempDir.path}/cert.der';

      // Generate self-signed cert and key via openssl
      Process.runSync('openssl', [
        'req', '-x509', '-newkey', 'rsa:2048',
        '-keyout', keyPath,
        '-out', certPath,
        '-days', '1', '-nodes',
        '-subj', '/CN=127.0.0.1',
      ]);

      // Convert to DER to get exact fingerprint
      Process.runSync('openssl', [
        'x509', '-in', certPath, '-out', derPath, '-outform', 'DER',
      ]);

      final derBytes = File(derPath).readAsBytesSync();
      certDerFingerprint = sha256.convert(derBytes).toString();

      final secContext = SecurityContext();
      secContext.useCertificateChain(certPath);
      secContext.usePrivateKey(keyPath);

      secureServer = await HttpServer.bindSecure(
        InternetAddress.loopbackIPv4,
        0,
        secContext,
      );
      securePort = secureServer.port;

      secureServer.listen((HttpRequest request) async {
        if (WebSocketTransformer.isUpgradeRequest(request)) {
          final ws = await WebSocketTransformer.upgrade(request);
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
                  'server_time': '2026-09-19T12:00:00.000Z',
                }
              }));
              ws.add(jsonEncode({
                'type': 'snapshot',
                'payload': {'entries': [], 'events': []}
              }));
            }
          });
        }
      });
    });

    tearDown(() async {
      await secureServer.close(force: true);
      try {
        tempDir.deleteSync(recursive: true);
      } catch (_) {}
    });

    test('client with pinned fingerprint connects over wss://', () async {
      final state = AgentDeskState();
      final conn = ConnectionService(state: state);
      conn.configure(
        url: 'wss://127.0.0.1:$securePort',
        token: 'test-token',
        deviceId: 'device-test',
        fingerprint: certDerFingerprint,
      );

      conn.connect();

      for (int i = 0; i < 40; i++) {
        await Future.delayed(const Duration(milliseconds: 50));
        if (conn.status == ConnectionStatus.connected) break;
      }

      expect(conn.status, ConnectionStatus.connected);
      expect(state.welcome?.transport, TransportMode.tls);

      conn.disconnect();
    });

    test('client with mismatched fingerprint fails handshake', () async {
      final state = AgentDeskState();
      final conn = ConnectionService(state: state);
      // Pass wrong fingerprint
      final wrongFingerprint = '00' * 32;
      conn.configure(
        url: 'wss://127.0.0.1:$securePort',
        token: 'test-token',
        deviceId: 'device-test',
        fingerprint: wrongFingerprint,
      );

      conn.connect();

      for (int i = 0; i < 20; i++) {
        await Future.delayed(const Duration(milliseconds: 50));
        if (conn.status == ConnectionStatus.reconnecting || conn.status == ConnectionStatus.error) {
          break;
        }
      }

      // Connection MUST not be connected
      expect(conn.status, isNot(ConnectionStatus.connected));
      expect(conn.lastError, isNotNull);

      conn.disconnect();
    });

    test('wss without fingerprint is rejected immediately', () async {
      final state = AgentDeskState();
      final conn = ConnectionService(state: state);
      conn.configure(
        url: 'wss://127.0.0.1:$securePort',
        token: 'test-token',
        deviceId: 'device-test',
        fingerprint: '',
      );

      conn.connect();
      await Future.delayed(const Duration(milliseconds: 50));

      expect(conn.status, isNot(ConnectionStatus.connected));
      expect(conn.lastError, contains('requires a pinned SHA-256 certificate fingerprint'));

      conn.disconnect();
    });
  });
}
