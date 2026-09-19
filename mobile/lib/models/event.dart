const int schemaVersion = 1;

/// The four attention categories. Tier order is fixed: lower sorts first.
enum Category {
  request(0, 'request'),
  error(1, 'error'),
  completed(2, 'completed'),
  working(3, 'working');

  const Category(this.tier, this.wireName);
  final int tier;
  final String wireName;

  static Category fromJson(String value) {
    switch (value) {
      case 'request':
        return Category.request;
      case 'error':
        return Category.error;
      case 'completed':
        return Category.completed;
      case 'working':
        return Category.working;
      default:
        throw ArgumentError('Unknown category: $value');
    }
  }

  String toJson() => wireName;
}

/// Severity 0..=3, serialised as a plain integer.
enum Severity {
  routine(0),
  notable(1),
  important(2),
  critical(3);

  const Severity(this.value);
  final int value;

  static Severity fromJson(dynamic value) {
    final int val = value is int ? value : int.parse(value.toString());
    switch (val) {
      case 0:
        return Severity.routine;
      case 1:
        return Severity.notable;
      case 2:
        return Severity.important;
      case 3:
        return Severity.critical;
      default:
        throw ArgumentError('Severity out of range: $val');
    }
  }

  int toJson() => value;
}

/// Kind of operation a task performs.
enum Operation {
  build('build'),
  test('test'),
  install('install'),
  analyze('analyze'),
  edit('edit'),
  other('other');

  const Operation(this.wireName);
  final String wireName;

  static Operation fromJson(String value) {
    switch (value) {
      case 'build':
        return Operation.build;
      case 'test':
        return Operation.test;
      case 'install':
        return Operation.install;
      case 'analyze':
        return Operation.analyze;
      case 'edit':
        return Operation.edit;
      case 'other':
        return Operation.other;
      default:
        throw ArgumentError('Unknown operation: $value');
    }
  }

  String toJson() => wireName;
}

/// Reference into the per-agent log store.
class LogRange {
  final int start;
  final int end;
  final bool pinned;

  const LogRange({
    required this.start,
    required this.end,
    required this.pinned,
  });

  factory LogRange.fromJson(Map<String, dynamic> json) {
    return LogRange(
      start: (json['start'] as num).toInt(),
      end: (json['end'] as num).toInt(),
      pinned: json['pinned'] as bool? ?? false,
    );
  }

  Map<String, dynamic> toJson() => {
    'start': start,
    'end': end,
    'pinned': pinned,
  };

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is LogRange &&
          runtimeType == other.runtimeType &&
          start == other.start &&
          end == other.end &&
          pinned == other.pinned;

  @override
  int get hashCode => start.hashCode ^ end.hashCode ^ pinned.hashCode;
}

/// Present only on `Category.request` events.
class RequestInfo {
  final String prompt;
  final List<String> options;

  const RequestInfo({
    required this.prompt,
    required this.options,
  });

  factory RequestInfo.fromJson(Map<String, dynamic> json) {
    return RequestInfo(
      prompt: json['prompt'] as String,
      options: (json['options'] as List<dynamic>?)
              ?.map((e) => e.toString())
              .toList() ??
          const [],
    );
  }

  Map<String, dynamic> toJson() => {
    'prompt': prompt,
    'options': options,
  };

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is RequestInfo &&
          runtimeType == other.runtimeType &&
          prompt == other.prompt &&
          _listEquals(options, other.options);

  @override
  int get hashCode => prompt.hashCode ^ options.hashCode;
}

/// What an adapter produces.
class RawAgentEvent {
  final String agentId;
  final int agentSeq;
  final String? taskId;
  final String kind;
  final Operation operation;
  final String message;
  final Map<String, dynamic> details;
  final List<String> logLines;
  final RequestInfo? request;

  const RawAgentEvent({
    required this.agentId,
    required this.agentSeq,
    this.taskId,
    required this.kind,
    required this.operation,
    required this.message,
    this.details = const {},
    this.logLines = const [],
    this.request,
  });

  factory RawAgentEvent.fromJson(Map<String, dynamic> json) {
    return RawAgentEvent(
      agentId: json['agent_id'] as String,
      agentSeq: (json['agent_seq'] as num).toInt(),
      taskId: json['task_id'] as String?,
      kind: json['kind'] as String,
      operation: Operation.fromJson(json['operation'] as String),
      message: json['message'] as String,
      details: json['details'] != null
          ? (json['details'] as Map).cast<String, dynamic>()
          : const {},
      logLines: (json['log_lines'] as List<dynamic>?)
              ?.map((e) => e.toString())
              .toList() ??
          const [],
      request: json['request'] != null
          ? RequestInfo.fromJson(
              (json['request'] as Map).cast<String, dynamic>())
          : null,
    );
  }

  Map<String, dynamic> toJson() => {
    'agent_id': agentId,
    'agent_seq': agentSeq,
    if (taskId != null) 'task_id': taskId,
    'kind': kind,
    'operation': operation.toJson(),
    'message': message,
    'details': details,
    'log_lines': logLines,
    if (request != null) 'request': request!.toJson(),
  };
}

/// Immutable processed event.
class Event {
  final int schemaVersion;
  final String eventId;
  final int seq;
  final int agentSeq;
  final String agentId;
  final String agentName;
  final String project;
  final String? taskId;
  final DateTime ts;
  final Category category;
  final Severity severity;
  final String kind;
  final Operation operation;
  final String summary;
  final String message;
  final Map<String, dynamic> details;
  final LogRange logRange;
  final RequestInfo? request;

  const Event({
    required this.schemaVersion,
    required this.eventId,
    required this.seq,
    required this.agentSeq,
    required this.agentId,
    required this.agentName,
    required this.project,
    this.taskId,
    required this.ts,
    required this.category,
    required this.severity,
    required this.kind,
    required this.operation,
    required this.summary,
    required this.message,
    this.details = const {},
    required this.logRange,
    this.request,
  });

  factory Event.fromJson(Map<String, dynamic> json) {
    return Event(
      schemaVersion: (json['schema_version'] as num).toInt(),
      eventId: json['event_id'] as String,
      seq: (json['seq'] as num).toInt(),
      agentSeq: (json['agent_seq'] as num).toInt(),
      agentId: json['agent_id'] as String,
      agentName: json['agent_name'] as String,
      project: json['project'] as String,
      taskId: json['task_id'] as String?,
      ts: DateTime.parse(json['ts'] as String).toUtc(),
      category: Category.fromJson(json['category'] as String),
      severity: Severity.fromJson(json['severity']),
      kind: json['kind'] as String,
      operation: Operation.fromJson(json['operation'] as String),
      summary: json['summary'] as String,
      message: json['message'] as String,
      details: json['details'] != null
          ? (json['details'] as Map).cast<String, dynamic>()
          : const {},
      logRange: LogRange.fromJson(
          (json['log_range'] as Map).cast<String, dynamic>()),
      request: json['request'] != null
          ? RequestInfo.fromJson(
              (json['request'] as Map).cast<String, dynamic>())
          : null,
    );
  }

  Map<String, dynamic> toJson() => {
    'schema_version': schemaVersion,
    'event_id': eventId,
    'seq': seq,
    'agent_seq': agentSeq,
    'agent_id': agentId,
    'agent_name': agentName,
    'project': project,
    if (taskId != null) 'task_id': taskId,
    'ts': ts.toUtc().toIso8601String(),
    'category': category.toJson(),
    'severity': severity.toJson(),
    'kind': kind,
    'operation': operation.toJson(),
    'summary': summary,
    'message': message,
    'details': details,
    'log_range': logRange.toJson(),
    if (request != null) 'request': request!.toJson(),
  };

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is Event &&
          runtimeType == other.runtimeType &&
          schemaVersion == other.schemaVersion &&
          eventId == other.eventId &&
          seq == other.seq &&
          agentSeq == other.agentSeq &&
          agentId == other.agentId &&
          agentName == other.agentName &&
          project == other.project &&
          taskId == other.taskId &&
          ts.isAtSameMomentAs(other.ts) &&
          category == other.category &&
          severity == other.severity &&
          kind == other.kind &&
          operation == other.operation &&
          summary == other.summary &&
          message == other.message &&
          logRange == other.logRange &&
          request == other.request;

  @override
  int get hashCode =>
      eventId.hashCode ^
      seq.hashCode ^
      category.hashCode ^
      severity.hashCode;
}

bool _listEquals<T>(List<T> a, List<T> b) {
  if (a.length != b.length) return false;
  for (int i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}
