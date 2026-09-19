import 'package:agentdesk/services/connection_configuration_store.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/services/secure_token_store.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:flutter_test/flutter_test.dart';

class MemoryConfigurationStore implements ConnectionConfigurationStore {
  ConnectionConfiguration value = const ConnectionConfiguration();
  int saves = 0;

  @override
  Future<ConnectionConfiguration> load() async => value;

  @override
  Future<void> save(ConnectionConfiguration configuration) async {
    saves += 1;
    value = configuration;
  }
}

class MemorySecureTokenStore implements SecureTokenStore {
  String? value;
  int reads = 0;
  int writes = 0;
  int deletes = 0;

  @override
  Future<void> deleteToken() async {
    deletes += 1;
    value = null;
  }

  @override
  Future<String?> readToken() async {
    reads += 1;
    return value;
  }

  @override
  Future<void> writeToken(String token) async {
    writes += 1;
    value = token;
  }
}

ConnectionService service(
  MemoryConfigurationStore configuration,
  MemorySecureTokenStore token,
) => ConnectionService(
  state: AgentDeskState(),
  configurationStore: configuration,
  tokenStore: token,
);

void main() {
  group('persistent secure mobile configuration', () {
    test(
      'saves non-secret settings and stores token through secure boundary',
      () async {
        final configuration = MemoryConfigurationStore();
        final tokens = MemorySecureTokenStore();
        final connection = service(configuration, tokens);

        await connection.saveConfiguration(
          url: 'wss://agentdesk.local:8765',
          token: 'sensitive-token',
          deviceId: 'phone-1',
          fingerprint: 'AA:BB:CC',
        );

        expect(configuration.value.url, 'wss://agentdesk.local:8765');
        expect(configuration.value.deviceId, 'phone-1');
        expect(configuration.value.fingerprint, 'AA:BB:CC');
        expect(tokens.value, 'sensitive-token');
        expect(tokens.writes, 1);
        expect(connection.isConfigurationComplete, isTrue);
      },
    );

    test(
      'startup restoration loads preferences and secure token before connection',
      () async {
        final configuration = MemoryConfigurationStore()
          ..value = const ConnectionConfiguration(
            url: 'wss://agentdesk.local:8765',
            deviceId: 'phone-1',
            fingerprint: 'AA:BB:CC',
          );
        final tokens = MemorySecureTokenStore()..value = 'restored-token';
        final connection = service(configuration, tokens);

        await connection.restoreConfiguration();

        expect(tokens.reads, 1);
        expect(connection.url, 'wss://agentdesk.local:8765');
        expect(connection.deviceId, 'phone-1');
        expect(connection.fingerprint, 'AA:BB:CC');
        expect(connection.token, 'restored-token');
        expect(connection.status, ConnectionStatus.disconnected);
        expect(connection.isConfigurationComplete, isTrue);
      },
    );

    test(
      'missing token produces configuration-required state without connecting',
      () async {
        final configuration = MemoryConfigurationStore()
          ..value = const ConnectionConfiguration(
            url: 'wss://agentdesk.local:8765',
            deviceId: 'phone-1',
            fingerprint: 'AA:BB:CC',
          );
        final connection = service(configuration, MemorySecureTokenStore());

        await connection.restoreConfiguration();
        connection.connect();

        expect(connection.status, ConnectionStatus.configurationRequired);
        expect(
          connection.configurationError,
          'Authentication token is required.',
        );
      },
    );

    test('missing URL is reported clearly', () async {
      final configuration = MemoryConfigurationStore();
      final tokens = MemorySecureTokenStore();
      final connection = service(configuration, tokens);

      await connection.saveConfiguration(
        url: '',
        token: 'token',
        deviceId: 'phone-1',
        fingerprint: 'AA:BB:CC',
      );

      expect(connection.status, ConnectionStatus.configurationRequired);
      expect(
        connection.configurationError,
        'Server WebSocket URL is required.',
      );
    });

    test('wss requires a pinned fingerprint', () async {
      final connection = service(
        MemoryConfigurationStore(),
        MemorySecureTokenStore(),
      );

      await connection.saveConfiguration(
        url: 'wss://agentdesk.local:8765',
        token: 'token',
        deviceId: 'phone-1',
        fingerprint: '',
      );

      expect(connection.status, ConnectionStatus.configurationRequired);
      expect(connection.configurationError, contains('pinned SHA-256'));
    });

    test(
      'complete wss configuration is accepted without transport downgrade',
      () async {
        final connection = service(
          MemoryConfigurationStore(),
          MemorySecureTokenStore(),
        );

        await connection.saveConfiguration(
          url: 'wss://agentdesk.local:8765',
          token: 'token',
          deviceId: 'phone-1',
          fingerprint: 'AA:BB:CC',
        );

        expect(connection.url, startsWith('wss://'));
        expect(connection.isConfigurationComplete, isTrue);
        expect(connection.status, ConnectionStatus.disconnected);
      },
    );

    test(
      'plain ws rejects non-loopback addresses and permits loopback development',
      () async {
        final connection = service(
          MemoryConfigurationStore(),
          MemorySecureTokenStore(),
        );

        await connection.saveConfiguration(
          url: 'ws://192.168.1.20:8765',
          token: 'token',
          deviceId: 'phone-1',
          fingerprint: '',
        );
        expect(connection.status, ConnectionStatus.configurationRequired);
        expect(connection.configurationError, contains('only for loopback'));

        await connection.saveConfiguration(
          url: 'ws://127.0.0.1:8765',
          token: 'token',
          deviceId: 'phone-1',
          fingerprint: '',
        );
        expect(connection.isConfigurationComplete, isTrue);
      },
    );

    test(
      'configuration updates replace persisted settings and secure token',
      () async {
        final configuration = MemoryConfigurationStore();
        final tokens = MemorySecureTokenStore();
        final connection = service(configuration, tokens);

        await connection.saveConfiguration(
          url: 'wss://first.local:8765',
          token: 'first-token',
          deviceId: 'phone-1',
          fingerprint: 'AA',
        );
        await connection.saveConfiguration(
          url: 'wss://second.local:8765',
          token: 'second-token',
          deviceId: 'phone-2',
          fingerprint: 'BB',
        );

        expect(configuration.saves, 2);
        expect(configuration.value.url, 'wss://second.local:8765');
        expect(configuration.value.deviceId, 'phone-2');
        expect(configuration.value.fingerprint, 'BB');
        expect(tokens.value, 'second-token');
        expect(tokens.writes, 2);
      },
    );

    test(
      'clearing token deletes it through secure-storage abstraction',
      () async {
        final tokens = MemorySecureTokenStore()..value = 'old-token';
        final connection = service(MemoryConfigurationStore(), tokens);

        await connection.saveConfiguration(
          url: 'wss://agentdesk.local:8765',
          token: '',
          deviceId: 'phone-1',
          fingerprint: 'AA',
        );

        expect(tokens.deletes, 1);
        expect(tokens.value, isNull);
        expect(connection.status, ConnectionStatus.configurationRequired);
      },
    );
  });
}
