import 'dart:convert';
import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/state/agentdesk_state.dart';

Event createTestEvent({
  required String id,
  required int seq,
  required Category category,
  Severity severity = Severity.routine,
  String summary = 'Summary',
  String message = 'Message',
}) {
  return Event(
    schemaVersion: 1,
    eventId: id,
    seq: seq,
    agentSeq: seq,
    agentId: 'agent-1',
    agentName: 'Agent 1',
    project: 'Proj',
    ts: DateTime.utc(2026, 9, 19, 12, 0, 0),
    category: category,
    severity: severity,
    kind: 'test',
    operation: Operation.build,
    summary: summary,
    message: message,
    logRange: const LogRange(start: 0, end: 10, pinned: false),
  );
}

void main() {
  group('P7.T2 - Reducer tests', () {
    test('snapshot replaces stale entries and merges events', () {
      final state = AgentDeskState();
      // Insert an old entry that should be cleared
      final oldEvent = createTestEvent(id: 'old-id', seq: 1, category: Category.working);
      state.applyEvent(EventPush(
        event: oldEvent,
        entry: QueueEntry(
          eventId: 'old-id',
          tier: Category.working.tier,
          score: 10,
          state: EntryState.new_,
          seq: 1,
        ),
      ));
      expect(state.queue.containsKey('old-id'), isTrue);

      final newEvent = createTestEvent(id: 'new-id', seq: 2, category: Category.error);
      final snapshot = Snapshot(
        entries: [
          QueueEntry(
            eventId: 'new-id',
            tier: Category.error.tier,
            score: 75,
            state: EntryState.new_,
            seq: 2,
          ),
        ],
        events: [newEvent],
      );

      state.applySnapshot(snapshot);
      expect(state.queue.containsKey('old-id'), isFalse);
      expect(state.queue.containsKey('new-id'), isTrue);
      // Old event is kept in event store (merged), new event is present
      expect(state.events.containsKey('old-id'), isTrue);
      expect(state.events.containsKey('new-id'), isTrue);
    });

    test('duplicate event ignores event payload but updates entry', () {
      final state = AgentDeskState();
      final eventA = createTestEvent(
        id: 'dup-id',
        seq: 1,
        category: Category.request,
        summary: 'Original Summary',
      );
      state.applyEvent(EventPush(
        event: eventA,
        entry: QueueEntry(
          eventId: 'dup-id',
          tier: Category.request.tier,
          score: 50,
          state: EntryState.new_,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ));
      expect(state.eventFor('dup-id')?.summary, 'Original Summary');
      expect(state.entryFor('dup-id')?.score, 50);

      // Send duplicate with changed summary and updated score
      final eventADup = createTestEvent(
        id: 'dup-id',
        seq: 1,
        category: Category.request,
        summary: 'Altered Summary That Should Be Ignored',
      );
      state.applyEvent(EventPush(
        event: eventADup,
        entry: QueueEntry(
          eventId: 'dup-id',
          tier: Category.request.tier,
          score: 95,
          state: EntryState.seen,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ));

      // Event payload is unchanged (ignored)
      expect(state.eventFor('dup-id')?.summary, 'Original Summary');
      // Queue entry is updated
      expect(state.entryFor('dup-id')?.score, 95);
      expect(state.entryFor('dup-id')?.state, EntryState.seen);
    });

    test('score_update and state_update ignore unknown ids', () {
      final state = AgentDeskState();
      state.applyScoreUpdate(const ScoreUpdate(eventId: 'unknown', score: 100, escalationLevel: 2));
      expect(state.queue.isEmpty, isTrue);

      state.applyStateUpdate(const StateUpdate(eventId: 'unknown', state: EntryState.seen));
      expect(state.queue.isEmpty, isTrue);
    });

    test('state_update modifies existing entry state, resolution, and superseded', () {
      final state = AgentDeskState();
      final ev = createTestEvent(id: 'req-1', seq: 1, category: Category.request);
      state.applyEvent(EventPush(
        event: ev,
        entry: QueueEntry(
          eventId: 'req-1',
          tier: Category.request.tier,
          score: 80,
          state: EntryState.new_,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ));

      state.applyStateUpdate(const StateUpdate(
        eventId: 'req-1',
        state: EntryState.dismissed,
        resolution: Resolution.approved,
        superseded: false,
      ));

      final updated = state.entryFor('req-1')!;
      expect(updated.state, EntryState.dismissed);
      expect(updated.resolution, Resolution.approved);
      expect(updated.isLive, isFalse);
      // Since it's approved and dismissed, it should no longer be displayed
      expect(state.entries.isEmpty, isTrue);
    });

    test('dismissed unresolved request remains visible on home', () {
      final state = AgentDeskState();
      final ev = createTestEvent(id: 'req-2', seq: 1, category: Category.request);
      state.applyEvent(EventPush(
        event: ev,
        entry: QueueEntry(
          eventId: 'req-2',
          tier: Category.request.tier,
          score: 80,
          state: EntryState.new_,
          resolution: Resolution.unresolved,
          seq: 1,
        ),
      ));

      // Dismiss without resolving
      state.applyStateUpdate(const StateUpdate(
        eventId: 'req-2',
        state: EntryState.dismissed,
        resolution: Resolution.unresolved,
      ));

      // Still in entries!
      expect(state.entries.length, 1);
      expect(state.entriesForTier(0).length, 1);
    });
  });

  group('P7.T3 - Queue ordering matches laptop rule', () {
    test('tier ordering: requests (0) < errors (1) < completed (2) < working (3)', () {
      final entries = [
        QueueEntry(eventId: 'w', tier: 3, score: 99, state: EntryState.new_, seq: 10),
        QueueEntry(eventId: 'c', tier: 2, score: 99, state: EntryState.new_, seq: 9),
        QueueEntry(eventId: 'e', tier: 1, score: 10, state: EntryState.new_, seq: 8),
        QueueEntry(eventId: 'r', tier: 0, score: 5, state: EntryState.new_, seq: 7),
      ];
      entries.sort();
      expect(entries.map((e) => e.eventId).toList(), ['r', 'e', 'c', 'w']);
    });

    test('score tie-breaking: higher score first, then newer seq first', () {
      final entries = [
        QueueEntry(eventId: 'w1', tier: 3, score: 50, state: EntryState.new_, seq: 1),
        QueueEntry(eventId: 'w2', tier: 3, score: 80, state: EntryState.new_, seq: 2),
        QueueEntry(eventId: 'w3', tier: 3, score: 80, state: EntryState.new_, seq: 5),
      ];
      entries.sort();
      // w3 (tier 3, score 80, seq 5) > w2 (tier 3, score 80, seq 2) > w1 (tier 3, score 50, seq 1)
      expect(entries.map((e) => e.eventId).toList(), ['w3', 'w2', 'w1']);
    });

    test('replays first 50 lines of agentdesk.golden deterministically', () {
      final goldenFile = File('../core/agentdesk-core/golden/agentdesk.golden');
      if (!goldenFile.existsSync()) {
        return; // Skip if run from a different relative root
      }
      final lines = goldenFile.readAsLinesSync().take(50);
      final state = AgentDeskState();
      for (final line in lines) {
        final map = jsonDecode(line) as Map<String, dynamic>;
        final msg = Message.fromJson(map);
        if (msg.type == 'event') {
          state.applyEvent(msg.payload as EventPush);
        } else if (msg.type == 'score_update') {
          state.applyScoreUpdate(msg.payload as ScoreUpdate);
        } else if (msg.type == 'state_update') {
          state.applyStateUpdate(msg.payload as StateUpdate);
        }
      }

      final displayed = state.entries;
      for (int i = 0; i < displayed.length - 1; i++) {
        final a = displayed[i];
        final b = displayed[i + 1];
        // Must be in sorted order
        expect(a.compareTo(b) <= 0, isTrue,
            reason: 'Entry at $i (${a.tier}, ${a.score}, ${a.seq}) should precede or equal entry at ${i + 1} (${b.tier}, ${b.score}, ${b.seq})');
      }
    });
  });
}
