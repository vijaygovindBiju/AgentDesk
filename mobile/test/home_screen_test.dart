import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/client_metrics.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:agentdesk/ui/debug_screen.dart';
import 'package:agentdesk/ui/event_screen.dart';
import 'package:agentdesk/ui/home_screen.dart';

Event testEvent({
  required String id,
  required int seq,
  required Category category,
  Severity severity = Severity.routine,
  String summary = 'Summary',
  String message = 'Message',
  String? taskId,
  RequestInfo? request,
}) {
  return Event(
    schemaVersion: 1,
    eventId: id,
    seq: seq,
    agentSeq: seq,
    agentId: 'agent-1',
    agentName: 'Agent 1',
    project: 'Proj',
    taskId: taskId,
    ts: DateTime.utc(2026, 9, 19, 12, 0, 0),
    category: category,
    severity: severity,
    kind: 'test_kind',
    operation: Operation.build,
    summary: summary,
    message: message,
    details: {'extra_info': 'value_42'},
    logRange: const LogRange(start: 0, end: 10, pinned: false),
    request: request,
  );
}

void main() {
  group('P7.T4 - Widget tests', () {
    testWidgets('home shows the four tiers in fixed order', (tester) async {
      final state = AgentDeskState();
      final connection = ConnectionService(state: state);
      final metrics = ClientMetrics();

      await tester.pumpWidget(MaterialApp(
        home: HomeScreen(
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));

      expect(find.text('Requests'), findsOneWidget);
      expect(find.text('Errors'), findsOneWidget);
      expect(find.text('Completed'), findsOneWidget);
      expect(find.text('Working'), findsOneWidget);

      // Verify empty-state copy
      expect(find.text('No pending approval requests'), findsOneWidget);
      expect(find.text('No active errors'), findsOneWidget);
      expect(find.text('No completed tasks yet'), findsOneWidget);
      expect(find.text('No active background tasks'), findsOneWidget);
    });

    testWidgets('shows escalation badge for escalated working entry',
        (tester) async {
      final state = AgentDeskState();
      final connection = ConnectionService(state: state);
      final metrics = ClientMetrics();

      final ev = testEvent(
        id: 'work-1',
        seq: 1,
        category: Category.working,
        summary: 'Compiling large crate',
      );
      state.applyEvent(EventPush(
        event: ev,
        entry: const QueueEntry(
          eventId: 'work-1',
          tier: 3,
          score: 10,
          state: EntryState.new_,
          escalationLevel: 1,
          seq: 1,
        ),
      ));

      await tester.pumpWidget(MaterialApp(
        home: HomeScreen(
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));

      expect(find.text('Compiling large crate'), findsOneWidget);
      expect(find.byKey(const Key('escalation_badge_work-1')), findsOneWidget);
      expect(find.text('Unusually long (L1)'), findsOneWidget);
    });

    testWidgets('shows still blocking indicator for dismissed-but-unresolved request',
        (tester) async {
      final state = AgentDeskState();
      final connection = ConnectionService(state: state);
      final metrics = ClientMetrics();

      final ev = testEvent(
        id: 'req-1',
        seq: 1,
        category: Category.request,
        summary: 'Confirm production deploy',
        request: const RequestInfo(
          prompt: 'Deploy to production?',
          options: ['approve', 'deny'],
        ),
      );
      state.applyEvent(EventPush(
        event: ev,
        entry: const QueueEntry(
          eventId: 'req-1',
          tier: 0,
          score: 80,
          state: EntryState.dismissed,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ));

      await tester.pumpWidget(MaterialApp(
        home: HomeScreen(
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));

      expect(find.text('Confirm production deploy'), findsOneWidget);
      expect(find.byKey(const Key('still_blocking_badge_req-1')), findsOneWidget);
      expect(find.text('Still blocking agent'), findsOneWidget);
    });

    testWidgets('shows insecure-dev banner on debug screen when transport is insecure_dev',
        (tester) async {
      final state = AgentDeskState();
      final connection = ConnectionService(state: state);
      final metrics = ClientMetrics();

      state.setWelcome(Welcome(
        daemonVersion: '0.1.0',
        schemaVersion: 1,
        pipelineMode: PipelineMode.agentdesk,
        transport: TransportMode.insecureDev,
        serverTime: DateTime.utc(2026, 9, 19, 12, 0, 0),
      ));

      await tester.pumpWidget(MaterialApp(
        home: DebugScreen(
          connection: connection,
          state: state,
          metrics: metrics,
        ),
      ));

      expect(find.byKey(const Key('insecure_dev_banner')), findsOneWidget);
      expect(find.textContaining('INSECURE DEVELOPMENT MODE'), findsOneWidget);
    });

    testWidgets('event screen renders Level 2 details and request approval buttons',
        (tester) async {
      final state = AgentDeskState();
      final connection = ConnectionService(state: state);
      final metrics = ClientMetrics();

      final ev = testEvent(
        id: 'req-2',
        seq: 1,
        category: Category.request,
        severity: Severity.critical,
        summary: 'Database Migration Needed',
        message: 'Schema v2 requires dropping table legacy_users',
        taskId: 'task-db-mig',
        request: const RequestInfo(
          prompt: 'Allow migration?',
          options: ['approve', 'deny'],
        ),
      );
      state.applyEvent(EventPush(
        event: ev,
        entry: const QueueEntry(
          eventId: 'req-2',
          tier: 0,
          score: 95,
          state: EntryState.new_,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ));

      await tester.pumpWidget(MaterialApp(
        home: EventScreen(
          eventId: 'req-2',
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));

      expect(find.text('Database Migration Needed'), findsOneWidget);
      expect(find.text('Schema v2 requires dropping table legacy_users'), findsOneWidget);
      expect(find.text('Critical (3)'), findsOneWidget);
      expect(find.text('Details (Level 2)'), findsOneWidget);
      expect(find.text('extra_info'), findsOneWidget);
      expect(find.text('value_42'), findsOneWidget);
      expect(find.byKey(const Key('approve_button')), findsOneWidget);
      expect(find.byKey(const Key('deny_button')), findsOneWidget);
      expect(find.byKey(const Key('view_logs_button')), findsOneWidget);
      expect(find.byKey(const Key('dismiss_button')), findsOneWidget);
    });
  });
}
