import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'debug_screen.dart';
import 'event_screen.dart';
import 'settings_screen.dart';

class HomeScreen extends StatelessWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;

  const HomeScreen({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
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
    {
      'tier': 3,
      'category': Category.working,
      'title': 'Working',
      'empty': 'No active background tasks',
      'color': Colors.blue,
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
            title: const Text('AgentDesk'),
            actions: [
              // Connection status indicator chip
              InkWell(
                onTap: () {
                  metrics.recordTap();
                  Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => SettingsScreen(connection: connection),
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
                key: const Key('debug_button'),
                icon: const Icon(Icons.analytics_outlined),
                tooltip: 'Debug & Metrics',
                onPressed: () {
                  metrics.recordTap();
                  Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => DebugScreen(
                        connection: connection,
                        state: state,
                        metrics: metrics,
                      ),
                    ),
                  );
                },
              ),
              IconButton(
                key: const Key('settings_button'),
                icon: const Icon(Icons.settings_outlined),
                tooltip: 'Settings',
                onPressed: () {
                  metrics.recordTap();
                  Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => SettingsScreen(connection: connection),
                    ),
                  );
                },
              ),
            ],
          ),
          body: ListView(
            padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 12),
            children: [
              for (final def in tierDefs)
                _buildTierSection(
                  context,
                  tier: def['tier'] as int,
                  title: def['title'] as String,
                  emptyCopy: def['empty'] as String,
                  accentColor: def['color'] as MaterialColor,
                ),
            ],
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
}
