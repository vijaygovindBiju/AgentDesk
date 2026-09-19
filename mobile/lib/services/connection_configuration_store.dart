import 'package:shared_preferences/shared_preferences.dart';

/// Non-secret connection settings. The authentication token is intentionally
/// excluded and belongs in [SecureTokenStore].
class ConnectionConfiguration {
  const ConnectionConfiguration({this.url, this.deviceId, this.fingerprint});

  final String? url;
  final String? deviceId;
  final String? fingerprint;
}

/// Persists non-secret connection settings independently of platform APIs.
/// Tests provide an in-memory implementation through this boundary.
abstract interface class ConnectionConfigurationStore {
  Future<ConnectionConfiguration> load();
  Future<void> save(ConnectionConfiguration configuration);
}

class SharedPreferencesConnectionConfigurationStore
    implements ConnectionConfigurationStore {
  static const _urlKey = 'agentdesk.connection.url';
  static const _deviceIdKey = 'agentdesk.connection.device_id';
  static const _fingerprintKey = 'agentdesk.connection.fingerprint';

  Future<SharedPreferences> get _preferences => SharedPreferences.getInstance();

  @override
  Future<ConnectionConfiguration> load() async {
    final preferences = await _preferences;
    return ConnectionConfiguration(
      url: preferences.getString(_urlKey),
      deviceId: preferences.getString(_deviceIdKey),
      fingerprint: preferences.getString(_fingerprintKey),
    );
  }

  @override
  Future<void> save(ConnectionConfiguration configuration) async {
    final preferences = await _preferences;
    await _writeOptional(preferences, _urlKey, configuration.url);
    await _writeOptional(preferences, _deviceIdKey, configuration.deviceId);
    await _writeOptional(
      preferences,
      _fingerprintKey,
      configuration.fingerprint,
    );
  }

  Future<void> _writeOptional(
    SharedPreferences preferences,
    String key,
    String? value,
  ) async {
    if (value == null || value.trim().isEmpty) {
      await preferences.remove(key);
    } else {
      await preferences.setString(key, value);
    }
  }
}
