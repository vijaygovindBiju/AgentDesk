import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';

void main() {
  group('P7.T1 - Model decode tests for every message type', () {
    test('welcome message decodes and round-trips', () {
      final jsonStr = '''
      {
        "type": "welcome",
        "payload": {
          "daemon_version": "0.1.0",
          "schema_version": 1,
          "pipeline_mode": "agentdesk",
          "transport": "tls",
          "server_time": "2026-09-17T10:32:05.000Z"
        }
      }
      ''';
      final map = jsonDecode(jsonStr) as Map<String, dynamic>;
      final msg = Message.fromJson(map);
      expect(msg.type, 'welcome');
      expect(msg.requestId, isNull);
      final welcome = msg.payload as Welcome;
      expect(welcome.daemonVersion, '0.1.0');
      expect(welcome.schemaVersion, 1);
      expect(welcome.pipelineMode, PipelineMode.agentdesk);
      expect(welcome.transport, TransportMode.tls);
      expect(welcome.serverTime, DateTime.parse("2026-09-17T10:32:05.000Z"));

      final roundTrip = Message.fromJson(msg.toJson());
      final rtWelcome = roundTrip.payload as Welcome;
      expect(rtWelcome.daemonVersion, welcome.daemonVersion);
      expect(rtWelcome.pipelineMode, welcome.pipelineMode);
    });

    test('snapshot message decodes entries and events', () {
      final jsonStr = '''
      {
        "type": "snapshot",
        "payload": {
          "entries": [
            {
              "event_id": "00000000-0000-0000-0000-000000000000",
              "tier": 0,
              "score": 90,
              "state": "new",
              "resolution": "unresolved",
              "superseded": false,
              "escalation_level": 0,
              "seq": 1
            }
          ],
          "events": [
            {
              "schema_version": 1,
              "event_id": "00000000-0000-0000-0000-000000000000",
              "seq": 1,
              "agent_seq": 1,
              "agent_id": "a",
              "agent_name": "Agent A",
              "project": "Proj",
              "task_id": "task-1",
              "ts": "2026-09-17T10:32:05.000Z",
              "category": "request",
              "severity": 3,
              "kind": "approval_required",
              "operation": "other",
              "summary": "Need approval",
              "message": "Allow schema migration?",
              "details": {"table": "users"},
              "log_range": {"start": 0, "end": 3, "pinned": true},
              "request": {
                "prompt": "Allow migration?",
                "options": ["approve", "deny"]
              }
            }
          ]
        }
      }
      ''';
      final map = jsonDecode(jsonStr) as Map<String, dynamic>;
      final msg = Message.fromJson(map);
      expect(msg.type, 'snapshot');
      final snapshot = msg.payload as Snapshot;
      expect(snapshot.entries.length, 1);
      expect(snapshot.events.length, 1);

      final entry = snapshot.entries.first;
      expect(entry.eventId, '00000000-0000-0000-0000-000000000000');
      expect(entry.tier, 0);
      expect(entry.score, 90);
      expect(entry.state, EntryState.new_);
      expect(entry.resolution, Resolution.unresolved);
      expect(entry.isLive, isTrue);

      final event = snapshot.events.first;
      expect(event.category, Category.request);
      expect(event.severity, Severity.critical);
      expect(event.operation, Operation.other);
      expect(event.logRange.pinned, isTrue);
      expect(event.request?.options, ['approve', 'deny']);
    });

    test('event and score_update and state_update decodes', () {
      final eventPushJson = {
        'type': 'event',
        'payload': {
          'entry': {
            'event_id': 'id-1',
            'tier': 1,
            'score': 80,
            'state': 'new',
            'seq': 5,
          },
          'event': {
            'schema_version': 1,
            'event_id': 'id-1',
            'seq': 5,
            'agent_seq': 3,
            'agent_id': 'ag-1',
            'agent_name': 'Ag 1',
            'project': 'Pr 1',
            'ts': '2026-09-17T10:32:05.000Z',
            'category': 'error',
            'severity': 2,
            'kind': 'compile_error',
            'operation': 'build',
            'summary': 'Build failed',
            'message': 'Syntax error on line 42',
            'details': {},
            'log_range': {'start': 10, 'end': 20, 'pinned': true},
          },
        },
      };
      final pushMsg = Message.fromJson(eventPushJson);
      final push = pushMsg.payload as EventPush;
      expect(push.event.eventId, 'id-1');
      expect(push.entry.score, 80);

      final scoreUpdateJson = {
        'type': 'score_update',
        'payload': {'event_id': 'id-1', 'score': 45, 'escalation_level': 1},
      };
      final scoreMsg = Message.fromJson(scoreUpdateJson);
      final scoreUpdate = scoreMsg.payload as ScoreUpdate;
      expect(scoreUpdate.eventId, 'id-1');
      expect(scoreUpdate.score, 45);
      expect(scoreUpdate.escalationLevel, 1);

      final stateUpdateJson = {
        'type': 'state_update',
        'payload': {'event_id': 'id-1', 'state': 'seen', 'superseded': false},
      };
      final stateMsg = Message.fromJson(stateUpdateJson);
      final stateUpdate = stateMsg.payload as StateUpdate;
      expect(stateUpdate.state, EntryState.seen);
      expect(stateUpdate.resolution, isNull);
    });

    test('raw_line and raw_event decodes', () {
      final rawLineJson = {
        'type': 'raw_line',
        'payload': {
          'agent_id': 'a',
          'offset': 7,
          'ts': '2026-09-17T10:32:05.000Z',
          'text': 'Compiling...',
        },
      };
      final rlMsg = Message.fromJson(rawLineJson);
      final rawLine = rlMsg.payload as RawLine;
      expect(rawLine.agentId, 'a');
      expect(rawLine.offset, 7);
      expect(rawLine.text, 'Compiling...');

      final rawEventJson = {
        'type': 'raw_event',
        'payload': {
          'agent_id': 'a',
          'agent_seq': 1,
          'kind': 'step',
          'operation': 'build',
          'message': 'running',
          'details': {},
          'log_lines': ['line 1', 'line 2'],
        },
      };
      final reMsg = Message.fromJson(rawEventJson);
      final re = reMsg.payload as RawAgentEvent;
      expect(re.logLines.length, 2);
    });

    test('event_logs, command_result, metrics, error decodes', () {
      final logsJson = {
        'type': 'event_logs',
        'request_id': 'r-2',
        'payload': {
          'event_id': 'id-1',
          'offset': 5,
          'total': 8,
          'evicted': false,
          'lines': [
            {'offset': 5, 'ts': '2026-09-17T10:32:05.000Z', 'text': 'log 5'},
            {'offset': 6, 'ts': '2026-09-17T10:32:05.000Z', 'text': 'log 6'},
          ],
        },
      };
      final logsMsg = Message.fromJson(logsJson);
      expect(logsMsg.requestId, 'r-2');
      final eventLogs = logsMsg.payload as EventLogs;
      expect(eventLogs.total, 8);
      expect(eventLogs.lines.length, 2);

      final cmdResultJson = {
        'type': 'command_result',
        'request_id': 'r-5',
        'payload': {'ok': false, 'error': 'already_resolved'},
      };
      final cmdMsg = Message.fromJson(cmdResultJson);
      final cmdResult = cmdMsg.payload as CommandResult;
      expect(cmdResult.ok, isFalse);
      expect(cmdResult.error, CommandError.alreadyResolved);

      final metricsJson = {
        'type': 'metrics',
        'request_id': 'r-6',
        'payload': {'raw_events': 42, 'escalations': 2},
      };
      final metricsMsg = Message.fromJson(metricsJson);
      final metrics = metricsMsg.payload as Map<String, int>;
      expect(metrics['raw_events'], 42);
      expect(metrics['escalations'], 2);

      final errJson = {
        'type': 'error',
        'request_id': 'r-9',
        'payload': {'code': 'bad_request', 'message': 'cannot parse frame'},
      };
      final errMsg = Message.fromJson(errJson);
      final err = errMsg.payload as ErrorReply;
      expect(err.code, 'bad_request');
    });

    test('client request payloads serialize correctly', () {
      final hello = Message.push(
        'hello',
        Hello(
          token: 'secret',
          deviceId: 'phone-1',
          clientVersion: '0.1.0',
          schemaVersion: 1,
        ),
      );
      final helloJson = hello.toJson();
      expect(helloJson['type'], 'hello');
      expect(helloJson['request_id'], isNull);
      expect((helloJson['payload'] as Map)['token'], 'secret');

      final reqDetails = Message.request(
        'r-1',
        'get_event_details',
        EventRef(eventId: 'e-1'),
      );
      final reqDetailsJson = reqDetails.toJson();
      expect(reqDetailsJson['request_id'], 'r-1');
      expect((reqDetailsJson['payload'] as Map)['event_id'], 'e-1');

      final reqLogs = Message.request(
        'r-2',
        'get_event_logs',
        GetEventLogs(eventId: 'e-1', offset: -1, limit: 100),
      );
      final reqLogsJson = reqLogs.toJson();
      expect((reqLogsJson['payload'] as Map)['offset'], -1);

      final ack = Message.request('r-3', 'ack', EventRef(eventId: 'e-1'));
      expect(ack.type, 'ack');

      final dismiss = Message.request(
        'r-4',
        'dismiss',
        EventRef(eventId: 'e-1'),
      );
      expect(dismiss.type, 'dismiss');

      final resp = Message.request(
        'r-5',
        'respond_request',
        RespondRequest(eventId: 'e-1', decision: Decision.approve),
      );
      expect((resp.toJson()['payload'] as Map)['decision'], 'approve');

      final structured = Message.request(
        'r-6',
        'respond_request',
        const RespondRequest(
          eventId: 'e-2',
          decision: Decision.approve,
          selectedOptions: ['Rust', 'Go'],
          textInput: 'custom-value',
        ),
      );
      expect((structured.toJson()['payload'] as Map)['selected_options'], [
        'Rust',
        'Go',
      ]);
      expect(
        (structured.toJson()['payload'] as Map)['text_input'],
        'custom-value',
      );
    });
  });
}
