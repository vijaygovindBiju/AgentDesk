import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import '../services/connection_service.dart';
import 'brand_logo.dart';
import 'qr_scan_screen.dart';
import 'structured_request_fixtures_screen.dart';

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

  Future<void> _openQrScanner() async {
    final paired = await Navigator.of(context).push<bool>(
      MaterialPageRoute(
        builder: (_) => QRScanScreen(connection: widget.connection),
      ),
    );
    if (paired == true && mounted) {
      setState(() {
        _urlController.text = widget.connection.url;
        _tokenController.text = widget.connection.token;
        _deviceController.text = widget.connection.deviceId;
        _fingerprintController.text = widget.connection.fingerprint ?? '';
      });
    }
  }

  Future<void> _confirmUnpair() async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Unpair Phone?'),
        content: const Text(
          'This will remove saved pairing credentials from this phone. '
          'You will need to scan a new QR code from your laptop to reconnect.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(ctx).pop(false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            key: const Key('confirm_unpair_button'),
            style: FilledButton.styleFrom(
              backgroundColor: Theme.of(context).colorScheme.error,
            ),
            onPressed: () => Navigator.of(ctx).pop(true),
            child: const Text('Unpair'),
          ),
        ],
      ),
    );

    if (confirmed == true && mounted) {
      await widget.connection.unpair();
      setState(() {
        _urlController.text = widget.connection.url;
        _tokenController.text = '';
        _fingerprintController.text = '';
      });
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Phone unpaired successfully.')),
        );
      }
    }
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
    final isPaired = widget.connection.isPaired;

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

          // Primary Pairing Section
          if (isPaired)
            Card(
              color: Theme.of(context).colorScheme.primaryContainer.withValues(alpha: 0.25),
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Icon(Icons.check_circle, color: Colors.green.shade600, size: 28),
                        const SizedBox(width: 12),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                'Paired with Laptop',
                                style: Theme.of(context).textTheme.titleMedium?.copyWith(
                                  fontWeight: FontWeight.bold,
                                ),
                              ),
                              Text(
                                'Instance: ${widget.connection.instanceId}',
                                style: Theme.of(context).textTheme.bodySmall,
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 12),
                    if (widget.connection.lanAddress != null)
                      Text(
                        'LAN Address: ${widget.connection.lanAddress}',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    const SizedBox(height: 16),
                    Row(
                      children: [
                        Expanded(
                          child: OutlinedButton.icon(
                            key: const Key('scan_qr_button'),
                            onPressed: _openQrScanner,
                            icon: const Icon(Icons.qr_code_scanner),
                            label: const Text('Re-Pair (QR)'),
                          ),
                        ),
                        const SizedBox(width: 12),
                        Expanded(
                          child: OutlinedButton.icon(
                            key: const Key('unpair_button'),
                            style: OutlinedButton.styleFrom(
                              foregroundColor: Theme.of(context).colorScheme.error,
                            ),
                            onPressed: _confirmUnpair,
                            icon: const Icon(Icons.link_off),
                            label: const Text('Unpair'),
                          ),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            )
          else
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Row(
                      children: [
                        Icon(
                          Icons.qr_code_scanner,
                          size: 36,
                          color: Theme.of(context).colorScheme.primary,
                        ),
                        const SizedBox(width: 16),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                'Pair with Laptop',
                                style: Theme.of(context).textTheme.titleMedium?.copyWith(
                                  fontWeight: FontWeight.bold,
                                ),
                              ),
                              const SizedBox(height: 4),
                              const Text(
                                'Scan the QR code shown by "agentdesk pair" on your computer.',
                                style: TextStyle(fontSize: 13),
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 16),
                    FilledButton.icon(
                      key: const Key('scan_qr_button'),
                      onPressed: _openQrScanner,
                      icon: const Icon(Icons.camera_alt),
                      label: const Text('Scan QR Code to Pair'),
                    ),
                  ],
                ),
              ),
            ),

          const SizedBox(height: 16),

          // Collapsible / Expandable Manual Configuration
          ExpansionTile(
            key: const Key('manual_config_expansion_tile'),
            initiallyExpanded: true,
            title: const Text('Manual / Developer Configuration'),
            subtitle: const Text('IP address, custom token, and TLS fingerprint'),
            children: [
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 8),
                child: Column(
                  children: [
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
                        labelText: 'TLS SHA-256 Fingerprint',
                        hintText: 'Required for wss://; optional only for loopback ws://',
                        border: OutlineInputBorder(),
                      ),
                    ),
                    const SizedBox(height: 16),
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
                  ],
                ),
              ),
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
          if (kDebugMode) ...[
            const SizedBox(height: 12),
            OutlinedButton.icon(
              key: const Key('structured_fixtures_button'),
              onPressed: () {
                Navigator.of(context).push(
                  MaterialPageRoute(
                    builder: (_) => StructuredRequestFixtureScreen(
                      state: widget.connection.state,
                    ),
                  ),
                );
              },
              icon: const Icon(Icons.science_outlined),
              label: const Text('Structured UI Fixtures'),
            ),
          ],
        ],
      ),
    );
  }
}
