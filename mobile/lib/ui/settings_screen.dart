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
  Future<void> _openQrScanner() async {
    final paired = await Navigator.of(context).push<bool>(
      MaterialPageRoute(
        builder: (_) => QRScanScreen(connection: widget.connection),
      ),
    );
    if (paired == true && mounted) {
      setState(() {});
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
      setState(() {});
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Phone unpaired successfully.')),
        );
      }
    }
  }

  void _disconnect() {
    widget.connection.disconnect();
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(const SnackBar(content: Text('Disconnected from relay.')));
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
                    if (widget.connection.relayUrl != null &&
                        widget.connection.relayUrl!.isNotEmpty)
                      Text(
                        'Relay: ${widget.connection.relayUrl}',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                    if (widget.connection.status == ConnectionStatus.connected &&
                        widget.connection.activeTransport != null) ...[
                      const SizedBox(height: 8),
                      Row(
                        children: [
                          Icon(
                            Icons.cloud_done,
                            size: 16,
                            color: Colors.green.shade700,
                          ),
                          const SizedBox(width: 6),
                          Text(
                            'Connected via Relay',
                            style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                  color: Colors.green.shade700,
                                  fontWeight: FontWeight.bold,
                                ),
                          ),
                        ],
                      ),
                    ],
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

          // Relay connection status (relay is the only transport)
          Card(
            key: const Key('relay_status_card'),
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Icon(
                        isConnected ? Icons.circle : Icons.circle_outlined,
                        size: 14,
                        color: isConnected ? Colors.green.shade600 : Theme.of(context).colorScheme.error,
                      ),
                      const SizedBox(width: 8),
                      Text(
                        isConnected ? 'Relay Connected' : 'Relay Disconnected',
                        key: const Key('relay_status_text'),
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  const Text(
                    'Your phone and laptop each connect outbound to the AgentDesk relay over '
                    'WSS. They do not need to share a Wi-Fi network: the laptop can be on '
                    'hostel/college Wi-Fi while the phone is on mobile data.',
                    style: TextStyle(fontSize: 13),
                  ),
                  if (widget.connection.lastError != null &&
                      !isConnected) ...[
                    const SizedBox(height: 8),
                    Text(
                      widget.connection.lastError!,
                      key: const Key('configuration_error'),
                      style: TextStyle(color: Theme.of(context).colorScheme.error),
                    ),
                  ],
                  if (isPaired) ...[
                    const SizedBox(height: 12),
                    isConnected
                        ? OutlinedButton.icon(
                            key: const Key('disconnect_button'),
                            onPressed: _disconnect,
                            icon: const Icon(Icons.link_off),
                            label: const Text('Disconnect'),
                          )
                        : FilledButton.icon(
                            key: const Key('reconnect_button'),
                            onPressed: () {
                              widget.connection.connect();
                              setState(() {});
                            },
                            icon: const Icon(Icons.refresh),
                            label: const Text('Reconnect to Relay'),
                          ),
                  ],
                ],
              ),
            ),
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
