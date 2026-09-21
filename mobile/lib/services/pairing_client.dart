import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:web_socket_channel/io.dart';
import '../models/message.dart';
import 'fingerprint.dart';

/// The QR payload structure carried in the terminal QR code.
/// Format: `agentdesk://pair?v=1&inst=...&sess=...&sec=...&fp=...[&lan=...][&relay=...]`
class PairingPayload {
  final int version;
  final String instanceId;
  final String sessionId;
  final String pairingSecret;
  final String fingerprint;
  final String? lanAddress;
  final String? relayUrl;

  const PairingPayload({
    required this.version,
    required this.instanceId,
    required this.sessionId,
    required this.pairingSecret,
    required this.fingerprint,
    this.lanAddress,
    this.relayUrl,
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

    final lan = params['lan']?.trim();
    final relay = params['relay']?.trim();

    return PairingPayload(
      version: version,
      instanceId: instanceId,
      sessionId: sessionId,
      pairingSecret: sec,
      fingerprint: fp,
      lanAddress: lan?.isEmpty ?? true ? null : lan,
      relayUrl: relay?.isEmpty ?? true ? null : relay,
    );
  }

  String toUriString() {
    final buffer = StringBuffer('agentdesk://pair?v=$version')
      ..write('&inst=${Uri.encodeComponent(instanceId)}')
      ..write('&sess=${Uri.encodeComponent(sessionId)}')
      ..write('&sec=${Uri.encodeComponent(pairingSecret)}')
      ..write('&fp=${Uri.encodeComponent(fingerprint)}');
    if (lanAddress != null && lanAddress!.isNotEmpty) {
      buffer.write('&lan=${Uri.encodeComponent(lanAddress!)}');
    }
    if (relayUrl != null && relayUrl!.isNotEmpty) {
      buffer.write('&relay=${Uri.encodeComponent(relayUrl!)}');
    }
    return buffer.toString();
  }
}

class PairingResult {
  final bool success;
  final String? instanceId;
  final String? deviceToken;
  final String? lanAddress;
  final String? fingerprint;
  final String? relayUrl;
  final String? error;

  const PairingResult.success({
    required this.instanceId,
    required this.deviceToken,
    this.lanAddress,
    required this.fingerprint,
    this.relayUrl,
  })  : success = true,
        error = null;

  const PairingResult.failure(this.error)
      : success = false,
        instanceId = null,
        deviceToken = null,
        lanAddress = null,
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
    final lan = payload.lanAddress;
    if (lan == null || lan.isEmpty) {
      return const PairingResult.failure(
        'No connection endpoint provided in pairing QR code',
      );
    }

    final targetUrl = 'wss://$lan';
    final uri = Uri.parse(targetUrl);

    try {
      final client = HttpClient();
      client.badCertificateCallback = (cert, host, port) {
        return verifyCertificateFingerprint(cert.der, payload.fingerprint);
      };

      final ws = await WebSocket.connect(
        uri.toString(),
        customClient: client,
      ).timeout(
        timeout,
        onTimeout: () {
          throw TimeoutException('Connection timed out connecting to $targetUrl');
        },
      );

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
                    lanAddress: payload.lanAddress,
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
    } on SocketException catch (e) {
      return PairingResult.failure(
        'Could not reach laptop at ${payload.lanAddress}. '
        'Ensure both your phone and laptop are connected to the same Wi-Fi network. ($e)',
      );
    } on HandshakeException catch (e) {
      return PairingResult.failure(
        'TLS security check failed. The laptop certificate did not match the QR code fingerprint. ($e)',
      );
    } catch (e) {
      return PairingResult.failure('Pairing failed: $e');
    }
  }
}
