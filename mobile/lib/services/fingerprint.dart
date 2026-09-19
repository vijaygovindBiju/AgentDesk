import 'package:crypto/crypto.dart';

/// Verify whether the SHA-256 fingerprint of [certDer] matches [pinnedFingerprint].
///
/// Accepts fingerprints formatted with colons (e.g. `AA:BB:CC:...`) or continuous hex.
/// Ignores whitespace, colons, and case differences.
bool verifyCertificateFingerprint(List<int> certDer, String pinnedFingerprint) {
  final cleanPinned = pinnedFingerprint
      .replaceAll(':', '')
      .replaceAll(' ', '')
      .toLowerCase()
      .trim();

  if (cleanPinned.length != 64) {
    return false;
  }

  final digest = sha256.convert(certDer).toString().toLowerCase();
  return digest == cleanPinned;
}

/// Format a DER certificate's SHA-256 fingerprint into standard colon-separated hex `AA:BB:CC:...`.
String formatFingerprint(List<int> certDer) {
  final digest = sha256.convert(certDer).toString().toUpperCase();
  final buffer = StringBuffer();
  for (int i = 0; i < digest.length; i += 2) {
    if (i > 0) buffer.write(':');
    buffer.write(digest.substring(i, i + 2));
  }
  return buffer.toString();
}
