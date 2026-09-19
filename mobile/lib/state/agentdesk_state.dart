import 'package:flutter/foundation.dart';
import '../models/models.dart';

class AgentDeskState extends ChangeNotifier {
  final Map<String, Event> _events = {};
  final Map<String, QueueEntry> _queue = {};

  Welcome? _welcome;
  Welcome? get welcome => _welcome;

  Map<String, Event> get events => Map.unmodifiable(_events);
  Map<String, QueueEntry> get queue => Map.unmodifiable(_queue);

  /// Return all entries to display, ordered by (tier asc, score desc, seq desc).
  List<QueueEntry> get entries {
    final list = _queue.values.where(_shouldDisplay).toList();
    list.sort();
    return list;
  }

  static bool _shouldDisplay(QueueEntry entry) {
    if (entry.isLive) return true;
    // Dismissed requests that are still unresolved remain visible in Requests tier.
    if (entry.tier == 0 &&
        entry.state == EntryState.dismissed &&
        entry.resolution == Resolution.unresolved &&
        !entry.superseded) {
      return true;
    }
    return false;
  }

  List<QueueEntry> entriesForTier(int tier) {
    final list = _queue.values
        .where((e) => e.tier == tier && _shouldDisplay(e))
        .toList();
    list.sort();
    return list;
  }

  Event? eventFor(String eventId) => _events[eventId];
  QueueEntry? entryFor(String eventId) => _queue[eventId];

  void setWelcome(Welcome welcome) {
    _welcome = welcome;
    notifyListeners();
  }

  /// On snapshot: wholesale replace of queue, merge events.
  void applySnapshot(Snapshot snapshot) {
    _queue.clear();
    for (final entry in snapshot.entries) {
      _queue[entry.eventId] = entry;
    }
    for (final event in snapshot.events) {
      _events[event.eventId] = event;
    }
    notifyListeners();
  }

  /// On event: if known event_id, ignore event payload but apply entry.
  /// If unknown event_id, add event and apply entry.
  void applyEvent(EventPush eventPush) {
    final eventId = eventPush.event.eventId;
    if (!_events.containsKey(eventId)) {
      _events[eventId] = eventPush.event;
    }
    _queue[eventPush.entry.eventId] = eventPush.entry;
    notifyListeners();
  }

  /// On score_update: ignore if event_id is not in queue.
  void applyScoreUpdate(ScoreUpdate update) {
    final current = _queue[update.eventId];
    if (current == null) return;
    _queue[update.eventId] = current.copyWith(
      score: update.score,
      escalationLevel: update.escalationLevel,
    );
    notifyListeners();
  }

  /// On state_update: ignore if event_id is not in queue.
  void applyStateUpdate(StateUpdate update) {
    final current = _queue[update.eventId];
    if (current == null) return;
    _queue[update.eventId] = current.copyWith(
      state: update.state,
      resolution: update.resolution ?? current.resolution,
      superseded: update.superseded,
    );
    notifyListeners();
  }

  /// On event_details reply: update event in store, and entry in queue.
  void applyEventDetails(EventPush details) {
    _events[details.event.eventId] = details.event;
    _queue[details.entry.eventId] = details.entry;
    notifyListeners();
  }

  void clear() {
    _events.clear();
    _queue.clear();
    _welcome = null;
    notifyListeners();
  }
}
