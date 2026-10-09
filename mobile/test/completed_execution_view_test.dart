import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/client_metrics.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:agentdesk/ui/completed_screen.dart';
import 'package:agentdesk/ui/event_screen.dart';

void main() {
  group('Completed Execution Transcript Tests', () {
    testWidgets('Completed execution view renders full chronological transcript without terminal artifacts', (tester) async {
      final state = AgentDeskState();
      final connection = ConnectionService(state: state);
      final metrics = ClientMetrics();

      const eventId = 'ev-completed-1';

      // Full execution transcript produced by the adapter
      const rawSummaryMessage = '''
▸ Thought for 11s, 235 tokens
The objective was to write "hello world" to the designated file.

● Bash
rm -f /home/pirate/test_summary.txt

● Bash
git status

● Bash
git diff --stat

▸ Thought for 9s
All core components now function as expected...

● Edit
~/test_summary.txt

Final result:
Written hello world to test_summary.txt.
''';

      final completedEvent = Event(
        schemaVersion: 1,
        eventId: eventId,
        seq: 10,
        agentSeq: 5,
        agentId: 'antigravity',
        agentName: 'Antigravity',
        project: 'AgentDesk',
        taskId: 'task-101',
        ts: DateTime.utc(2026, 10, 2, 12, 0, 0),
        category: Category.completed,
        severity: Severity.important,
        kind: 'task_completed',
        operation: Operation.other,
        summary: 'Task Completed',
        message: rawSummaryMessage,
        details: {
          'status': 'completed',
          'final_result': 'Written hello world to test_summary.txt.',
          'file': '~/test_summary.txt',
          'command': 'git diff --stat',
          'tool_count': 4,
          'execution_steps': [
            {
              'kind': 'thought',
              'duration': '11s',
              'tokens': '235 tokens',
              'text': 'The objective was to write "hello world" to the designated file.',
            },
            {
              'kind': 'tool_call',
              'tool': 'Bash',
              'command': 'rm -f /home/pirate/test_summary.txt',
            },
            {
              'kind': 'tool_call',
              'tool': 'Bash',
              'command': 'git status',
            },
            {
              'kind': 'tool_call',
              'tool': 'Bash',
              'command': 'git diff --stat',
            },
            {
              'kind': 'thought',
              'duration': '9s',
              'text': 'All core components now function as expected...',
            },
            {
              'kind': 'tool_call',
              'tool': 'Edit',
              'command': '~/test_summary.txt',
            },
            {
              'kind': 'result',
              'text': 'Written hello world to test_summary.txt.',
            },
          ],
        },
        logRange: const LogRange(start: 0, end: 20, pinned: false),
      );

      state.applySnapshot(Snapshot(
        events: [completedEvent],
        entries: [
          const QueueEntry(
            eventId: eventId,
            tier: 2,
            score: 50,
            state: EntryState.new_,
            seq: 10,
          ),
        ],
      ));

      // 1. Verify CompletedScreen list card shows compact summary and metadata
      await tester.pumpWidget(MaterialApp(
        home: CompletedScreen(
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));
      await tester.pumpAndSettle();

      expect(find.text('Task Completed'), findsOneWidget);
      expect(find.text('Antigravity · AgentDesk'), findsOneWidget);
      expect(find.text('Written hello world to test_summary.txt.'), findsOneWidget);
      expect(find.text('4 tool calls · 11s'), findsOneWidget);

      // 2. Open EventScreen (detailed Level 2 view)
      await tester.pumpWidget(MaterialApp(
        home: EventScreen(
          eventId: eventId,
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));
      await tester.pumpAndSettle();

      // Verify Header Card
      expect(find.text('Task Completed'), findsOneWidget);
      expect(find.byKey(const Key('completed_execution_view')), findsOneWidget);

      // Verify Chronological Elements appear:
      // Thought 1 + duration + tokens
      expect(find.text('Thought for 11s'), findsOneWidget);
      expect(find.text('235 tokens'), findsOneWidget);
      expect(
        find.text('The objective was to write "hello world" to the designated file.'),
        findsOneWidget,
      );

      // Bash invocations and commands
      expect(find.text('● Bash'), findsNWidgets(3));
      expect(find.text('rm -f /home/pirate/test_summary.txt'), findsOneWidget);
      expect(find.text('git status'), findsOneWidget);
      expect(find.text('git diff --stat'), findsOneWidget);

      // Thought 2
      expect(find.text('Thought for 9s'), findsOneWidget);
      expect(find.text('All core components now function as expected...'), findsOneWidget);

      // Edit tool call
      expect(find.text('● Edit'), findsOneWidget);
      expect(find.text('~/test_summary.txt'), findsOneWidget);

      // Final result highlighted
      expect(find.text('Final Result'), findsOneWidget);
      expect(find.text('Written hello world to test_summary.txt.'), findsAtLeastNWidgets(1));

      // Verify Terminal / ANSI / Kitty artifacts are completely absent
      expect(find.textContaining('Ga='), findsNothing);
      expect(find.textContaining('AAAAAAA=='), findsNothing);
      expect(find.textContaining('⣾'), findsNothing);
      expect(find.textContaining('\x1b'), findsNothing);
    });

    testWidgets('CompletedScreen parses fallback message when tool_count is absent', (tester) async {
      final state = AgentDeskState();
      const eventId = 'completed-event-fallback-1';
      final fallbackEvent = Event(
        schemaVersion: 1,
        eventId: eventId,
        seq: 1,
        agentSeq: 1,
        agentId: 'antigravity',
        agentName: 'Antigravity',
        project: 'AgentDesk',
        taskId: 'task-fallback-1',
        ts: DateTime.utc(2026, 10, 3, 8, 0, 0),
        category: Category.completed,
        severity: Severity.important,
        kind: 'task_completed',
        operation: Operation.other,
        summary: 'Task Completed',
        message: 'Thought for 5s, 500 tokens\n● Bash(ls)\n● Edit(/foo)\nDone creating files.',
        details: const {},
        logRange: const LogRange(start: 0, end: 10, pinned: false),
      );

      state.applySnapshot(Snapshot(
        events: [fallbackEvent],
        entries: [
          const QueueEntry(
            eventId: eventId,
            tier: 2,
            score: 50,
            state: EntryState.new_,
            seq: 1,
          ),
        ],
      ));

      await tester.pumpWidget(MaterialApp(
        home: CompletedScreen(
          state: state,
        ),
      ));
      await tester.pumpAndSettle();

      expect(find.text('Task Completed'), findsOneWidget);
      expect(find.text('2 tool calls · 5s'), findsOneWidget);
      expect(find.text('Done creating files.'), findsOneWidget);
    });

    testWidgets('EventScreen renders multi-paragraph markdown final result prominently at the top', (tester) async {
      final state = AgentDeskState();
      final connection = ConnectionService(state: state);
      final metrics = ClientMetrics();

      const eventId = 'ev-completed-grep';
      const multiParagraphResult = '''The directory and Rust grep program have been created:

• Directory: test_folder
• Executable binary: grep
• Source code: grep.rs (also located at main.rs)
• Cargo manifest: Cargo.toml

### Features Supported

• Regex matching: Powered by the Rust regex engine.
• Flags: -i, -n, -v, -c, -r
• Exit codes: 0 on match, 1 on no match, and 2 on syntax or file errors.

### Example Usage

  # Pattern search in a file with line numbers
  /tmp/test_folder/grep -n "edition" /tmp/test_folder/Cargo.toml''';

      final completedEvent = Event(
        schemaVersion: 1,
        eventId: eventId,
        seq: 20,
        agentSeq: 10,
        agentId: 'antigravity',
        agentName: 'Antigravity',
        project: 'AgentDesk',
        taskId: 'task-grep-1',
        ts: DateTime.utc(2026, 10, 3, 9, 0, 0),
        category: Category.completed,
        severity: Severity.important,
        kind: 'task_completed',
        operation: Operation.other,
        summary: 'Task Completed',
        message: 'Thought for 4s, 521 tokens\n● Bash(cargo build --release)\n$multiParagraphResult',
        details: {
          'status': 'completed',
          'final_result': multiParagraphResult,
          'tool_count': 1,
          'execution_steps': [
            {
              'kind': 'thought',
              'duration': '4s',
              'tokens': '521 tokens',
              'text': 'The user has approved the Rust implementation.',
            },
            {
              'kind': 'tool_call',
              'tool': 'Bash',
              'command': 'cargo build --release',
            },
            {
              'kind': 'result',
              'text': multiParagraphResult,
            },
          ],
        },
        logRange: const LogRange(start: 0, end: 20, pinned: false),
      );

      state.applySnapshot(Snapshot(
        events: [completedEvent],
        entries: [
          const QueueEntry(
            eventId: eventId,
            tier: 2,
            score: 50,
            state: EntryState.new_,
            seq: 20,
          ),
        ],
      ));

      // 1. Verify CompletedScreen list tile shows the headline
      await tester.pumpWidget(MaterialApp(
        home: CompletedScreen(
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));
      await tester.pumpAndSettle();

      expect(find.text('Task Completed'), findsOneWidget);
      expect(find.textContaining('The directory and Rust grep program have been created:'), findsOneWidget);

      // 2. Verify EventScreen renders the full multi-paragraph result in the Final Result box
      await tester.pumpWidget(MaterialApp(
        home: EventScreen(
          eventId: eventId,
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ));
      await tester.pumpAndSettle();

      expect(find.text('Final Result'), findsOneWidget);
      expect(find.textContaining('• Directory: test_folder'), findsOneWidget);
      expect(find.textContaining('### Features Supported'), findsOneWidget);
      expect(find.textContaining('### Example Usage'), findsOneWidget);
      expect(find.textContaining('/tmp/test_folder/grep -n "edition" /tmp/test_folder/Cargo.toml'), findsOneWidget);
    });
  });
}
