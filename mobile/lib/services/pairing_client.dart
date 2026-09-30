import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:web_socket_channel/io.dart';
import '../models/message.dart';
import 'relay_url.dart';

/// The QR payload structure carried in the terminal QR code.
/// Format: `agentdesk://pair?v=1&inst=...&sess=...&sec=...&fp=...&relay=...`
class PairingPayload {
  final int version;
  final String instanceId;
  final String sessionId;
  final String pairingSecret;
  final String fingerprint;
  final String relayUrl;

  const PairingPayload({
    required this.version,
    required this.instanceId,
    required this.sessionId,
    required this.pairingSecret,
    required this.fingerprint,
    required this.relayUrl,
  });

  static PairingPayload? tryParse(String rawUri) {
    try {
      return parse(rawUri);
    } catch (_) {
      return null;
    }
  }

  static PairingPayload parse(String rawUri) {
    final trimmed = rawUri.trim();
    if (!trimmed.startsWith('agentdesk://pair?') &&
        !trimmed.startsWith('agentdesk://pair/?')) {
      throw const FormatException(
        'Invalid pairing URI scheme; expected agentdesk://pair?...',
      );
    }

    final uri = Uri.parse(trimmed);
    final params = uri.queryParameters;

    final vStr = params['v'];
    if (vStr == null) {
      throw const FormatException('Missing version in pairing URI');
    }
    final version = int.tryParse(vStr);
    if (version == null || version != 1) {
      throw FormatException('Unsupported pairing protocol version: $vStr');
    }

    final instanceId = params['inst']?.trim();
    if (instanceId == null || instanceId.isEmpty) {
      throw const FormatException('Missing instance ID in pairing URI');
    }

    final sessionId = params['sess']?.trim();
    if (sessionId == null || sessionId.isEmpty) {
      throw const FormatException('Missing session ID in pairing URI');
    }

    final sec = params['sec']?.trim();
    if (sec == null ||
        sec.length != 64 ||
        !RegExp(r'^[0-9a-fA-F]+$').hasMatch(sec)) {
      throw const FormatException(
        'Invalid pairing secret in pairing URI; must be 64-char hex',
      );
    }

    final fp = params['fp']?.trim();
    if (fp == null || fp.isEmpty) {
      throw const FormatException('Missing fingerprint in pairing URI');
    }

    // Relay-only: the QR must carry the relay endpoint. Legacy `lan=` values
    // are ignored; the phone never connects to a laptop LAN address.
    final relay = params['relay']?.trim();
    if (relay == null || relay.isEmpty) {
      throw const FormatException('Pairing QR code has no relay endpoint');
    }
    final resolvedRelay = relay;

    return PairingPayload(
      version: version,
      instanceId: instanceId,
      sessionId: sessionId,
      pairingSecret: sec,
      fingerprint: fp,
      relayUrl: resolvedRelay,
    );
  }

  String toUriString() {
    final buffer = StringBuffer('agentdesk://pair?v=$version')
      ..write('&inst=${Uri.encodeComponent(instanceId)}')
      ..write('&sess=${Uri.encodeComponent(sessionId)}')
      ..write('&sec=${Uri.encodeComponent(pairingSecret)}')
      ..write('&fp=${Uri.encodeComponent(fingerprint)}')
      ..write('&relay=${Uri.encodeComponent(relayUrl)}');
    return buffer.toString();
  }
}

class PairingResult {
  final bool success;
  final String? instanceId;
  final String? deviceToken;
  final String? fingerprint;
  final String? relayUrl;
  final String? error;

  const PairingResult.success({
    required this.instanceId,
    required this.deviceToken,
    required this.fingerprint,
    required this.relayUrl,
  })  : success = true,
        error = null;

  const PairingResult.failure(this.error)
      : success = false,
        instanceId = null,
        deviceToken = null,
        fingerprint = null,
        relayUrl = null;
}

class PairingClient {
  const PairingClient();

  Future<PairingResult> pair({
    required PairingPayload payload,
    required String deviceId,
    required String deviceName,
    Duration timeout = const Duration(seconds: 15),
  }) async {
    final relay = payload.relayUrl.trim();
    if (relay.isEmpty) {
      return const PairingResult.failure(
        'No relay URL provided in pairing QR code',
      );
    }
    final relayProblem = validateRelayUrl(relay, allowLoopback: kDebugMode);
    if (relayProblem != null) {
      return PairingResult.failure(relayProblem);
    }

    final cleanRelay = relay.replaceAll(RegExp(r'/+$'), '');
    final relayUri = Uri.parse(
      '$cleanRelay/ws?role=pair&inst=${Uri.encodeComponent(payload.instanceId)}&sess=${Uri.encodeComponent(payload.sessionId)}&sec=${Uri.encodeComponent(payload.pairingSecret)}',
    );
    try {
      debugPrint('Attempting remote pairing via relay: ${relayUri.host}...');
      final ws = await WebSocket.connect(relayUri.toString()).timeout(timeout);
      return await _performPairingExchange(
        ws: ws,
        payload: payload,
        deviceId: deviceId,
        deviceName: deviceName,
        timeout: timeout,
      );
    } catch (e) {
      debugPrint('Relay pairing failed or unreachable ($e)');
      final errStr = e.toString().toLowerCase();
      if (e is SocketException ||
          errStr.contains('failed host lookup') ||
          errStr.contains('no address associated with hostname') ||
          errStr.contains('socketexception') ||
          errStr.contains('network is unreachable')) {
        return PairingResult.failure(
          'Could not reach relay server at ${relayUri.host}. Please verify the relay hostname is valid and reachable over the internet.',
        );
      }
      return PairingResult.failure('Relay pairing failed: $e');
    }
  }

  Future<PairingResult> _performPairingExchange({
    required WebSocket ws,
    required PairingPayload payload,
    required String deviceId,
    required String deviceName,
    required Duration timeout,
  }) async {
    try {
      final channel = IOWebSocketChannel(ws);
      final completer = Completer<PairingResult>();

      final req = Message.push(
        'pair_request',
        PairRequest(
          deviceId: deviceId,
          deviceName: deviceName,
          sessionId: payload.sessionId,
          pairingSecret: payload.pairingSecret,
          schemaVersion: payload.version,
        ),
      );

      channel.sink.add(jsonEncode(req.toJson()));

      late final StreamSubscription subscription;
      subscription = channel.stream.listen(
        (data) {
          try {
            final text = data is String ? data : utf8.decode(data as List<int>);
            final map = jsonDecode(text) as Map<String, dynamic>;
            final msg = Message.fromJson(map);
            if (msg.type == 'pair_response' && msg.payload is PairResponse) {
              final resp = msg.payload as PairResponse;
              if (!completer.isCompleted) {
                completer.complete(
                  PairingResult.success(
                    instanceId: resp.instanceId,
                    deviceToken: resp.deviceToken,
                    fingerprint: payload.fingerprint,
                    relayUrl: payload.relayUrl,
                  ),
                );
              }
            } else if (msg.type == 'error') {
              final err = msg.payload is ErrorReply
                  ? (msg.payload as ErrorReply).message
                  : 'Pairing error';
              if (!completer.isCompleted) {
                completer.complete(PairingResult.failure(err));
              }
            }
          } catch (e) {
            if (!completer.isCompleted) {
              completer.complete(
                PairingResult.failure('Failed to parse server response: $e'),
              );
            }
          }
        },
        onError: (err) {
          if (!completer.isCompleted) {
            completer.complete(
              PairingResult.failure('Pairing connection error: $err'),
            );
          }
        },
        onDone: () {
          final code = channel.closeCode;
          final reason = channel.closeReason;
          if (!completer.isCompleted) {
            if (code == 4001) {
              completer.complete(
                PairingResult.failure('Pairing rejected: $reason'),
              );
            } else if (code == 4002) {
              completer.complete(
                PairingResult.failure('Protocol version mismatch: $reason'),
              );
            } else {
              completer.complete(
                PairingResult.failure('Connection closed: ${reason ?? 'Code $code'}'),
              );
            }
          }
        },
        cancelOnError: true,
      );

      final result = await completer.future.timeout(
        timeout,
        onTimeout: () {
          subscription.cancel();
          channel.sink.close();
          return const PairingResult.failure(
            'Pairing timed out waiting for server confirmation',
          );
        },
      );

      await subscription.cancel();
      await channel.sink.close();
      return result;
    } catch (e) {
      return PairingResult.failure('Pairing message exchange failed: $e');
    }
  }
}
