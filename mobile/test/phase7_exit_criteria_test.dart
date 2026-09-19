import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/client_metrics.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:agentdesk/ui/event_screen.dart';
import 'package:agentdesk/ui/home_screen.dart';
import 'package:agentdesk/ui/log_viewer_screen.dart';

class MockConnectionService extends ConnectionService {
  MockConnectionService({required super.state});

  final List<String> approvedEvents = [];
  final List<String> dismissedEvents = [];

  @override
  ConnectionStatus get status => ConnectionStatus.connected;

  @override
  Future<EventPush> getEventDetails(String eventId) async {
    final event = state.eventFor(eventId)!;
    final entry = state.entryFor(eventId)!;
    return EventPush(event: event, entry: entry.copyWith(state: EntryState.seen));
  }

  @override
  Future<EventLogs> getEventLogs(String eventId, int offset, int limit) async {
    return EventLogs(
      eventId: eventId,
      offset: 100,
      total: 3,
      evicted: false,
      lines: [
        LogLine(offset: 100, ts: DateTime.utc(2026, 9, 19, 12, 0, 0), text: 'Connecting to DB'),
        LogLine(offset: 101, ts: DateTime.utc(2026, 9, 19, 12, 0, 1), text: 'Prepared statement built'),
        LogLine(offset: 102, ts: DateTime.utc(2026, 9, 19, 12, 0, 2), text: 'Waiting for operator approval...'),
      ],
    );
  }

  @override
  Future<CommandResult> respondRequest(String eventId, Decision decision) async {
    if (decision == Decision.approve) {
      approvedEvents.add(eventId);
      state.applyStateUpdate(StateUpdate(
        eventId: eventId,
        state: EntryState.seen,
        resolution: Resolution.approved,
        superseded: false,
      ));
      return const CommandResult(ok: true);
    }
    return const CommandResult(ok: true);
  }

  @override
  Future<CommandResult> dismiss(String eventId) async {
    dismissedEvents.add(eventId);
    state.applyStateUpdate(StateUpdate(
      eventId: eventId,
      state: EntryState.dismissed,
      superseded: false,
    ));
    return const CommandResult(ok: true);
  }
}

void main() {
  testWidgets('Phase 7 exit criteria: summary -> details -> logs -> approve end-to-end',
      (tester) async {
    final state = AgentDeskState();
    final connection = MockConnectionService(state: state);
    final metrics = ClientMetrics();

    final requestEventId = 'req-e2e-1';

    // Populate initial state via snapshot
    state.applySnapshot(Snapshot(
      entries: [
        const QueueEntry(
          eventId: 'req-e2e-1',
          tier: 0,
          score: 90,
          state: EntryState.new_,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ],
      events: [
        Event(
          schemaVersion: 1,
          eventId: requestEventId,
          seq: 1,
          agentSeq: 1,
          agentId: 'backend-agent',
          agentName: 'Backend Agent',
          project: 'AgentDesk',
          taskId: 'task-db-1',
          ts: DateTime.utc(2026, 9, 19, 12, 0, 0),
          category: Category.request,
          severity: Severity.critical,
          kind: 'approval_required',
          operation: Operation.edit,
          summary: 'DB migration approval needed',
          message: 'Confirm migration on staging',
          details: const {'table': 'auth_tokens', 'rows': 1500},
          logRange: const LogRange(start: 100, end: 105, pinned: true),
          request: const RequestInfo(
            prompt: 'Execute migration on staging?',
            options: ['approve', 'deny'],
          ),
        ),
      ],
    ));

    await tester.pumpWidget(MaterialApp(
      home: HomeScreen(
        state: state,
        connection: connection,
        metrics: metrics,
      ),
    ));
    await tester.pumpAndSettle();

    // 1. SUMMARY: Verify entry on HomeScreen
    expect(find.text('DB migration approval needed'), findsOneWidget);
    expect(find.text('Confirm migration on staging'), findsOneWidget);
    expect(find.byKey(Key('entry_card_$requestEventId')), findsOneWidget);

    // 2. DETAILS: Tap card to open EventScreen
    await tester.tap(find.byKey(Key('entry_card_$requestEventId')));
    await tester.pumpAndSettle();

    expect(find.byType(EventScreen), findsOneWidget);
    expect(find.text('Details (Level 2)'), findsOneWidget);
    expect(find.text('table'), findsOneWidget);
    expect(find.text('auth_tokens'), findsOneWidget);
    expect(find.text('Execute migration on staging?'), findsOneWidget);

    // 3. LOGS: Tap View Logs button in app bar or list
    expect(find.byKey(const Key('view_logs_appbar_button')), findsOneWidget);
    await tester.tap(find.byKey(const Key('view_logs_appbar_button')));
    await tester.pumpAndSettle();

    expect(find.byType(LogViewerScreen), findsOneWidget);
    expect(find.text('Connecting to DB'), findsOneWidget);
    expect(find.text('Prepared statement built'), findsOneWidget);
    expect(find.text('Waiting for operator approval...'), findsOneWidget);

    // Pop back to EventScreen
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.byType(EventScreen), findsOneWidget);

    // 4. APPROVE: Tap Approve
    expect(find.byKey(const Key('approve_button')), findsOneWidget);
    await tester.ensureVisible(find.byKey(const Key('approve_button')));
    await tester.tap(find.byKey(const Key('approve_button')));
    await tester.pumpAndSettle();

    // Verify approve succeeded and popped back or showed snackbar
    expect(connection.approvedEvents, contains(requestEventId));
    expect(state.entryFor(requestEventId)?.resolution, Resolution.approved);
  });
}
