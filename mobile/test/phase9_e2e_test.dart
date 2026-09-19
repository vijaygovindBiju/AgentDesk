import 'dart:convert';
import 'dart:io';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/client_metrics.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';

void main() {
  group('Phase 9 - Live End-to-End Validation Checklist Test (P9.1, P9.T2)', () {
    late Directory tempDir;
    late HttpServer secureServer;
    late int securePort;
    late String certDerFingerprint;
    WebSocket? activeSocket;

    setUp(() async {
      tempDir = Directory.systemTemp.createTempSync('agentdesk_p9_e2e');
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
          activeSocket = ws;
          ws.listen((data) {
            final map = jsonDecode(data.toString()) as Map<String, dynamic>;
            final type = map['type'] as String;
            final reqId = map['request_id'] as String?;

            if (type == 'hello') {
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
              // Send initial snapshot
              ws.add(jsonEncode({
                'type': 'snapshot',
                'payload': {
                  'entries': [
                    {
                      'event_id': 'work-task-1',
                      'tier': 3,
                      'score': 50,
                      'state': 'new',
                      'seq': 1,
                      'escalation_level': 0,
                    }
                  ],
                  'events': [
                    {
                      'schema_version': 1,
                      'event_id': 'work-task-1',
                      'seq': 1,
                      'agent_seq': 1,
                      'agent_id': 'builder-agent',
                      'agent_name': 'Builder Agent',
                      'project': 'AgentDesk',
                      'task_id': 'task-build-42',
                      'ts': '2026-09-19T12:00:00.000Z',
                      'category': 'working',
                      'severity': 1,
                      'kind': 'build_start',
                      'operation': 'build',
                      'summary': 'Building core workspace',
                      'message': 'Compiling crates...',
                      'details': {'crate_count': 5},
                      'log_range': {'start': 0, 'end': 10, 'pinned': false},
                    }
                  ]
                }
              }));
            } else if (type == 'get_event_details') {
              final payload = map['payload'] as Map<String, dynamic>;
              final eventId = payload['event_id'] as String;
              ws.add(jsonEncode({
                'type': 'event_details',
                'request_id': reqId,
                'payload': {
                  'event': {
                    'schema_version': 1,
                    'event_id': eventId,
                    'seq': 2,
                    'agent_seq': 2,
                    'agent_id': 'builder-agent',
                    'agent_name': 'Builder Agent',
                    'project': 'AgentDesk',
                    'task_id': 'task-db-mig',
                    'ts': '2026-09-19T12:01:00.000Z',
                    'category': 'request',
                    'severity': 3,
                    'kind': 'approval_required',
                    'operation': 'edit',
                    'summary': 'Apply database migration',
                    'message': 'Migration alters table user_sessions',
                    'details': {'alterations': 2, 'risk': 'medium'},
                    'log_range': {'start': 10, 'end': 20, 'pinned': true},
                    'request': {
                      'prompt': 'Run migration on production?',
                      'options': ['approve', 'deny'],
                    },
                  },
                  'entry': {
                    'event_id': eventId,
                    'tier': 0,
                    'score': 95,
                    'state': 'seen',
                    'resolution': 'unresolved',
                    'seq': 2,
                    'escalation_level': 0,
                  }
                }
              }));
            } else if (type == 'get_event_logs') {
              final payload = map['payload'] as Map<String, dynamic>;
              final eventId = payload['event_id'] as String;
              final offset = payload['offset'] as int;
              ws.add(jsonEncode({
                'type': 'event_logs',
                'request_id': reqId,
                'payload': {
                  'event_id': eventId,
                  'offset': offset == -1 ? 15 : offset,
                  'total': 20,
                  'evicted': false,
                  'lines': [
                    {'offset': 15, 'ts': '2026-09-19T12:01:01.000Z', 'text': 'ALTER TABLE user_sessions ADD COLUMN last_seen TIMESTAMP;'},
                    {'offset': 16, 'ts': '2026-09-19T12:01:02.000Z', 'text': 'Waiting for operator approval...'},
                  ]
                }
              }));
            } else if (type == 'respond_request') {
              ws.add(jsonEncode({
                'type': 'command_result',
                'request_id': reqId,
                'payload': {'ok': true}
              }));
            } else if (type == 'dismiss') {
              ws.add(jsonEncode({
                'type': 'command_result',
                'request_id': reqId,
                'payload': {'ok': true}
              }));
            }
          });
        }
      });
    });

    tearDown(() async {
      await activeSocket?.close();
      await secureServer.close(force: true);
      try {
        tempDir.deleteSync(recursive: true);
      } catch (_) {}
    });

    test('P9.1: Full checklist executes over wss:// with certificate pinning', () async {
      final state = AgentDeskState();
      final metrics = ClientMetrics();
      final conn = ConnectionService(state: state);

      // 1. Configure wss:// and pinned SHA-256 fingerprint
      conn.configure(
        url: 'wss://127.0.0.1:$securePort',
        token: 'secret-auth-token-1234',
        deviceId: 'real-test-device-android-01',
        fingerprint: certDerFingerprint,
      );

      // 2. Connect & Handshake (hello -> welcome -> snapshot)
      conn.connect();

      for (int i = 0; i < 40; i++) {
        await Future.delayed(const Duration(milliseconds: 50));
        if (conn.status == ConnectionStatus.connected) break;
      }
      expect(conn.status, ConnectionStatus.connected);
      expect(state.welcome?.transport, TransportMode.tls);

      // Snapshot received
      expect(state.queue.length, 1);
      expect(state.entriesForTier(3).first.eventId, 'work-task-1');
      expect(state.entriesForTier(3).first.escalationLevel, 0);

      // 3. See new events pushed from daemon
      activeSocket!.add(jsonEncode({
        'type': 'event',
        'payload': {
          'event': {
            'schema_version': 1,
            'event_id': 'req-mig-2',
            'seq': 2,
            'agent_seq': 2,
            'agent_id': 'builder-agent',
            'agent_name': 'Builder Agent',
            'project': 'AgentDesk',
            'task_id': 'task-db-mig',
            'ts': '2026-09-19T12:01:00.000Z',
            'category': 'request',
            'severity': 3,
            'kind': 'approval_required',
            'operation': 'edit',
            'summary': 'Apply database migration',
            'message': 'Migration alters table user_sessions',
            'details': {'alterations': 2, 'risk': 'medium'},
            'log_range': {'start': 10, 'end': 20, 'pinned': true},
            'request': {
              'prompt': 'Run migration on production?',
              'options': ['approve', 'deny'],
            },
          },
          'entry': {
            'event_id': 'req-mig-2',
            'tier': 0,
            'score': 95,
            'state': 'new',
            'resolution': 'unresolved',
            'seq': 2,
            'escalation_level': 0,
          }
        }
      }));

      await Future.delayed(const Duration(milliseconds: 60));
      expect(state.queue.length, 2);
      expect(state.entriesForTier(0).first.eventId, 'req-mig-2'); // Tier 0 Request is top!

      // 4. Watchdog badge on long-running task via score_update
      activeSocket!.add(jsonEncode({
        'type': 'score_update',
        'payload': {
          'event_id': 'work-task-1',
          'score': 65,
          'escalation_level': 1,
        }
      }));

      await Future.delayed(const Duration(milliseconds: 60));
      final workEntry = state.entryFor('work-task-1')!;
      expect(workEntry.escalationLevel, 1);
      expect(workEntry.score, 65);

      // 5. Open details (Level 2 details)
      metrics.recordTap();
      final detailsPush = await conn.getEventDetails('req-mig-2');
      expect(detailsPush.event.details['alterations'], 2);
      expect(state.entryFor('req-mig-2')?.state, EntryState.seen);

      // 6. Page logs (tail-first + backward)
      metrics.recordLogPageRequested();
      final logsTail = await conn.getEventLogs('req-mig-2', -1, 10);
      expect(logsTail.lines.length, 2);
      expect(logsTail.lines.last.text, 'Waiting for operator approval...');

      // 7. Approve request
      final approveRes = await conn.respondRequest('req-mig-2', Decision.approve);
      expect(approveRes.ok, true);

      // 8. See completion: task finishes and supersedes working entry
      activeSocket!.add(jsonEncode({
        'type': 'event',
        'payload': {
          'event': {
            'schema_version': 1,
            'event_id': 'comp-mig-3',
            'seq': 3,
            'agent_seq': 3,
            'agent_id': 'builder-agent',
            'agent_name': 'Builder Agent',
            'project': 'AgentDesk',
            'task_id': 'task-db-mig',
            'ts': '2026-09-19T12:02:00.000Z',
            'category': 'completed',
            'severity': 2,
            'kind': 'migration_complete',
            'operation': 'edit',
            'summary': 'Database migration completed',
            'message': 'Migration finished successfully',
            'details': {},
            'log_range': {'start': 20, 'end': 30, 'pinned': false},
          },
          'entry': {
            'event_id': 'comp-mig-3',
            'tier': 2,
            'score': 70,
            'state': 'new',
            'seq': 3,
            'escalation_level': 0,
          }
        }
      }));

      await Future.delayed(const Duration(milliseconds: 60));
      expect(state.entryFor('comp-mig-3')?.tier, 2);

      // 9. Dismiss entry
      final dismissRes = await conn.dismiss('comp-mig-3');
      expect(dismissRes.ok, true);

      // 10. Reconnect after network drop (simulated airplane mode)
      await activeSocket!.close(1001, 'Going away');
      // Wait for client to notice disconnect and enter reconnecting
      for (int i = 0; i < 20; i++) {
        await Future.delayed(const Duration(milliseconds: 50));
        if (conn.status == ConnectionStatus.reconnecting) break;
      }
      expect(conn.status, ConnectionStatus.reconnecting);

      // Reconnect immediately
      conn.connect();
      for (int i = 0; i < 40; i++) {
        await Future.delayed(const Duration(milliseconds: 50));
        if (conn.status == ConnectionStatus.connected) break;
      }
      expect(conn.status, ConnectionStatus.connected);

      conn.disconnect();
    });
  });
}
