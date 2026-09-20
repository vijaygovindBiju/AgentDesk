import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// Isolates authentication-token persistence from the application.
/// Implementations must use OS-backed secure storage; ordinary preferences
/// and files are never valid token stores.
abstract interface class SecureTokenStore {
  Future<String?> readToken();
  Future<void> writeToken(String token);
  Future<void> deleteToken();
}

class FlutterSecureTokenStore implements SecureTokenStore {
  static const _tokenKey = 'agentdesk.authentication_token';
  static const _androidOptions = AndroidOptions(
    migrateOnAlgorithmChange: true,
    migrateWithBackup: true,
    resetOnError: false,
  );

  const FlutterSecureTokenStore({FlutterSecureStorage? storage})
    : _storage =
          storage ?? const FlutterSecureStorage(aOptions: _androidOptions);

  final FlutterSecureStorage _storage;

  @override
  Future<String?> readToken() => _storage.read(key: _tokenKey);

  @override
  Future<void> writeToken(String token) =>
      _storage.write(key: _tokenKey, value: token);

  @override
  Future<void> deleteToken() => _storage.delete(key: _tokenKey);
}
