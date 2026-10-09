import 'dart:io';

/// AgentDesk is relay-only: the phone reaches the laptop exclusively through a
/// publicly reachable relay over outbound WSS. It must never target a direct
/// private address, a laptop hostname, or (in production) localhost.
bool isLoopbackHost(String host) {
  final h = host.toLowerCase();
  return h == '127.0.0.1' || h == '::1' || h == 'localhost';
}

bool isPrivateNetworkHost(String host) {
  final h = host.toLowerCase();
  if (h.endsWith('.local') || h.endsWith('.internal') || h.endsWith('.home')) {
    return true;
  }
  final ip = InternetAddress.tryParse(h);
  if (ip == null) return false;
  final b = ip.rawAddress;
  if (ip.type == InternetAddressType.IPv4) {
    return b[0] == 10 ||
        (b[0] == 172 && b[1] >= 16 && b[1] <= 31) ||
        (b[0] == 192 && b[1] == 168) ||
        (b[0] == 169 && b[1] == 254) ||
        (b[0] == 100 && b[1] >= 64 && b[1] <= 127) ||
        b.every((x) => x == 0);
  }
  return (b[0] & 0xfe) == 0xfc || (b[0] == 0xfe && (b[1] & 0xc0) == 0x80);
}

// Deprecated alias for backwards-compatibility
bool isPrivateOrLanHost(String host) => isPrivateNetworkHost(host);

/// Returns a user-facing error, or null if [url] is an acceptable relay.
/// Loopback relays (`ws://127.0.0.1:8766`) are a development-only
/// configuration and are accepted only when [allowLoopback] is set.
String? validateRelayUrl(String url, {bool allowLoopback = false}) {
  final uri = Uri.tryParse(url.trim());
  if (uri == null || !uri.hasScheme || uri.host.isEmpty) {
    return 'Relay URL is invalid.';
  }
  final loopback = isLoopbackHost(uri.host);
  if (loopback) {
    return allowLoopback
        ? null
        : 'Relay URL points at localhost. Use a public relay (wss://...).';
  }
  if (uri.scheme != 'wss') {
    return 'Relay URL must use wss:// (TLS).';
  }
  if (isPrivateNetworkHost(uri.host)) {
    return 'Relay host ${uri.host} is a private network address and is unreachable '
        'from mobile data. Use a public relay.';
  }
  return null;
}
