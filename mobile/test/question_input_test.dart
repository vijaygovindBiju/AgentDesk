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
    state.applyStateUpdate(
      StateUpdate(
        eventId: eventId,
        state: EntryState.seen,
        resolution: Resolution.approved,
        superseded: false,
      ),
    );
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
    expect(
      connection.state.entryFor('single')?.resolution,
      Resolution.approved,
    );
  });

  testWidgets('renders explicit yes/no options as structured choices', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'yes-no',
      type: QuestionType.yesNo,
      options: ['Continue', 'Stop'],
    );
    await showRequest(tester, connection, event);

    expect(find.text('Continue'), findsOneWidget);
    expect(find.text('Stop'), findsOneWidget);
    expect(find.byKey(const Key('approve_button')), findsNothing);
    await tester.tap(find.byKey(const Key('question_option_Stop')));
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.selectedOptions, ['Stop']);
    expect(connection.decision, Decision.approve);
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

  testWidgets('choice plus supplied Other preserves option and write-in text', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'other-option',
      type: QuestionType.singleChoice,
      options: ['Local', 'Other'],
      details: const {'allows_write_in': true},
    );
    await showRequest(tester, connection, event);

    expect(find.text('Other / Write your own'), findsNothing);
    await tester.tap(find.byKey(const Key('question_option_Other')));
    await tester.pump();
    await tester.enterText(
      find.byKey(const Key('question_text_input')),
      'WASM',
    );
    tester.testTextInput.hide();
    await tester.pump();
    await tester.drag(find.byType(ListView), const Offset(0, -300));
    await tester.pump();
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, ['Other']);
    expect(connection.textInput, 'WASM');
  });

  const opt1 = 'Yes, run command';
  const opt2 =
      "Yes, and always allow in this conversation for commands that start with 'ls'";
  const opt3 =
      "Yes, and always allow for commands that start with 'ls' (Persist to settings.json)";
  const opt4 = 'No, cancel';
  const cmd = 'ls -la && which cargo rustc && cargo --version';

  test('four-option command approval request decodes correctly and preserves all exact labels', () {
    final wireJson = {
      'schema_version': 1,
      'event_id': 'real-command-approval-1',
      'seq': 1,
      'agent_seq': 1,
      'agent_id': 'antigravity',
      'agent_name': 'Antigravity',
      'project': 'AgentDesk',
      'ts': '2026-09-28T18:00:00.000Z',
      'category': 'request',
      'severity': 3,
      'kind': 'approval_required',
      'operation': 'test',
      'summary': 'Command requires approval: $cmd',
      'message': 'Command requires approval: $cmd',
      'details': {
        'command': cmd,
        'interaction': 'command_approval',
        'selected_option': 0,
        'selected_options': [opt1],
        'allows_write_in': false,
        'options': [opt1, opt2, opt3, opt4],
      },
      'log_range': {'start': 0, 'end': 0, 'pinned': true},
      'request': {
        'prompt': 'Run this command?\n$cmd',
        'options': [opt1, opt2, opt3, opt4],
        'question_type': 'single_choice',
      },
    };

    final event = Event.fromJson(wireJson);
    expect(event.eventId, 'real-command-approval-1');
    expect(event.category, Category.request);
    expect(event.kind, 'approval_required');
    expect(event.request, isNotNull);
    expect(event.request!.questionType, QuestionType.singleChoice);
    expect(event.request!.prompt, 'Run this command?\n$cmd');
    expect(event.request!.options, [opt1, opt2, opt3, opt4]);
    expect(event.request!.options.length, 4);
    expect(event.request!.options.contains('approve'), isFalse);
    expect(event.request!.options.contains('deny'), isFalse);
    expect(event.details['allows_write_in'], isFalse);
    expect(event.details['command'], cmd);
  });

  testWidgets('four-option command approval UI exposes all four options', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'four-options-ui',
      type: QuestionType.singleChoice,
      options: [opt1, opt2, opt3, opt4],
      details: const {
        'command': cmd,
        'interaction': 'command_approval',
        'selected_option': 0,
        'allows_write_in': false,
      },
    );
    await showRequest(tester, connection, event);

    expect(find.text(opt1), findsOneWidget);
    expect(find.text(opt2), findsOneWidget);
    expect(find.text(opt3), findsOneWidget);
    expect(find.text(opt4), findsOneWidget);
    expect(find.text('Other / Write your own'), findsNothing);
    expect(find.byKey(const Key('approve_button')), findsNothing);
    expect(find.byKey(const Key('deny_button')), findsNothing);
  });

  testWidgets('selecting option 1 produces the correct structured response', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'four-options-1',
      type: QuestionType.singleChoice,
      options: [opt1, opt2, opt3, opt4],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.tap(find.byKey(const Key('question_option_Yes, run command')));
    await tester.drag(find.byType(ListView), const Offset(0, -300));
    await tester.pump();
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, [opt1]);
    expect(connection.textInput, isNull);

    final req = RespondRequest(
      eventId: event.eventId,
      decision: connection.decision!,
      selectedOptions: connection.selectedOptions,
    );
    final json = req.toJson();
    expect(json['decision'], 'approve');
    expect(json['selected_options'], [opt1]);
  });

  testWidgets('selecting option 2 produces the correct structured response', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'four-options-2',
      type: QuestionType.singleChoice,
      options: [opt1, opt2, opt3, opt4],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.tap(find.byKey(const Key('question_option_$opt2')));
    await tester.drag(find.byType(ListView), const Offset(0, -300));
    await tester.pump();
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, [opt2]);
    expect(connection.textInput, isNull);
  });

  testWidgets('selecting option 3 produces the correct structured response', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'four-options-3',
      type: QuestionType.singleChoice,
      options: [opt1, opt2, opt3, opt4],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.drag(find.byType(ListView), const Offset(0, -100));
    await tester.pump();
    await tester.tap(find.byKey(const Key('question_option_$opt3')));
    await tester.drag(find.byType(ListView), const Offset(0, -200));
    await tester.pump();
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, [opt3]);
    expect(connection.textInput, isNull);
  });

  testWidgets('selecting option 4 produces the correct structured response', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'four-options-4',
      type: QuestionType.singleChoice,
      options: [opt1, opt2, opt3, opt4],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.drag(find.byType(ListView), const Offset(0, -150));
    await tester.pump();
    await tester.tap(find.byKey(const Key('question_option_$opt4')));
    await tester.drag(find.byType(ListView), const Offset(0, -200));
    await tester.pump();
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, [opt4]);
    expect(connection.textInput, isNull);
  });

  testWidgets('two-option approval with real labels keeps exact choices', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'two-options-real',
      type: QuestionType.singleChoice,
      options: ['Yes, run command', 'No, cancel'],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    expect(find.text('Yes, run command'), findsOneWidget);
    expect(find.text('No, cancel'), findsOneWidget);
    expect(find.byKey(const Key('approve_button')), findsNothing);
    expect(find.byKey(const Key('deny_button')), findsNothing);

    await tester.tap(find.byKey(const Key('question_option_Yes, run command')));
    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, ['Yes, run command']);
  });

  testWidgets('legacy two-option approval renders standard Approve and Deny buttons', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = Event(
      schemaVersion: 1,
      eventId: 'legacy-approval',
      seq: 1,
      agentSeq: 1,
      agentId: 'antigravity',
      agentName: 'Antigravity',
      project: 'legacy-project',
      ts: DateTime.utc(2026, 9, 20),
      category: Category.request,
      severity: Severity.critical,
      kind: 'approval_required',
      operation: Operation.other,
      summary: 'Legacy approval',
      message: 'Run migration?',
      details: const {},
      logRange: const LogRange(start: 0, end: 0, pinned: false),
      request: const RequestInfo(
        prompt: 'Run migration?',
        options: ['approve', 'deny'],
        questionType: null,
      ),
    );
    await showRequest(tester, connection, event);

    expect(find.byKey(const Key('approve_button')), findsOneWidget);
    expect(find.byKey(const Key('deny_button')), findsOneWidget);

    await tester.tap(find.byKey(const Key('approve_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.approve);
    expect(connection.selectedOptions, isNull);
  });

  testWidgets('submitting choice question without selection shows inline error', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'no-selection',
      type: QuestionType.singleChoice,
      options: ['Option A', 'Option B'],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(find.text('Please select an option.'), findsOneWidget);
    expect(connection.decision, isNull);
  });

  testWidgets('cancel button on question sends deny decision', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'cancel-question',
      type: QuestionType.singleChoice,
      options: [opt1, opt2, opt3, opt4],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.drag(find.byType(ListView), const Offset(0, -300));
    await tester.pump();
    await tester.tap(find.byKey(const Key('deny_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, Decision.deny);
  });

  testWidgets('submitting free text question without input shows inline error', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'empty-free-text',
      type: QuestionType.freeText,
      options: [],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.tap(find.byKey(const Key('send_question_button')));
    await tester.pumpAndSettle();

    expect(find.text('Please enter a response.'), findsOneWidget);
    expect(connection.decision, isNull);
  });

  testWidgets('cancel button on free text question does not send deny decision', (
    tester,
  ) async {
    final connection = CapturingConnection(state: AgentDeskState());
    final event = requestEvent(
      id: 'cancel-free-text',
      type: QuestionType.freeText,
      options: [],
      details: const {'allows_write_in': false},
    );
    await showRequest(tester, connection, event);

    await tester.tap(find.byKey(const Key('deny_question_button')));
    await tester.pumpAndSettle();

    expect(connection.decision, isNull);
  });
}
