import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';

void main() {
  group('ConnectionService tests', () {
    late HttpServer server;
    late int port;

    setUp(() async {
      server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
      port = server.port;
    });

    tearDown(() async {
      await server.close(force: true);
    });

    test('performs full handshake and receives pushed messages', () async {
      final state = AgentDeskState();
      final conn = ConnectionService(state: state);
      conn.configure(
        url: 'ws://127.0.0.1:$port',
        token: 'test-token',
        deviceId: 'device-test',
      );

      final serverReceivedFrames = <String>[];
      final serverHandshakeDone = Completer<void>();

      server.listen((HttpRequest request) async {
        if (WebSocketTransformer.isUpgradeRequest(request)) {
          final ws = await WebSocketTransformer.upgrade(request);
          ws.listen((data) {
            final text = data.toString();
            serverReceivedFrames.add(text);
            final map = jsonDecode(text) as Map<String, dynamic>;
            if (map['type'] == 'hello') {
              // Send welcome
              ws.add(jsonEncode({
                'type': 'welcome',
                'payload': {
                  'daemon_version': '0.1.0',
                  'schema_version': 1,
                  'pipeline_mode': 'agentdesk',
                  'transport': 'insecure_dev',
                  'server_time': '2026-09-19T12:00:00.000Z',
                }
              }));

              // Send snapshot
              ws.add(jsonEncode({
                'type': 'snapshot',
                'payload': {
                  'entries': [
                    {
                      'event_id': 'snap-1',
                      'tier': 0,
                      'score': 85,
                      'state': 'new',
                      'resolution': 'unresolved',
                      'seq': 1
                    }
                  ],
                  'events': [
                    {
                      'schema_version': 1,
                      'event_id': 'snap-1',
                      'seq': 1,
                      'agent_seq': 1,
                      'agent_id': 'ag-1',
                      'agent_name': 'Agent 1',
                      'project': 'Proj',
                      'ts': '2026-09-19T12:00:00.000Z',
                      'category': 'request',
                      'severity': 2,
                      'kind': 'approval_required',
                      'operation': 'other',
                      'summary': 'Approve DB migration',
                      'message': 'Migration needs confirmation',
                      'details': {},
                      'log_range': {'start': 0, 'end': 2, 'pinned': true},
                    }
                  ]
                }
              }));

              if (!serverHandshakeDone.isCompleted) {
                serverHandshakeDone.complete();
              }
            } else if (map['type'] == 'ack') {
              final reqId = map['request_id'];
              ws.add(jsonEncode({
                'type': 'command_result',
                'request_id': reqId,
                'payload': {'ok': true},
              }));
            }
          });
        }
      });

      conn.connect();

      // Wait until handshake completes
      await serverHandshakeDone.future.timeout(const Duration(seconds: 3));

      // Wait a tick for client to process snapshot
      await Future.delayed(const Duration(milliseconds: 100));

      expect(conn.status, ConnectionStatus.connected);
      expect(state.welcome?.pipelineMode, PipelineMode.agentdesk);
      expect(state.entries.length, 1);
      expect(state.entries.first.eventId, 'snap-1');

      // Test request-reply correlation
      final ackResult = await conn.ack('snap-1');
      expect(ackResult.ok, isTrue);

      conn.disconnect();
      expect(conn.status, ConnectionStatus.disconnected);
    });

    test('reconnects with backoff when server closes connection', () async {
      final state = AgentDeskState();
      final conn = ConnectionService(state: state);
      conn.configure(
        url: 'ws://127.0.0.1:$port',
        token: 'test-token',
        deviceId: 'device-test',
      );

      final connectedCompleter = Completer<WebSocket>();

      server.listen((HttpRequest request) async {
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
                  'transport': 'insecure_dev',
                  'server_time': '2026-09-19T12:00:00.000Z',
                }
              }));
              ws.add(jsonEncode({
                'type': 'snapshot',
                'payload': {'entries': [], 'events': []}
              }));
              if (!connectedCompleter.isCompleted) {
                connectedCompleter.complete(ws);
              }
            }
          });
        }
      });

      conn.connect();
      final ws = await connectedCompleter.future.timeout(const Duration(seconds: 3));
      await Future.delayed(const Duration(milliseconds: 50));
      expect(conn.status, ConnectionStatus.connected);

      // Close the socket from the server side
      await ws.close();
      await Future.delayed(const Duration(milliseconds: 100));

      // Client should detect and transition to reconnecting
      expect(conn.status, ConnectionStatus.reconnecting);

      conn.disconnect();
      expect(conn.status, ConnectionStatus.disconnected);
    });
  });
}
