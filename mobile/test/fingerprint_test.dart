import 'dart:typed_data';
import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:agentdesk/services/fingerprint.dart';

void main() {
  group('P8.T2 - Flutter fingerprint verification tests', () {
    final sampleDer = Uint8List.fromList(
      List.generate(256, (i) => (i * 17) % 256),
    );
    final expectedDigest = sha256.convert(sampleDer).toString();
    final colonSeparated = formatFingerprint(sampleDer);

    test('positive: matches exact colon-separated uppercase fingerprint', () {
      expect(verifyCertificateFingerprint(sampleDer, colonSeparated), isTrue);
    });

    test('positive: matches continuous lowercase hex', () {
      expect(verifyCertificateFingerprint(sampleDer, expectedDigest.toLowerCase()), isTrue);
    });

    test('positive: matches mixed case with spaces and colons', () {
      final messy = '  ${colonSeparated.toLowerCase().replaceAll(':', ' : ')}  ';
      expect(verifyCertificateFingerprint(sampleDer, messy), isTrue);
    });

    test('negative: rejects completely wrong fingerprint', () {
      final wrong = '00' * 32;
      expect(verifyCertificateFingerprint(sampleDer, wrong), isFalse);
    });

    test('negative: rejects single-bit flipped fingerprint', () {
      final chars = expectedDigest.split('');
      chars[0] = chars[0] == 'a' ? 'b' : 'a';
      final flipped = chars.join();
      expect(verifyCertificateFingerprint(sampleDer, flipped), isFalse);
    });

    test('negative: rejects truncated or invalid length fingerprint', () {
      expect(verifyCertificateFingerprint(sampleDer, 'AA:BB:CC'), isFalse);
      expect(verifyCertificateFingerprint(sampleDer, ''), isFalse);
      expect(verifyCertificateFingerprint(sampleDer, 'invalid_hex_string'), isFalse);
    });

    test('negative: rejects when certificate bytes are modified', () {
      final modifiedDer = Uint8List.fromList(sampleDer);
      modifiedDer[0] ^= 0x01;
      expect(verifyCertificateFingerprint(modifiedDer, colonSeparated), isFalse);
    });
  });
}
