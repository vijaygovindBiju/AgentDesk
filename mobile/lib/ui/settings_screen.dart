import 'package:flutter/material.dart';
import '../services/connection_service.dart';
import 'brand_logo.dart';

class SettingsScreen extends StatefulWidget {
  final ConnectionService connection;
  final VoidCallback? onShowIntroduction;

  const SettingsScreen({
    super.key,
    required this.connection,
    this.onShowIntroduction,
  });

  @override
  State<SettingsScreen> createState() => _SettingsScreenState();
}

class _SettingsScreenState extends State<SettingsScreen> {
  late final TextEditingController _urlController;
  late final TextEditingController _tokenController;
  late final TextEditingController _deviceController;
  late final TextEditingController _fingerprintController;

  @override
  void initState() {
    super.initState();
    _urlController = TextEditingController(text: widget.connection.url);
    _tokenController = TextEditingController(text: widget.connection.token);
    _deviceController = TextEditingController(text: widget.connection.deviceId);
    _fingerprintController = TextEditingController(
      text: widget.connection.fingerprint ?? '',
    );
  }

  @override
  void dispose() {
    _urlController.dispose();
    _tokenController.dispose();
    _deviceController.dispose();
    _fingerprintController.dispose();
    super.dispose();
  }

  Future<void> _saveAndConnect() async {
    try {
      await widget.connection.saveConfiguration(
        url: _urlController.text,
        token: _tokenController.text,
        deviceId: _deviceController.text,
        fingerprint: _fingerprintController.text,
      );
      if (!mounted) return;
      widget.connection.connect();
      final message = widget.connection.isConfigurationComplete
          ? 'Configuration saved securely, connecting...'
          : widget.connection.configurationError!;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text(message)));
      if (widget.connection.isConfigurationComplete) {
        Navigator.of(context).pop();
      }
    } catch (_) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
          content: Text('Could not save secure connection configuration.'),
        ),
      );
    }
  }

  void _disconnect() {
    widget.connection.disconnect();
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(const SnackBar(content: Text('Disconnected from daemon.')));
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final isConnected = widget.connection.status == ConnectionStatus.connected;

    return Scaffold(
      appBar: AppBar(title: const Text('Connection Settings')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Card(
            child: Padding(
              padding: EdgeInsets.all(18),
              child: BrandLogo(size: 54, showWordmark: true),
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            key: const Key('url_field'),
            controller: _urlController,
            decoration: const InputDecoration(
              labelText: 'Server WebSocket URL',
              hintText: 'wss://192.168.1.10:8765',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            key: const Key('token_field'),
            controller: _tokenController,
            obscureText: true,
            enableSuggestions: false,
            autocorrect: false,
            decoration: const InputDecoration(
              labelText: 'Authentication Token',
              hintText: 'Stored in this device’s secure storage',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            key: const Key('device_id_field'),
            controller: _deviceController,
            decoration: const InputDecoration(
              labelText: 'Device Identifier',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            key: const Key('fingerprint_field'),
            controller: _fingerprintController,
            decoration: const InputDecoration(
              labelText: 'TLS SHA-256 Fingerprint (Phase 8)',
              hintText:
                  'Required for wss://; optional only for loopback ws:// development',
              border: OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 24),
          if (!widget.connection.isConfigurationComplete) ...[
            Text(
              widget.connection.configurationError!,
              key: const Key('configuration_error'),
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
            const SizedBox(height: 16),
          ],
          Row(
            children: [
              Expanded(
                child: FilledButton.icon(
                  key: const Key('save_connect_button'),
                  onPressed: _saveAndConnect,
                  icon: const Icon(Icons.link),
                  label: const Text('Save & Connect'),
                ),
              ),
              if (isConnected) ...[
                const SizedBox(width: 12),
                OutlinedButton.icon(
                  key: const Key('disconnect_button'),
                  onPressed: _disconnect,
                  icon: const Icon(Icons.link_off),
                  label: const Text('Disconnect'),
                ),
              ],
            ],
          ),
          const SizedBox(height: 28),
          Text('About & Help', style: Theme.of(context).textTheme.titleLarge),
          const SizedBox(height: 8),
          const Text(
            'AgentDesk is a phone-based control and attention surface for coding agents running on your computer.',
          ),
          const SizedBox(height: 8),
          const Text('Version 0.1.0'),
          const SizedBox(height: 12),
          OutlinedButton.icon(
            key: const Key('show_introduction_button'),
            onPressed: widget.onShowIntroduction,
            icon: const Icon(Icons.menu_book_outlined),
            label: const Text('Show Introduction Again'),
          ),
        ],
      ),
    );
  }
}
