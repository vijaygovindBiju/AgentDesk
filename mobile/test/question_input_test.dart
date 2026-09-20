import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/client_metrics.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:agentdesk/ui/event_screen.dart';

class CapturingConnection extends ConnectionService {
  CapturingConnection({required super.state});

  List<String>? selectedOptions;
  String? textInput;
  Decision? decision;

  @override
  ConnectionStatus get status => ConnectionStatus.connected;

  @override
  Future<EventPush> getEventDetails(String eventId) async {
    return EventPush(
      event: state.eventFor(eventId)!,
      entry: state.entryFor(eventId)!,
    );
  }

  @override
  Future<CommandResult> respondRequest(
    String eventId,
    Decision decision, {
    List<String>? selectedOptions,
    String? textInput,
  }) async {
    this.decision = decision;
    this.selectedOptions = selectedOptions;
    this.textInput = textInput;
    return const CommandResult(ok: true);
  }
}

Event requestEvent({
  required String id,
  required QuestionType type,
  required List<String> options,
  Map<String, dynamic> details = const {},
}) {
  return Event(
    schemaVersion: 1,
    eventId: id,
    seq: 1,
    agentSeq: 1,
    agentId: 'antigravity',
    agentName: 'Antigravity',
    project: 'generic-project',
    ts: DateTime.utc(2026, 9, 20),
    category: Category.request,
    severity: Severity.critical,
    kind: 'input_required',
    operation: Operation.other,
    summary: 'Input required',
    message: 'The agent needs a decision.',
    details: details,
    logRange: const LogRange(start: 0, end: 0, pinned: false),
    request: RequestInfo(
      prompt: 'Use this exact question: ${type.wireName}',
      options: options,
      questionType: type,
    ),
  );
}

Future<void> showRequest(
  WidgetTester tester,
  CapturingConnection connection,
  Event event,
) async {
  final state = connection.state;
  state.applySnapshot(
    Snapshot(
      entries: [
        QueueEntry(
          eventId: event.eventId,
          tier: 0,
          score: 90,
          state: EntryState.new_,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ],
      events: [event],
    ),
  );

  await tester.pumpWidget(
    MaterialApp(
      home: EventScreen(
        eventId: event.eventId,
        state: state,
        connection: connection,
        metrics: ClientMetrics(),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('renders exact single-choice options and sends selection', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'single',
      type: QuestionType.singleChoice,
      options: ['Python', 'Rust'],
    );
    await showRequest(tester, connection, event);

    expect(find.text('Use this exact question: single_choice'), findsOneWidget);
    expect(find.text('Python'), findsOneWidget);
    expect(find.text('Rust'), findsOneWidget);
    await tester.tap(find.byKey(const Key('question_option_Rust')));
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, ['Rust']);
    expect(connection.textInput, isNull);
  });

  testWidgets('renders multi-select options and sends all selections', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'multi',
      type: QuestionType.multipleChoice,
      options: ['size', 'extensions', 'depth'],
    );
    await showRequest(tester, connection, event);

    await tester.tap(find.byKey(const Key('question_option_size')));
    await tester.tap(find.byKey(const Key('question_option_depth')));
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.selectedOptions, containsAll(<String>['size', 'depth']));
    expect(connection.selectedOptions, hasLength(2));
  });

  testWidgets('renders free text input and sends arbitrary text', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'text',
      type: QuestionType.freeText,
      options: const [],
    );
    await showRequest(tester, connection, event);

    const answer = 'name=dirscope; --depth=2';
    await tester.enterText(
      find.byKey(const Key('question_text_input')),
      answer,
    );
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.textInput, answer);
    expect(connection.selectedOptions, isNull);
  });

  testWidgets('choice plus write-in exposes both supplied choices and text', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'other',
      type: QuestionType.singleChoice,
      options: ['Existing'],
      details: const {'allows_write_in': true},
    );
    await showRequest(tester, connection, event);

    expect(find.text('Existing'), findsOneWidget);
    await tester.tap(find.text('Other / Write your own'));
    await tester.pump();
    expect(find.byKey(const Key('question_text_input')), findsOneWidget);
  });
}
