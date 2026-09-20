import 'event.dart';
import 'queue.dart';

class CloseCode {
  static const int unauthorized = 4001;
  static const int schemaMismatch = 4002;
  static const int slowClient = 4003;
}

enum PipelineMode {
  rawLines('raw_lines'),
  rawEvents('raw_events'),
  agentdesk('agentdesk');

  const PipelineMode(this.wireName);
  final String wireName;

  static PipelineMode fromJson(String value) {
    switch (value) {
      case 'raw_lines':
        return PipelineMode.rawLines;
      case 'raw_events':
        return PipelineMode.rawEvents;
      case 'agentdesk':
        return PipelineMode.agentdesk;
      default:
        throw ArgumentError('Unknown pipeline mode: $value');
    }
  }

  String toJson() => wireName;
}

enum TransportMode {
  tls('tls'),
  insecureDev('insecure_dev');

  const TransportMode(this.wireName);
  final String wireName;

  static TransportMode fromJson(String value) {
    switch (value) {
      case 'tls':
        return TransportMode.tls;
      case 'insecure_dev':
        return TransportMode.insecureDev;
      default:
        throw ArgumentError('Unknown transport mode: $value');
    }
  }

  String toJson() => wireName;
}

enum Decision {
  approve('approve'),
  deny('deny');

  const Decision(this.wireName);
  final String wireName;

  static Decision fromJson(String value) {
    switch (value) {
      case 'approve':
        return Decision.approve;
      case 'deny':
        return Decision.deny;
      default:
        throw ArgumentError('Unknown decision: $value');
    }
  }

  String toJson() => wireName;
}

enum CommandError {
  noSuchEvent('no_such_event'),
  notARequest('not_a_request'),
  alreadyResolved('already_resolved'),
  invalid('invalid');

  const CommandError(this.wireName);
  final String wireName;

  static CommandError fromJson(String value) {
    switch (value) {
      case 'no_such_event':
        return CommandError.noSuchEvent;
      case 'not_a_request':
        return CommandError.notARequest;
      case 'already_resolved':
        return CommandError.alreadyResolved;
      case 'invalid':
        return CommandError.invalid;
      default:
        throw ArgumentError('Unknown command error: $value');
    }
  }

  String toJson() => wireName;
}

const int logOffsetTail = -1;

class LogLine {
  final int offset;
  final DateTime ts;
  final String text;

  const LogLine({required this.offset, required this.ts, required this.text});

  factory LogLine.fromJson(Map<String, dynamic> json) {
    return LogLine(
      offset: (json['offset'] as num).toInt(),
      ts: DateTime.parse(json['ts'] as String).toUtc(),
      text: json['text'] as String,
    );
  }

  Map<String, dynamic> toJson() => {
    'offset': offset,
    'ts': ts.toUtc().toIso8601String(),
    'text': text,
  };

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is LogLine &&
          runtimeType == other.runtimeType &&
          offset == other.offset &&
          ts.isAtSameMomentAs(other.ts) &&
          text == other.text;

  @override
  int get hashCode => offset.hashCode ^ text.hashCode;
}

// Handshake
class Hello {
  final String token;
  final String deviceId;
  final String clientVersion;
  final int schemaVersion;

  const Hello({
    required this.token,
    required this.deviceId,
    required this.clientVersion,
    required this.schemaVersion,
  });

  factory Hello.fromJson(Map<String, dynamic> json) {
    return Hello(
      token: json['token'] as String,
      deviceId: json['device_id'] as String,
      clientVersion: json['client_version'] as String,
      schemaVersion: (json['schema_version'] as num).toInt(),
    );
  }

  Map<String, dynamic> toJson() => {
    'token': token,
    'device_id': deviceId,
    'client_version': clientVersion,
    'schema_version': schemaVersion,
  };
}

class Welcome {
  final String daemonVersion;
  final int schemaVersion;
  final PipelineMode pipelineMode;
  final TransportMode transport;
  final DateTime serverTime;

  const Welcome({
    required this.daemonVersion,
    required this.schemaVersion,
    required this.pipelineMode,
    required this.transport,
    required this.serverTime,
  });

  factory Welcome.fromJson(Map<String, dynamic> json) {
    return Welcome(
      daemonVersion: json['daemon_version'] as String,
      schemaVersion: (json['schema_version'] as num).toInt(),
      pipelineMode: PipelineMode.fromJson(json['pipeline_mode'] as String),
      transport: TransportMode.fromJson(json['transport'] as String),
      serverTime: DateTime.parse(json['server_time'] as String).toUtc(),
    );
  }

  Map<String, dynamic> toJson() => {
    'daemon_version': daemonVersion,
    'schema_version': schemaVersion,
    'pipeline_mode': pipelineMode.toJson(),
    'transport': transport.toJson(),
    'server_time': serverTime.toUtc().toIso8601String(),
  };
}

class Snapshot {
  final List<QueueEntry> entries;
  final List<Event> events;

  const Snapshot({required this.entries, required this.events});

  factory Snapshot.fromJson(Map<String, dynamic> json) {
    return Snapshot(
      entries:
          (json['entries'] as List<dynamic>?)
              ?.map(
                (e) => QueueEntry.fromJson((e as Map).cast<String, dynamic>()),
              )
              .toList() ??
          const [],
      events:
          (json['events'] as List<dynamic>?)
              ?.map((e) => Event.fromJson((e as Map).cast<String, dynamic>()))
              .toList() ??
          const [],
    );
  }

  Map<String, dynamic> toJson() => {
    'entries': entries.map((e) => e.toJson()).toList(),
    'events': events.map((e) => e.toJson()).toList(),
  };
}

class EventPush {
  final Event event;
  final QueueEntry entry;

  const EventPush({required this.event, required this.entry});

  factory EventPush.fromJson(Map<String, dynamic> json) {
    return EventPush(
      event: Event.fromJson((json['event'] as Map).cast<String, dynamic>()),
      entry: QueueEntry.fromJson(
        (json['entry'] as Map).cast<String, dynamic>(),
      ),
    );
  }

  Map<String, dynamic> toJson() => {
    'event': event.toJson(),
    'entry': entry.toJson(),
  };
}

class ScoreUpdate {
  final String eventId;
  final int score;
  final int escalationLevel;

  const ScoreUpdate({
    required this.eventId,
    required this.score,
    required this.escalationLevel,
  });

  factory ScoreUpdate.fromJson(Map<String, dynamic> json) {
    return ScoreUpdate(
      eventId: json['event_id'] as String,
      score: (json['score'] as num).toInt(),
      escalationLevel: (json['escalation_level'] as num).toInt(),
    );
  }

  Map<String, dynamic> toJson() => {
    'event_id': eventId,
    'score': score,
    'escalation_level': escalationLevel,
  };
}

class StateUpdate {
  final String eventId;
  final EntryState state;
  final Resolution? resolution;
  final bool superseded;

  const StateUpdate({
    required this.eventId,
    required this.state,
    this.resolution,
    this.superseded = false,
  });

  factory StateUpdate.fromJson(Map<String, dynamic> json) {
    return StateUpdate(
      eventId: json['event_id'] as String,
      state: EntryState.fromJson(json['state'] as String),
      resolution: json['resolution'] != null
          ? Resolution.fromJson(json['resolution'] as String)
          : null,
      superseded: json['superseded'] as bool? ?? false,
    );
  }

  Map<String, dynamic> toJson() => {
    'event_id': eventId,
    'state': state.toJson(),
    if (resolution != null) 'resolution': resolution!.toJson(),
    'superseded': superseded,
  };
}

class RawLine {
  final String agentId;
  final int offset;
  final DateTime ts;
  final String text;

  const RawLine({
    required this.agentId,
    required this.offset,
    required this.ts,
    required this.text,
  });

  factory RawLine.fromJson(Map<String, dynamic> json) {
    return RawLine(
      agentId: json['agent_id'] as String,
      offset: (json['offset'] as num).toInt(),
      ts: DateTime.parse(json['ts'] as String).toUtc(),
      text: json['text'] as String,
    );
  }

  Map<String, dynamic> toJson() => {
    'agent_id': agentId,
    'offset': offset,
    'ts': ts.toUtc().toIso8601String(),
    'text': text,
  };
}

class EventRef {
  final String eventId;

  const EventRef({required this.eventId});

  factory EventRef.fromJson(Map<String, dynamic> json) {
    return EventRef(eventId: json['event_id'] as String);
  }

  Map<String, dynamic> toJson() => {'event_id': eventId};
}

class GetEventLogs {
  final String eventId;
  final int offset;
  final int limit;

  const GetEventLogs({
    required this.eventId,
    required this.offset,
    required this.limit,
  });

  factory GetEventLogs.fromJson(Map<String, dynamic> json) {
    return GetEventLogs(
      eventId: json['event_id'] as String,
      offset: (json['offset'] as num).toInt(),
      limit: (json['limit'] as num).toInt(),
    );
  }

  Map<String, dynamic> toJson() => {
    'event_id': eventId,
    'offset': offset,
    'limit': limit,
  };
}

class EventLogs {
  final String eventId;
  final int offset;
  final int total;
  final bool evicted;
  final List<LogLine> lines;

  const EventLogs({
    required this.eventId,
    required this.offset,
    required this.total,
    required this.evicted,
    required this.lines,
  });

  factory EventLogs.fromJson(Map<String, dynamic> json) {
    return EventLogs(
      eventId: json['event_id'] as String,
      offset: (json['offset'] as num).toInt(),
      total: (json['total'] as num).toInt(),
      evicted: json['evicted'] as bool? ?? false,
      lines:
          (json['lines'] as List<dynamic>?)
              ?.map((e) => LogLine.fromJson((e as Map).cast<String, dynamic>()))
              .toList() ??
          const [],
    );
  }

  Map<String, dynamic> toJson() => {
    'event_id': eventId,
    'offset': offset,
    'total': total,
    'evicted': evicted,
    'lines': lines.map((e) => e.toJson()).toList(),
  };
}

class RespondRequest {
  final String eventId;
  final Decision decision;
  final List<String>? selectedOptions;
  final String? textInput;

  const RespondRequest({
    required this.eventId,
    required this.decision,
    this.selectedOptions,
    this.textInput,
  });

  factory RespondRequest.fromJson(Map<String, dynamic> json) {
    return RespondRequest(
      eventId: json['event_id'] as String,
      decision: Decision.fromJson(json['decision'] as String),
      selectedOptions: (json['selected_options'] as List<dynamic>?)
          ?.map((e) => e.toString())
          .toList(),
      textInput: json['text_input'] as String?,
    );
  }

  Map<String, dynamic> toJson() => {
    'event_id': eventId,
    'decision': decision.toJson(),
    if (selectedOptions != null) 'selected_options': selectedOptions,
    if (textInput != null) 'text_input': textInput,
  };
}

class CommandResult {
  final bool ok;
  final CommandError? error;

  const CommandResult({required this.ok, this.error});

  factory CommandResult.fromJson(Map<String, dynamic> json) {
    return CommandResult(
      ok: json['ok'] as bool? ?? false,
      error: json['error'] != null
          ? CommandError.fromJson(json['error'] as String)
          : null,
    );
  }

  Map<String, dynamic> toJson() => {
    'ok': ok,
    if (error != null) 'error': error!.toJson(),
  };
}

class ErrorReply {
  final String code;
  final String message;

  const ErrorReply({required this.code, required this.message});

  factory ErrorReply.fromJson(Map<String, dynamic> json) {
    return ErrorReply(
      code: json['code'] as String? ?? 'unknown',
      message: json['message'] as String? ?? '',
    );
  }

  Map<String, dynamic> toJson() => {'code': code, 'message': message};
}

/// The envelope carrying every frame.
class Message {
  final String type;
  final String? requestId;
  final dynamic payload;

  const Message({required this.type, this.requestId, this.payload});

  factory Message.push(String type, dynamic payload) {
    return Message(type: type, requestId: null, payload: payload);
  }

  factory Message.withRequestId(
    String type,
    String requestId,
    dynamic payload,
  ) {
    return Message(type: type, requestId: requestId, payload: payload);
  }

  factory Message.request(String requestId, String type, dynamic payload) {
    return Message(type: type, requestId: requestId, payload: payload);
  }

  factory Message.fromJson(Map<String, dynamic> json) {
    final type = json['type'] as String;
    final requestId = json['request_id'] as String?;
    final rawPayload = json['payload'];

    dynamic parsedPayload;
    if (rawPayload is Map) {
      final payloadMap = rawPayload.cast<String, dynamic>();
      switch (type) {
        case 'hello':
          parsedPayload = Hello.fromJson(payloadMap);
          break;
        case 'welcome':
          parsedPayload = Welcome.fromJson(payloadMap);
          break;
        case 'snapshot':
          parsedPayload = Snapshot.fromJson(payloadMap);
          break;
        case 'event':
        case 'event_details':
          parsedPayload = EventPush.fromJson(payloadMap);
          break;
        case 'score_update':
          parsedPayload = ScoreUpdate.fromJson(payloadMap);
          break;
        case 'state_update':
          parsedPayload = StateUpdate.fromJson(payloadMap);
          break;
        case 'raw_event':
          parsedPayload = RawAgentEvent.fromJson(payloadMap);
          break;
        case 'raw_line':
          parsedPayload = RawLine.fromJson(payloadMap);
          break;
        case 'get_event_details':
        case 'ack':
        case 'dismiss':
          parsedPayload = EventRef.fromJson(payloadMap);
          break;
        case 'get_event_logs':
          parsedPayload = GetEventLogs.fromJson(payloadMap);
          break;
        case 'event_logs':
          parsedPayload = EventLogs.fromJson(payloadMap);
          break;
        case 'respond_request':
          parsedPayload = RespondRequest.fromJson(payloadMap);
          break;
        case 'command_result':
          parsedPayload = CommandResult.fromJson(payloadMap);
          break;
        case 'metrics':
          parsedPayload = Map<String, int>.from(
            payloadMap.map((k, v) => MapEntry(k, (v as num).toInt())),
          );
          break;
        case 'error':
          parsedPayload = ErrorReply.fromJson(payloadMap);
          break;
        case 'get_metrics':
          parsedPayload = const {};
          break;
        default:
          parsedPayload = rawPayload;
      }
    } else {
      parsedPayload = rawPayload;
    }

    return Message(type: type, requestId: requestId, payload: parsedPayload);
  }

  Map<String, dynamic> toJson() {
    dynamic serializedPayload;
    if (payload == null) {
      serializedPayload = <String, dynamic>{};
    } else if (payload is Map ||
        payload is List ||
        payload is String ||
        payload is num ||
        payload is bool) {
      serializedPayload = payload;
    } else {
      try {
        serializedPayload = (payload as dynamic).toJson();
      } catch (_) {
        serializedPayload = payload;
      }
    }

    return {
      'type': type,
      if (requestId != null) 'request_id': requestId,
      'payload': serializedPayload,
    };
  }
}
