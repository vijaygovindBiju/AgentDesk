import 'package:flutter/foundation.dart' hide Category;
import '../models/models.dart';

class AgentDeskState extends ChangeNotifier {
  final Map<String, Event> _events = {};
  final Map<String, QueueEntry> _queue = {};
  final Set<String> _finishedEventIds = {};

  Welcome? _welcome;
  Welcome? get welcome => _welcome;

  Map<String, Event> get events => Map.unmodifiable(_events);
  Map<String, QueueEntry> get queue => Map.unmodifiable(_queue);

  bool isFinished(String eventId) => _finishedEventIds.contains(eventId);

  int get activeWorkingCount {
    return _queue.values
        .where((e) =>
            e.tier == 3 &&
            _events[e.eventId]?.request == null &&
            _shouldDisplay(e) &&
            !isFinished(e.eventId))
        .length;
  }

  int get finishedWorkingCount {
    return _queue.values
        .where((e) => e.tier == 3 && isFinished(e.eventId))
        .length;
  }

  bool isRequestEntry(QueueEntry entry) {
    final ev = _events[entry.eventId];
    if (ev != null) {
      if (ev.category == Category.completed || ev.kind == 'task_completed') {
        return false;
      }
      if (ev.request != null || ev.category == Category.request) {
        return true;
      }
    }
    return entry.tier == 0;
  }

  int get pendingRequestCount {
    return _queue.values
        .where((e) =>
            isRequestEntry(e) &&
            _shouldDisplay(e) &&
            (e.resolution == Resolution.unresolved ||
                e.resolution == Resolution.pendingDelivery))
        .length;
  }

  List<QueueEntry> get pendingRequests {
    final list = _queue.values
        .where((e) =>
            isRequestEntry(e) &&
            _shouldDisplay(e) &&
            (e.resolution == Resolution.unresolved ||
                e.resolution == Resolution.pendingDelivery))
        .toList();
    list.sort();
    return list;
  }

  void dismissAllFinishedWorking() {
    bool changed = false;
    for (final entry in _queue.values) {
      if (entry.tier == 3 &&
          isFinished(entry.eventId) &&
          entry.state != EntryState.dismissed) {
        _queue[entry.eventId] = entry.copyWith(state: EntryState.dismissed);
        changed = true;
      }
    }
    if (changed) notifyListeners();
  }

  /// Return all entries to display, ordered by (tier asc, score desc, seq desc).
  List<QueueEntry> get entries {
    final list = _queue.values.where(_shouldDisplay).toList();
    list.sort();
    return list;
  }

  bool _shouldDisplay(QueueEntry entry) {
    if (entry.isLive) return true;
    
    final isReq = isRequestEntry(entry);
    // Dismissed requests that are still unresolved or pending delivery remain visible in Requests tier.
    if (isReq &&
        entry.state == EntryState.dismissed &&
        (entry.resolution == Resolution.unresolved ||
            entry.resolution == Resolution.pendingDelivery) &&
        !entry.superseded) {
      return true;
    }

    // Hide working entries (tier==3) that are both finished AND dismissed.
    // This implies that dismissed working entries that are NOT finished should remain visible.
    if (entry.tier == 3 && _events[entry.eventId]?.request == null && entry.state == EntryState.dismissed && !entry.superseded) {
      return !isFinished(entry.eventId);
    }
    
    return false;
  }

  List<QueueEntry> entriesForTier(int tier) {
    final list = _queue.values
        .where((e) {
          final isReq = isRequestEntry(e);
          if (tier == 0) {
            return isReq && _shouldDisplay(e);
          }
          if (isReq) {
            return false;
          }
          return e.tier == tier && _shouldDisplay(e);
        })
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
    
    _finishedEventIds.clear();
    for (final event in _events.values) {
      if (event.category == Category.working) {
        bool hasFinished = event.taskId != null &&
            _events.values.any((e) =>
                (e.category == Category.completed || e.category == Category.error) &&
                e.agentId == event.agentId &&
                e.taskId == event.taskId);
        if (hasFinished) {
          _finishedEventIds.add(event.eventId);
        }
      }
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

    if ((eventPush.event.category == Category.completed ||
            eventPush.event.category == Category.error) &&
        eventPush.event.taskId != null) {
      for (final e in _events.values) {
        if (e.category == Category.working &&
            e.agentId == eventPush.event.agentId &&
            e.taskId == eventPush.event.taskId) {
          _finishedEventIds.add(e.eventId);
        }
      }
    }

    // Reverse check: if a working event arrives after its completion/error
    // event (out-of-order delivery), mark it as finished immediately.
    if (eventPush.event.category == Category.working &&
        eventPush.event.taskId != null) {
      final alreadyFinished = _events.values.any((e) =>
          (e.category == Category.completed || e.category == Category.error) &&
          e.agentId == eventPush.event.agentId &&
          e.taskId == eventPush.event.taskId);
      if (alreadyFinished) {
        _finishedEventIds.add(eventId);
      }
    }

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
    _finishedEventIds.clear();
    _welcome = null;
    notifyListeners();
  }
}
