import 'package:flutter/foundation.dart' hide Category;
import 'package:flutter/material.dart';

import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'event_screen.dart';

class StructuredRequestFixtureScreen extends StatefulWidget {
  final AgentDeskState state;

  const StructuredRequestFixtureScreen({super.key, required this.state});

  @override
  State<StructuredRequestFixtureScreen> createState() =>
      _StructuredRequestFixtureScreenState();
}

class _StructuredRequestFixtureScreenState
    extends State<StructuredRequestFixtureScreen> {
  late final FixtureConnection _connection;
  final ClientMetrics _metrics = ClientMetrics();
  late final List<_Fixture> _fixtures;

  @override
  void initState() {
    super.initState();
    _connection = FixtureConnection(state: widget.state);
    _fixtures = _buildFixtures();
    widget.state.applySnapshot(
      Snapshot(
        entries: [
          for (var i = 0; i < _fixtures.length; i++)
            QueueEntry(
              eventId: _fixtures[i].event.eventId,
              tier: 0,
              score: 100 - i,
              state: EntryState.new_,
              resolution: Resolution.unresolved,
              seq: i + 1,
            ),
        ],
        events: _fixtures.map((fixture) => fixture.event).toList(),
      ),
    );
  }

  List<_Fixture> _buildFixtures() {
    var sequence = 0;
    Event event({
      required String id,
      required String title,
      required String prompt,
      required QuestionType? questionType,
      required List<String> options,
      Map<String, dynamic> details = const {},
    }) {
      return Event(
        schemaVersion: 1,
        eventId: id,
        seq: ++sequence,
        agentSeq: sequence,
        agentId: 'structured-fixture',
        agentName: 'Structured Fixture',
        project: 'AgentDesk UI',
        ts: DateTime.utc(2026, 9, 21),
        category: Category.request,
        severity: Severity.critical,
        kind: 'input_required',
        operation: Operation.other,
        summary: title,
        message: 'Deterministic structured UI fixture',
        details: details,
        logRange: const LogRange(start: 0, end: 0, pinned: false),
        request: RequestInfo(
          prompt: prompt,
          options: options,
          questionType: questionType,
        ),
      );
    }

    return [
      _Fixture(
        label: 'Approval',
        event: event(
          id: 'fixture-approval',
          title: 'Approval',
          prompt: 'Run the database migration?',
          questionType: null,
          options: const [],
        ),
      ),
      _Fixture(
        label: 'Single choice',
        event: event(
          id: 'fixture-single',
          title: 'Single choice',
          prompt: 'Which programming language should be used?',
          questionType: QuestionType.singleChoice,
          options: const ['Rust', 'Python', 'Go', 'C++', 'JavaScript'],
        ),
      ),
      _Fixture(
        label: 'Multiple choice',
        event: event(
          id: 'fixture-multiple',
          title: 'Multiple choice',
          prompt: 'Which features should be enabled?',
          questionType: QuestionType.multipleChoice,
          options: const [
            'Logging',
            'Metrics',
            'TLS',
            'Caching',
            'Notifications',
          ],
        ),
      ),
      _Fixture(
        label: 'Free-form text',
        event: event(
          id: 'fixture-free-text',
          title: 'Free-form text',
          prompt: 'Describe the behavior you want.',
          questionType: QuestionType.freeText,
          options: const [],
        ),
      ),
      _Fixture(
        label: 'Choice + write-in',
        event: event(
          id: 'fixture-write-in',
          title: 'Choice + write-in',
          prompt: 'Which deployment target should be used?',
          questionType: QuestionType.singleChoice,
          options: const ['Local', 'Docker', 'Kubernetes', 'Other'],
          details: const {'allows_write_in': true},
        ),
      ),
      _Fixture(
        label: 'Dynamic A',
        event: event(
          id: 'fixture-dynamic-a',
          title: 'Dynamic option set A',
          prompt: 'Choose a runtime.',
          questionType: QuestionType.singleChoice,
          options: const ['Rust', 'Python', 'Go'],
        ),
      ),
      _Fixture(
        label: 'Dynamic B',
        event: event(
          id: 'fixture-dynamic-b',
          title: 'Dynamic option set B',
          prompt: 'Choose a team.',
          questionType: QuestionType.singleChoice,
          options: const ['Frontend', 'Backend', 'DevOps', 'Mobile'],
        ),
      ),
      _Fixture(
        label: 'Long content',
        event: event(
          id: 'fixture-long',
          title: 'Long content and scrolling',
          prompt:
              'This is a deliberately long, multiline question.\n\n'
              'Explain the behavior, constraints, fallback strategy, and '
              'observability requirements for this operation in detail.',
          questionType: QuestionType.multipleChoice,
          options: [
            'A very long option name describing a resilient implementation '
                'with retries and clear diagnostics',
            'Unicode: 日本語 / हिन्दी / العربية / 🚀',
            'Another option with multiple words and a second line of context',
            ...List.generate(20, (index) => 'Option ${index + 1}'),
          ],
        ),
      ),
    ];
  }

  Future<void> _openFixture(_Fixture fixture) async {
    await Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => EventScreen(
          eventId: fixture.event.eventId,
          state: widget.state,
          connection: _connection,
          metrics: _metrics,
        ),
      ),
    );
    if (mounted) setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    if (!kDebugMode) {
      return const Scaffold(
        body: Center(child: Text('Structured fixtures are debug-only.')),
      );
    }

    return Scaffold(
      appBar: AppBar(title: const Text('Structured UI Fixtures')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text(
            'These deterministic requests use the same EventScreen and '
            'structured RequestInfo model as live AgentDesk traffic.',
          ),
          const SizedBox(height: 16),
          for (final fixture in _fixtures)
            Card(
              child: ListTile(
                key: Key('fixture_${fixture.event.eventId}'),
                title: Text(fixture.label),
                subtitle: Text(fixture.event.request!.prompt),
                trailing: const Icon(Icons.chevron_right),
                onTap: () => _openFixture(fixture),
              ),
            ),
          if (_connection.responses.isNotEmpty) ...[
            const SizedBox(height: 16),
            Text(
              'Captured responses',
              style: Theme.of(context).textTheme.titleMedium,
            ),
            for (final response in _connection.responses)
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Text(response),
                ),
              ),
          ],
        ],
      ),
    );
  }
}

class _Fixture {
  final String label;
  final Event event;

  const _Fixture({required this.label, required this.event});
}

class FixtureConnection extends ConnectionService {
  FixtureConnection({required super.state});

  final List<String> responses = [];

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
    responses.add(
      '$eventId: decision=${decision.wireName}, '
      'selected=${selectedOptions ?? ''}, text=${textInput ?? ''}',
    );
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

  @override
  Future<CommandResult> dismiss(String eventId) async {
    state.applyStateUpdate(
      StateUpdate(
        eventId: eventId,
        state: EntryState.dismissed,
        resolution: Resolution.denied,
        superseded: false,
      ),
    );
    return const CommandResult(ok: true);
  }
}
