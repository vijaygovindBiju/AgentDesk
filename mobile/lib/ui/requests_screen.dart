import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'event_screen.dart';
import 'agent_control_overlay.dart';
import 'app_theme.dart';

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

  Widget _buildControlAction(BuildContext context) {
    return IconButton(
      icon: Container(
        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
        decoration: BoxDecoration(
          color: AppTheme.primaryEmerald.withValues(alpha: 0.15),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: AppTheme.primaryEmerald.withValues(alpha: 0.35),
          ),
        ),
        child: const Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.tune, size: 14, color: AppTheme.primaryEmerald),
            SizedBox(width: 4),
            Text(
              'Controls',
              style: TextStyle(
                fontSize: 11,
                fontWeight: FontWeight.bold,
                color: AppTheme.primaryEmerald,
              ),
            ),
          ],
        ),
      ),
      tooltip: 'Agent Controls & Quotas',
      onPressed: () => AgentControlOverlay.show(context, connection: connection),
    );
  }

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
            appBar: AppBar(
              title: const Text('Requests'),
              actions: [_buildControlAction(context), const SizedBox(width: 8)],
            ),
            body: const _EmptyPage(
              icon: Icons.check_circle_outline,
              title: 'No pending requests',
              message: 'When the coding agent needs human approval or input, it will appear here.',
            ),
          );
        }

        return Scaffold(
          appBar: AppBar(
            title: const Text('Requests'),
            actions: [_buildControlAction(context), const SizedBox(width: 8)],
          ),
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
    if (event == null || event.category == Category.completed || event.kind == 'task_completed') {
      return const SizedBox.shrink();
    }
    final isQuestion = event.request?.questionType != null;
    final resolution = entry.resolution;

    final isDark = Theme.of(context).brightness == Brightness.dark;

    return Card(
      key: Key('entry_card_${entry.eventId}'),
      margin: const EdgeInsets.only(bottom: 12),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(14),
        side: BorderSide(
          color: !isResolved
              ? AppTheme.warningAmber
              : (isDark ? AppTheme.cardBorder : Colors.grey.shade300),
          width: !isResolved ? 1.5 : 1.0,
        ),
      ),
      child: ListTile(
        contentPadding: const EdgeInsets.all(16),
        leading: CircleAvatar(
          backgroundColor: isResolved
              ? (isDark ? AppTheme.surfaceCardElevated : Colors.grey.withValues(alpha: .14))
              : (isQuestion
                  ? AppTheme.warningAmber.withValues(alpha: .2)
                  : AppTheme.warningAmber.withValues(alpha: .25)),
          child: Icon(
            isResolved
                ? (resolution == Resolution.approved ? Icons.check : Icons.close)
                : (isQuestion ? Icons.help_outline : Icons.lock_outline),
            color: isResolved
                ? (resolution == Resolution.approved ? AppTheme.primaryEmerald : AppTheme.errorRed)
                : (isQuestion ? Colors.orange : AppTheme.warningAmber),
          ),
        ),
        title: Row(
          children: [
            Expanded(
              child: Text(
                event.summary.isNotEmpty
                    ? event.summary
                    : (isResolved
                        ? (resolution == Resolution.approved ? 'Approved' : 'Denied')
                        : (isQuestion ? 'Input Required' : 'Approval Required')),
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
              )
            else if (!isResolved)
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                decoration: BoxDecoration(
                  color: AppTheme.warningAmber.withValues(alpha: 0.2),
                  borderRadius: BorderRadius.circular(6),
                ),
                child: const Text(
                  'ACTION NEEDED',
                  style: TextStyle(
                    fontSize: 9,
                    fontWeight: FontWeight.bold,
                    color: AppTheme.warningAmber,
                  ),
                ),
              ),
          ],
        ),
        subtitle: Padding(
          padding: const EdgeInsets.only(top: 8),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (event.message.isNotEmpty) ...[
                Text(
                  event.message,
                  style: TextStyle(
                    fontSize: 13,
                    color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                  ),
                ),
                const SizedBox(height: 6),
              ],
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(10),
                decoration: BoxDecoration(
                  color: isDark ? AppTheme.surfaceCardElevated : Colors.grey.shade100,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(
                    color: isDark ? AppTheme.cardBorder : Colors.grey.shade300,
                  ),
                ),
                child: Text(
                  event.request?.prompt ?? event.message,
                  style: const TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 12,
                  ),
                  maxLines: 3,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(height: 6),
              Text(
                '${event.agentName} · ${event.project}',
                style: TextStyle(
                  fontSize: 11,
                  color: isDark ? AppTheme.textSecondary : Colors.grey.shade600,
                ),
              ),
              if (entry.state == EntryState.dismissed &&
                  (entry.resolution == Resolution.unresolved ||
                      entry.resolution == Resolution.pendingDelivery)) ...[
                const SizedBox(height: 6),
                Container(
                  key: Key('still_blocking_badge_${entry.eventId}'),
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: AppTheme.warningAmber.withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(4),
                  ),
                  child: const Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.hourglass_empty, size: 12, color: AppTheme.warningAmber),
                      SizedBox(width: 4),
                      Text(
                        'Still blocking agent',
                        style: TextStyle(
                          fontSize: 10,
                          fontWeight: FontWeight.bold,
                          color: AppTheme.warningAmber,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ],
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
