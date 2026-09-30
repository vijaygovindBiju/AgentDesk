import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'event_screen.dart';

class RequestsScreen extends StatelessWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;

  const RequestsScreen({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
  });

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([state, connection]),
      builder: (context, _) {
        final pending = state.pendingRequests;
        final resolved = state.entriesForTier(0).where((e) =>
            e.resolution == Resolution.approved ||
            e.resolution == Resolution.denied
        ).toList();

        if (pending.isEmpty && resolved.isEmpty) {
          return Scaffold(
            appBar: AppBar(title: const Text('Requests')),
            body: const _EmptyPage(
              icon: Icons.inbox_outlined,
              title: 'No pending requests',
              message: 'Human attention requests will appear here.',
            ),
          );
        }

        return Scaffold(
          appBar: AppBar(title: const Text('Requests')),
          body: ListView(
            padding: const EdgeInsets.all(16),
            children: [
              if (pending.isNotEmpty) ...[
                if (resolved.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 8, left: 4),
                    child: Text(
                      'PENDING ATTENTION (${pending.length})',
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.bold,
                        color: Colors.amber.shade800,
                        letterSpacing: 0.5,
                      ),
                    ),
                  ),
                ...pending.map((entry) => _buildRequestCard(context, entry, false)),
              ],
              if (resolved.isNotEmpty) ...[
                Padding(
                  padding: EdgeInsets.only(
                    top: pending.isNotEmpty ? 16 : 0,
                    bottom: 8,
                    left: 4,
                  ),
                  child: Text(
                    'RESOLVED (${resolved.length})',
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: FontWeight.bold,
                      color: Colors.grey.shade600,
                      letterSpacing: 0.5,
                    ),
                  ),
                ),
                ...resolved.map((entry) => _buildRequestCard(context, entry, true)),
              ],
            ],
          ),
        );
      },
    );
  }

  Widget _buildRequestCard(BuildContext context, QueueEntry entry, bool isResolved) {
    final event = state.eventFor(entry.eventId);
    if (event == null) return const SizedBox.shrink();
    final isQuestion = event.request?.questionType != null;
    final resolution = entry.resolution;

    return Card(
      margin: const EdgeInsets.only(bottom: 12),
      child: ListTile(
        contentPadding: const EdgeInsets.all(16),
        leading: CircleAvatar(
          backgroundColor: isResolved
              ? Colors.grey.withValues(alpha: .14)
              : isQuestion
                  ? Colors.orange.withValues(alpha: .14)
                  : Colors.amber.withValues(alpha: .14),
          child: Icon(
            isResolved
                ? (resolution == Resolution.approved ? Icons.check : Icons.close)
                : (isQuestion ? Icons.help_outline : Icons.lock_outline),
            color: isResolved
                ? (resolution == Resolution.approved ? Colors.green.shade700 : Colors.red.shade700)
                : (isQuestion ? Colors.orange : Colors.amber.shade800),
          ),
        ),
        title: Row(
          children: [
            Expanded(
              child: Text(
                isResolved
                    ? (resolution == Resolution.approved ? 'Approved' : 'Denied')
                    : (isQuestion ? 'Input Required' : 'Approval Required'),
                style: const TextStyle(fontWeight: FontWeight.bold),
              ),
            ),
            if (isResolved && resolution != null)
              Chip(
                label: Text(
                  resolution.wireName,
                  style: const TextStyle(fontSize: 11),
                ),
                padding: EdgeInsets.zero,
                visualDensity: VisualDensity.compact,
              ),
          ],
        ),
        subtitle: Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Text(
            '${event.request?.prompt ?? event.message}\n${event.agentName} · ${event.project}',
          ),
        ),
        isThreeLine: true,
        trailing: const Icon(Icons.chevron_right),
        onTap: () => Navigator.of(context).push(
          MaterialPageRoute(
            builder: (_) => EventScreen(
              eventId: event.eventId,
              state: state,
              connection: connection,
              metrics: metrics,
            ),
          ),
        ),
      ),
    );
  }
}

class _EmptyPage extends StatelessWidget {
  final IconData icon;
  final String title;
  final String message;

  const _EmptyPage({
    required this.icon,
    required this.title,
    required this.message,
  });

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, size: 56, color: Theme.of(context).colorScheme.primary),
            const SizedBox(height: 16),
            Text(title, style: Theme.of(context).textTheme.titleLarge),
            const SizedBox(height: 8),
            Text(message, textAlign: TextAlign.center),
          ],
        ),
      ),
    );
  }
}
