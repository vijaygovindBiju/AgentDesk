import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'event_screen.dart';
import 'settings_screen.dart';
import 'qr_scan_screen.dart';
import 'brand_logo.dart';

class HomeScreen extends StatelessWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;
  final VoidCallback? onShowIntroduction;

  const HomeScreen({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
    this.onShowIntroduction,
  });

  static const List<Map<String, dynamic>> tierDefs = [
    {
      'tier': 0,
      'category': Category.request,
      'title': 'Requests',
      'empty': 'No pending approval requests',
      'color': Colors.amber,
    },
    {
      'tier': 1,
      'category': Category.error,
      'title': 'Errors',
      'empty': 'No active errors',
      'color': Colors.red,
    },
    {
      'tier': 2,
      'category': Category.completed,
      'title': 'Completed',
      'empty': 'No completed tasks yet',
      'color': Colors.green,
    },
  ];

  Color _categoryColor(Category category) {
    switch (category) {
      case Category.request:
        return Colors.amber.shade800;
      case Category.error:
        return Colors.red.shade700;
      case Category.completed:
        return Colors.green.shade700;
      case Category.working:
        return Colors.blue.shade700;
    }
  }

  void _openEvent(BuildContext context, String eventId) {
    metrics.recordTap();
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => EventScreen(
          eventId: eventId,
          state: state,
          connection: connection,
          metrics: metrics,
        ),
      ),
    );
  }

  void _openQrScanner(BuildContext context) {
    metrics.recordTap();
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => QRScanScreen(connection: connection),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([state, connection]),
      builder: (context, _) {
        final entries = state.entries;
        if (entries.isNotEmpty) {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            metrics.recordSummaryRendered(entries.length);
          });
        }

        final status = connection.status;
        final isConnected = status == ConnectionStatus.connected;

        return Scaffold(
          appBar: AppBar(
            title: const BrandLogo(size: 34, showWordmark: true),
            actions: [
              IconButton(
                key: const Key('home_scan_qr_button'),
                icon: const Icon(Icons.qr_code_scanner),
                tooltip: 'Scan QR Code to Pair',
                onPressed: () => _openQrScanner(context),
              ),
              // Connection status indicator chip
              InkWell(
                onTap: () {
                  metrics.recordTap();
                  Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => SettingsScreen(
                        connection: connection,
                        onShowIntroduction: onShowIntroduction,
                      ),
                    ),
                  );
                },
                borderRadius: BorderRadius.circular(16),
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 8.0,
                    vertical: 4.0,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Container(
                        key: const Key('status_indicator_dot'),
                        width: 10,
                        height: 10,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: isConnected
                              ? Colors.greenAccent.shade700
                              : status == ConnectionStatus.configurationRequired
                              ? Colors.orange
                              : status == ConnectionStatus.reconnecting
                              ? Colors.orange
                              : Colors.red,
                        ),
                      ),
                      const SizedBox(width: 6),
                      Text(status.name, style: const TextStyle(fontSize: 12)),
                    ],
                  ),
                ),
              ),
              IconButton(
                key: const Key('settings_button'),
                icon: const Icon(Icons.settings_outlined),
                tooltip: 'Settings',
                onPressed: () {
                  metrics.recordTap();
                  Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => SettingsScreen(
                        connection: connection,
                        onShowIntroduction: onShowIntroduction,
                      ),
                    ),
                  );
                },
              ),
            ],
          ),
          body: ListView(
            padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
            children: [
              if (!isConnected)
                Padding(
                  padding: const EdgeInsets.only(bottom: 12.0),
                  child: Card(
                    elevation: 3,
                    color: Theme.of(context).colorScheme.primaryContainer,
                    child: Padding(
                      padding: const EdgeInsets.all(16.0),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Row(
                            children: [
                              Container(
                                padding: const EdgeInsets.all(10),
                                decoration: BoxDecoration(
                                  color: Theme.of(context).colorScheme.primary.withValues(alpha: 0.15),
                                  borderRadius: BorderRadius.circular(12),
                                ),
                                child: Icon(
                                  Icons.qr_code_scanner,
                                  size: 32,
                                  color: Theme.of(context).colorScheme.primary,
                                ),
                              ),
                              const SizedBox(width: 14),
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      'Connect to Laptop',
                                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                                            fontWeight: FontWeight.bold,
                                          ),
                                    ),
                                    const SizedBox(height: 4),
                                    Text(
                                      'Run "agentdesk pair" in your laptop terminal, then scan the QR code to pair.',
                                      style: Theme.of(context).textTheme.bodySmall,
                                    ),
                                  ],
                                ),
                              ),
                            ],
                          ),
                          const SizedBox(height: 14),
                          FilledButton.icon(
                            key: const Key('home_banner_scan_button'),
                            onPressed: () => _openQrScanner(context),
                            icon: const Icon(Icons.qr_code_scanner),
                            label: const Text('Scan QR Code to Pair'),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              for (final def in tierDefs)
                _buildTierSection(
                  context,
                  tier: def['tier'] as int,
                  title: def['title'] as String,
                  emptyCopy: def['empty'] as String,
                  accentColor: def['color'] as MaterialColor,
                ),
              // Agent status indicator (replaces the old Working card list)
              _buildAgentStatusBar(context),
            ],
          ),
          floatingActionButton: isConnected
              ? null
              : FloatingActionButton.extended(
                  key: const Key('home_fab_scan_button'),
                  onPressed: () => _openQrScanner(context),
                  icon: const Icon(Icons.qr_code_scanner),
                  label: const Text('Scan QR Code'),
                ),
        );
      },
    );
  }

  Widget _buildTierSection(
    BuildContext context, {
    required int tier,
    required String title,
    required String emptyCopy,
    required MaterialColor accentColor,
  }) {
    final tierEntries = state.entriesForTier(tier);

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(4, 16, 4, 8),
          child: Row(
            children: [
              Text(
                title,
                style: Theme.of(
                  context,
                ).textTheme.titleMedium?.copyWith(fontWeight: FontWeight.bold),
              ),
              const SizedBox(width: 8),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                decoration: BoxDecoration(
                  color: accentColor.shade100,
                  borderRadius: BorderRadius.circular(12),
                ),
                child: Text(
                  '${tierEntries.length}',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.bold,
                    color: accentColor.shade900,
                  ),
                ),
              ),
            ],
          ),
        ),
        if (tierEntries.isEmpty)
          Container(
            padding: const EdgeInsets.all(16),
            margin: const EdgeInsets.only(bottom: 8),
            decoration: BoxDecoration(
              color: Colors.grey.shade100,
              borderRadius: BorderRadius.circular(8),
            ),
            width: double.infinity,
            child: Text(
              emptyCopy,
              style: TextStyle(
                color: Colors.grey.shade600,
                fontStyle: FontStyle.italic,
                fontSize: 13,
              ),
            ),
          )
        else
          for (final entry in tierEntries) _buildEntryCard(context, entry),
      ],
    );
  }

  Widget _buildEntryCard(BuildContext context, QueueEntry entry) {
    final event = state.eventFor(entry.eventId);
    final isStillBlocking =
        entry.tier == 0 &&
        entry.state == EntryState.dismissed &&
        entry.resolution == Resolution.unresolved;
    final isEscalated = entry.escalationLevel >= 1;

    final color = event != null
        ? _categoryColor(event.category)
        : Colors.indigo.shade700;

    return Card(
      key: Key('entry_card_${entry.eventId}'),
      margin: const EdgeInsets.only(bottom: 8),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(10),
        side: BorderSide(
          color: isStillBlocking
              ? Colors.amber.shade900
              : isEscalated
              ? Colors.red.shade400
              : Colors.grey.shade200,
          width: (isStillBlocking || isEscalated) ? 2.0 : 1.0,
        ),
      ),
      child: InkWell(
        onTap: () => _openEvent(context, entry.eventId),
        borderRadius: BorderRadius.circular(10),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Container(
                    width: 8,
                    height: 8,
                    decoration: BoxDecoration(
                      color: color,
                      shape: BoxShape.circle,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Text(
                    event != null
                        ? '${event.agentName} • ${event.project}'
                        : 'Unknown Agent',
                    style: TextStyle(
                      fontSize: 12,
                      color: Colors.grey.shade700,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const Spacer(),
                  // Badges
                  if (isStillBlocking)
                    Container(
                      key: Key('still_blocking_badge_${entry.eventId}'),
                      margin: const EdgeInsets.only(right: 6),
                      padding: const EdgeInsets.symmetric(
                        horizontal: 6,
                        vertical: 2,
                      ),
                      decoration: BoxDecoration(
                        color: Colors.amber.shade100,
                        borderRadius: BorderRadius.circular(4),
                        border: Border.all(color: Colors.amber.shade800),
                      ),
                      child: Text(
                        'Still blocking agent',
                        style: TextStyle(
                          fontSize: 10,
                          fontWeight: FontWeight.bold,
                          color: Colors.amber.shade900,
                        ),
                      ),
                    ),
                  if (isEscalated)
                    Container(
                      key: Key('escalation_badge_${entry.eventId}'),
                      margin: const EdgeInsets.only(right: 6),
                      padding: const EdgeInsets.symmetric(
                        horizontal: 6,
                        vertical: 2,
                      ),
                      decoration: BoxDecoration(
                        color: Colors.red.shade100,
                        borderRadius: BorderRadius.circular(4),
                        border: Border.all(color: Colors.red.shade800),
                      ),
                      child: Text(
                        'Unusually long (L${entry.escalationLevel})',
                        style: TextStyle(
                          fontSize: 10,
                          fontWeight: FontWeight.bold,
                          color: Colors.red.shade900,
                        ),
                      ),
                    ),
                  Text(
                    'Score: ${entry.score}',
                    style: TextStyle(fontSize: 11, color: Colors.grey.shade600),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              Text(
                event?.summary ?? 'No summary available',
                style: const TextStyle(
                  fontWeight: FontWeight.bold,
                  fontSize: 15,
                ),
              ),
              const SizedBox(height: 4),
              Text(
                event?.message ?? '',
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 13, color: Colors.grey.shade800),
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// Compact agent status indicator — replaces the old Working card list.
  /// Shows a single bar: "⚡ Agent Working (3 tasks)" or "✅ Agent Idle".
  Widget _buildAgentStatusBar(BuildContext context) {
    final activeCount = state.activeWorkingCount;
    final isWorking = activeCount > 0;

    return Padding(
      padding: const EdgeInsets.fromLTRB(4, 16, 4, 8),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        decoration: BoxDecoration(
          color: isWorking
              ? Colors.blue.shade50
              : Colors.green.shade50,
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: isWorking
                ? Colors.blue.shade200
                : Colors.green.shade200,
          ),
        ),
        child: Row(
          children: [
            // Status icon
            Container(
              width: 10,
              height: 10,
              decoration: BoxDecoration(
                color: isWorking
                    ? Colors.blue.shade600
                    : Colors.green.shade600,
                shape: BoxShape.circle,
                boxShadow: isWorking
                    ? [
                        BoxShadow(
                          color: Colors.blue.shade300,
                          blurRadius: 6,
                          spreadRadius: 1,
                        ),
                      ]
                    : null,
              ),
            ),
            const SizedBox(width: 12),
            // Status text
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    isWorking ? 'Agent Working' : 'Agent Idle',
                    style: TextStyle(
                      fontWeight: FontWeight.bold,
                      fontSize: 14,
                      color: isWorking
                          ? Colors.blue.shade900
                          : Colors.green.shade900,
                    ),
                  ),
                  if (isWorking)
                    Text(
                      '$activeCount active task${activeCount == 1 ? '' : 's'}',
                      style: TextStyle(
                        fontSize: 12,
                        color: Colors.blue.shade700,
                      ),
                    )
                  else
                    Text(
                      'Ready for input',
                      style: TextStyle(
                        fontSize: 12,
                        color: Colors.green.shade700,
                      ),
                    ),
                ],
              ),
            ),
            // Icon
            Icon(
              isWorking ? Icons.bolt : Icons.check_circle_outline,
              color: isWorking
                  ? Colors.blue.shade600
                  : Colors.green.shade600,
              size: 24,
            ),
          ],
        ),
      ),
    );
  }
}
