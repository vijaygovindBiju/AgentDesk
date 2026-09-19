/// User-attention state. Independent of `Resolution`.
enum EntryState {
  new_('new'),
  seen('seen'),
  dismissed('dismissed');

  const EntryState(this.wireName);
  final String wireName;

  static EntryState fromJson(String value) {
    switch (value) {
      case 'new':
        return EntryState.new_;
      case 'seen':
        return EntryState.seen;
      case 'dismissed':
        return EntryState.dismissed;
      default:
        throw ArgumentError('Unknown entry state: $value');
    }
  }

  String toJson() => wireName;
}

/// Task-outcome state for `Category.request` entries only.
enum Resolution {
  unresolved('unresolved'),
  approved('approved'),
  denied('denied');

  const Resolution(this.wireName);
  final String wireName;

  static Resolution fromJson(String value) {
    switch (value) {
      case 'unresolved':
        return Resolution.unresolved;
      case 'approved':
        return Resolution.approved;
      case 'denied':
        return Resolution.denied;
      default:
        throw ArgumentError('Unknown resolution: $value');
    }
  }

  String toJson() => wireName;
}

const int maxEscalationLevel = 2;

class QueueEntry implements Comparable<QueueEntry> {
  final String eventId;
  final int tier;
  final int score;
  final EntryState state;
  final Resolution? resolution;
  final bool superseded;
  final int escalationLevel;
  final int seq;

  const QueueEntry({
    required this.eventId,
    required this.tier,
    required this.score,
    required this.state,
    this.resolution,
    this.superseded = false,
    this.escalationLevel = 0,
    required this.seq,
  });

  factory QueueEntry.fromJson(Map<String, dynamic> json) {
    return QueueEntry(
      eventId: json['event_id'] as String,
      tier: (json['tier'] as num).toInt(),
      score: (json['score'] as num).toInt(),
      state: EntryState.fromJson(json['state'] as String),
      resolution: json['resolution'] != null
          ? Resolution.fromJson(json['resolution'] as String)
          : null,
      superseded: json['superseded'] as bool? ?? false,
      escalationLevel: (json['escalation_level'] as num?)?.toInt() ?? 0,
      seq: (json['seq'] as num).toInt(),
    );
  }

  Map<String, dynamic> toJson() => {
    'event_id': eventId,
    'tier': tier,
    'score': score,
    'state': state.toJson(),
    if (resolution != null) 'resolution': resolution!.toJson(),
    'superseded': superseded,
    'escalation_level': escalationLevel,
    'seq': seq,
  };

  QueueEntry copyWith({
    String? eventId,
    int? tier,
    int? score,
    EntryState? state,
    Resolution? resolution,
    bool clearResolution = false,
    bool? superseded,
    int? escalationLevel,
    int? seq,
  }) {
    return QueueEntry(
      eventId: eventId ?? this.eventId,
      tier: tier ?? this.tier,
      score: score ?? this.score,
      state: state ?? this.state,
      resolution: clearResolution ? null : (resolution ?? this.resolution),
      superseded: superseded ?? this.superseded,
      escalationLevel: escalationLevel ?? this.escalationLevel,
      seq: seq ?? this.seq,
    );
  }

  bool get isLive => state != EntryState.dismissed && !superseded;

  /// Sort key: lower tier first, higher score first, newer seq first.
  @override
  int compareTo(QueueEntry other) {
    if (tier != other.tier) {
      return tier.compareTo(other.tier);
    }
    if (score != other.score) {
      return other.score.compareTo(score);
    }
    return other.seq.compareTo(seq);
  }

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is QueueEntry &&
          runtimeType == other.runtimeType &&
          eventId == other.eventId &&
          tier == other.tier &&
          score == other.score &&
          state == other.state &&
          resolution == other.resolution &&
          superseded == other.superseded &&
          escalationLevel == other.escalationLevel &&
          seq == other.seq;

  @override
  int get hashCode =>
      eventId.hashCode ^
      tier.hashCode ^
      score.hashCode ^
      state.hashCode ^
      seq.hashCode;
}
