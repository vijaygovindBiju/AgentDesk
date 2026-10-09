import 'package:flutter/material.dart';
import '../services/connection_service.dart';
import 'agent_control_overlay.dart';
import 'app_theme.dart';
import 'brand_logo.dart';
import 'qr_scan_screen.dart';

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
        backgroundColor: Theme.of(ctx).brightness == Brightness.dark
            ? AppTheme.surfaceCardElevated
            : Colors.white,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
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
              backgroundColor: AppTheme.errorRed,
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
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('Disconnected from relay.')),
    );
    setState(() {});
  }

  Widget _buildSectionHeader(String title) {
    return Padding(
      padding: const EdgeInsets.only(left: 4, bottom: 8, top: 12),
      child: Text(
        title.toUpperCase(),
        style: const TextStyle(
          fontSize: 11,
          fontWeight: FontWeight.w700,
          letterSpacing: 1.1,
          color: AppTheme.textTertiary,
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final isConnected = widget.connection.status == ConnectionStatus.connected;
    final isPaired = widget.connection.isPaired;
    final isDark = Theme.of(context).brightness == Brightness.dark;

    final cardBg = isDark ? AppTheme.surfaceCardElevated : Colors.white;
    final borderColor = isDark ? AppTheme.cardBorderSubtle : Colors.grey.shade200;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Settings & Relay'),
      ),
      body: ListView(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        children: [
          // Brand Header Card
          Container(
            padding: const EdgeInsets.all(16),
            decoration: BoxDecoration(
              color: cardBg,
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: borderColor),
            ),
            child: Row(
              children: [
                const BrandLogo(size: 42, showWordmark: true),
                const Spacer(),
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
                  decoration: BoxDecoration(
                    color: (isConnected ? AppTheme.primaryEmerald : AppTheme.errorRed)
                        .withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(12),
                    border: Border.all(
                      color: (isConnected ? AppTheme.primaryEmerald : AppTheme.errorRed)
                          .withValues(alpha: 0.3),
                    ),
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        isConnected ? Icons.check_circle : Icons.circle_outlined,
                        size: 12,
                        color: isConnected ? AppTheme.primaryEmerald : AppTheme.errorRed,
                      ),
                      const SizedBox(width: 6),
                      Text(
                        isConnected ? 'Relay Active' : 'Relay Standby',
                        style: TextStyle(
                          fontSize: 11,
                          fontWeight: FontWeight.bold,
                          color: isConnected ? AppTheme.primaryEmerald : AppTheme.errorRed,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),

          _buildSectionHeader('Laptop Pairing'),

          // Pairing Section
          if (isPaired)
            Container(
              padding: const EdgeInsets.all(18),
              decoration: BoxDecoration(
                color: isDark
                    ? AppTheme.surfaceCardElevated
                    : AppTheme.primaryEmerald.withValues(alpha: 0.05),
                borderRadius: BorderRadius.circular(16),
                border: Border.all(
                  color: AppTheme.primaryEmerald.withValues(alpha: isDark ? 0.35 : 0.25),
                ),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Container(
                        padding: const EdgeInsets.all(8),
                        decoration: BoxDecoration(
                          color: AppTheme.primaryEmerald.withValues(alpha: 0.15),
                          shape: BoxShape.circle,
                        ),
                        child: const Icon(
                          Icons.laptop_mac,
                          color: AppTheme.primaryEmerald,
                          size: 22,
                        ),
                      ),
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
                              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                    color: AppTheme.textSecondary,
                                  ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  if (widget.connection.relayUrl != null &&
                      widget.connection.relayUrl!.isNotEmpty) ...[
                    Container(
                      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                      decoration: BoxDecoration(
                        color: isDark ? Colors.black26 : Colors.grey.shade100,
                        borderRadius: BorderRadius.circular(8),
                      ),
                      child: Row(
                        children: [
                          const Icon(Icons.cloud_outlined, size: 14, color: AppTheme.primaryCyan),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Text(
                              'Relay: ${widget.connection.relayUrl}',
                              style: const TextStyle(
                                fontSize: 12,
                                fontFamily: 'monospace',
                                color: AppTheme.textSecondary,
                              ),
                              overflow: TextOverflow.ellipsis,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                  const SizedBox(height: 16),
                  Row(
                    children: [
                      Expanded(
                        child: OutlinedButton.icon(
                          key: const Key('scan_qr_button'),
                          style: OutlinedButton.styleFrom(
                            side: BorderSide(
                              color: AppTheme.primaryCyan.withValues(alpha: 0.5),
                            ),
                          ),
                          onPressed: _openQrScanner,
                          icon: const Icon(Icons.qr_code_scanner, size: 18),
                          label: const Text('Re-Pair (QR)'),
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: OutlinedButton.icon(
                          key: const Key('unpair_button'),
                          style: OutlinedButton.styleFrom(
                            foregroundColor: AppTheme.errorRed,
                            side: BorderSide(
                              color: AppTheme.errorRed.withValues(alpha: 0.5),
                            ),
                          ),
                          onPressed: _confirmUnpair,
                          icon: const Icon(Icons.link_off, size: 18),
                          label: const Text('Unpair'),
                        ),
                      ),
                    ],
                  ),
                ],
              ),
            )
          else
            Container(
              padding: const EdgeInsets.all(20),
              decoration: BoxDecoration(
                color: cardBg,
                borderRadius: BorderRadius.circular(16),
                border: Border.all(color: borderColor),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Container(
                        padding: const EdgeInsets.all(10),
                        decoration: BoxDecoration(
                          color: AppTheme.primaryEmerald.withValues(alpha: 0.15),
                          shape: BoxShape.circle,
                        ),
                        child: const Icon(
                          Icons.qr_code_scanner,
                          size: 26,
                          color: AppTheme.primaryEmerald,
                        ),
                      ),
                      const SizedBox(width: 14),
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
                            const SizedBox(height: 2),
                            const Text(
                              'Scan the QR code shown by "agentdesk pair" or F4 on your laptop.',
                              style: TextStyle(
                                fontSize: 12,
                                color: AppTheme.textSecondary,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 18),
                  FilledButton.icon(
                    key: const Key('scan_qr_button'),
                    style: FilledButton.styleFrom(
                      backgroundColor: AppTheme.primaryEmerald,
                      foregroundColor: Colors.black,
                      padding: const EdgeInsets.symmetric(vertical: 14),
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(12),
                      ),
                    ),
                    onPressed: _openQrScanner,
                    icon: const Icon(Icons.camera_alt, size: 18),
                    label: const Text(
                      'Scan QR Code to Pair',
                      style: TextStyle(fontWeight: FontWeight.bold),
                    ),
                  ),
                ],
              ),
            ),

          _buildSectionHeader('Relay & Transport'),

          // Relay connection status card
          Container(
            key: const Key('relay_status_card'),
            padding: const EdgeInsets.all(18),
            decoration: BoxDecoration(
              color: cardBg,
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: borderColor),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Icon(
                      isConnected ? Icons.cloud_done : Icons.cloud_off,
                      size: 20,
                      color: isConnected ? AppTheme.primaryEmerald : AppTheme.errorRed,
                    ),
                    const SizedBox(width: 10),
                    Text(
                      isConnected ? 'Relay Connected' : 'Relay Disconnected',
                      key: const Key('relay_status_text'),
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                            fontWeight: FontWeight.bold,
                          ),
                    ),
                  ],
                ),
                const SizedBox(height: 10),
                const Text(
                  'Your phone and laptop each connect outbound to the AgentDesk relay over secure WSS. '
                  'They do not require peer-to-peer or shared Wi-Fi.',
                  style: TextStyle(
                    fontSize: 12,
                    color: AppTheme.textSecondary,
                    height: 1.4,
                  ),
                ),
                if (widget.connection.lastError != null && !isConnected) ...[
                  const SizedBox(height: 10),
                  Container(
                    padding: const EdgeInsets.all(10),
                    decoration: BoxDecoration(
                      color: AppTheme.errorRed.withValues(alpha: 0.1),
                      borderRadius: BorderRadius.circular(8),
                      border: Border.all(
                        color: AppTheme.errorRed.withValues(alpha: 0.3),
                      ),
                    ),
                    child: Text(
                      widget.connection.lastError!,
                      key: const Key('configuration_error'),
                      style: const TextStyle(
                        color: AppTheme.errorRed,
                        fontSize: 12,
                      ),
                    ),
                  ),
                ],
                if (isPaired) ...[
                  const SizedBox(height: 16),
                  isConnected
                      ? OutlinedButton.icon(
                          key: const Key('disconnect_button'),
                          style: OutlinedButton.styleFrom(
                            foregroundColor: AppTheme.textSecondary,
                          ),
                          onPressed: _disconnect,
                          icon: const Icon(Icons.link_off, size: 18),
                          label: const Text('Disconnect from Relay'),
                        )
                      : FilledButton.icon(
                          key: const Key('reconnect_button'),
                          style: FilledButton.styleFrom(
                            backgroundColor: AppTheme.primaryEmerald,
                            foregroundColor: Colors.black,
                          ),
                          onPressed: () {
                            widget.connection.connect();
                            setState(() {});
                          },
                          icon: const Icon(Icons.refresh, size: 18),
                          label: const Text('Reconnect to Relay'),
                        ),
                ],
              ],
            ),
          ),

          _buildSectionHeader('Agent Controls & Quotas'),

          // Agent Controls Card
          Container(
            padding: const EdgeInsets.all(16),
            decoration: BoxDecoration(
              color: cardBg,
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: borderColor),
            ),
            child: Row(
              children: [
                Container(
                  padding: const EdgeInsets.all(8),
                  decoration: BoxDecoration(
                    color: AppTheme.primaryCyan.withValues(alpha: 0.15),
                    shape: BoxShape.circle,
                  ),
                  child: const Icon(Icons.tune, color: AppTheme.primaryCyan, size: 20),
                ),
                const SizedBox(width: 14),
                const Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Model & Quota Controls',
                        style: TextStyle(fontWeight: FontWeight.bold, fontSize: 14),
                      ),
                      SizedBox(height: 2),
                      Text(
                        'Configure reasoning effort, model, and tool execution quotas.',
                        style: TextStyle(fontSize: 12, color: AppTheme.textSecondary),
                      ),
                    ],
                  ),
                ),
                OutlinedButton(
                  onPressed: () => AgentControlOverlay.show(context, connection: widget.connection),
                  style: OutlinedButton.styleFrom(
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  ),
                  child: const Text('Configure'),
                ),
              ],
            ),
          ),

          _buildSectionHeader('About & Help'),

          // About Card
          Container(
            padding: const EdgeInsets.all(18),
            decoration: BoxDecoration(
              color: cardBg,
              borderRadius: BorderRadius.circular(16),
              border: Border.all(color: borderColor),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Text(
                  'AgentDesk Mobile',
                  style: TextStyle(fontWeight: FontWeight.bold, fontSize: 15),
                ),
                const SizedBox(height: 4),
                const Text(
                  'Phone-based supervision, approval surface, and live terminal monitor for autonomous coding agents.',
                  style: TextStyle(fontSize: 12, color: AppTheme.textSecondary, height: 1.4),
                ),
                const SizedBox(height: 8),
                const Text(
                  'Version 0.1.0 • Relay-First Architecture',
                  style: TextStyle(fontSize: 11, color: AppTheme.textTertiary),
                ),
                const SizedBox(height: 14),
                OutlinedButton.icon(
                  key: const Key('show_introduction_button'),
                  onPressed: widget.onShowIntroduction,
                  icon: const Icon(Icons.menu_book_outlined, size: 18),
                  label: const Text('Show Introduction Walkthrough'),
                ),
              ],
            ),
          ),
          const SizedBox(height: 24),
        ],
      ),
    );
  }
}
