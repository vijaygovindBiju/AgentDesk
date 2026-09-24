import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'event_screen.dart';

/// Working screen: shows a clean status dashboard instead of a card list.
/// Displays whether the agent is working or idle, with a compact task summary.
class WorkingScreen extends StatelessWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;

  const WorkingScreen({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
  });

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
      listenable: state,
      builder: (context, _) {
        final activeCount = state.activeWorkingCount;
        final isWorking = activeCount > 0;

        // Get the latest working event for summary info
        final workingEntries = state.entriesForTier(3);
        final activeEntries = workingEntries
            .where((e) => !state.isFinished(e.eventId))
            .toList();

        return Scaffold(
          appBar: AppBar(title: const Text('Agent Status')),
          body: Padding(
            padding: const EdgeInsets.all(20),
            child: Column(
              children: [
                // Main status indicator
                _StatusCard(isWorking: isWorking, activeCount: activeCount),
                const SizedBox(height: 24),
                // Active task list (compact, no card flood)
                if (activeEntries.isNotEmpty) ...[
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Text(
                      'Active Tasks',
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                            fontWeight: FontWeight.bold,
                          ),
                    ),
                  ),
                  const SizedBox(height: 8),
                  Expanded(
                    child: ListView.builder(
                      itemCount: activeEntries.length,
                      itemBuilder: (context, index) {
                        final entry = activeEntries[index];
                        final event = state.eventFor(entry.eventId);
                        if (event == null) return const SizedBox.shrink();
                        return _CompactTaskRow(
                          event: event,
                          onTap: () => _openEvent(context, entry.eventId),
                        );
                      },
                    ),
                  ),
                ] else
                  Expanded(
                    child: Center(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(
                            Icons.check_circle_outline,
                            size: 48,
                            color: Colors.green.shade300,
                          ),
                          const SizedBox(height: 12),
                          Text(
                            'No active tasks',
                            style: TextStyle(
                              fontSize: 15,
                              color: Colors.grey.shade600,
                            ),
                          ),
                          const SizedBox(height: 4),
                          Text(
                            'The agent is ready for new work',
                            style: TextStyle(
                              fontSize: 13,
                              color: Colors.grey.shade500,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
        );
      },
    );
  }
}

/// Large status card showing Working / Idle
class _StatusCard extends StatelessWidget {
  final bool isWorking;
  final int activeCount;

  const _StatusCard({required this.isWorking, required this.activeCount});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(24),
      decoration: BoxDecoration(
        gradient: LinearGradient(
          colors: isWorking
              ? [Colors.blue.shade700, Colors.blue.shade500]
              : [Colors.green.shade700, Colors.green.shade500],
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
        ),
        borderRadius: BorderRadius.circular(16),
        boxShadow: [
          BoxShadow(
            color: (isWorking ? Colors.blue : Colors.green).withValues(alpha: 0.3),
            blurRadius: 12,
            offset: const Offset(0, 4),
          ),
        ],
      ),
      child: Column(
        children: [
          Icon(
            isWorking ? Icons.bolt : Icons.check_circle_outline,
            size: 48,
            color: Colors.white,
          ),
          const SizedBox(height: 12),
          Text(
            isWorking ? 'Agent Working' : 'Agent Idle',
            style: const TextStyle(
              fontSize: 24,
              fontWeight: FontWeight.bold,
              color: Colors.white,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            isWorking
                ? '$activeCount active task${activeCount == 1 ? '' : 's'}'
                : 'Ready for input',
            style: TextStyle(
              fontSize: 14,
              color: Colors.white.withValues(alpha: 0.85),
            ),
          ),
          if (isWorking) ...[
            const SizedBox(height: 12),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                minHeight: 4,
                backgroundColor: Colors.white.withValues(alpha: 0.25),
                valueColor: const AlwaysStoppedAnimation(Colors.white),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

/// Compact single-line task row (not a full card)
class _CompactTaskRow extends StatelessWidget {
  final Event event;
  final VoidCallback onTap;

  const _CompactTaskRow({required this.event, required this.onTap});

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(8),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 8, horizontal: 4),
        child: Row(
          children: [
            Container(
              width: 8,
              height: 8,
              decoration: BoxDecoration(
                color: Colors.blue.shade500,
                shape: BoxShape.circle,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    event.summary,
                    style: const TextStyle(
                      fontWeight: FontWeight.w600,
                      fontSize: 13,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                  Text(
                    '${event.agentName} · ${event.message}',
                    style: TextStyle(
                      fontSize: 12,
                      color: Colors.grey.shade600,
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ],
              ),
            ),
            Icon(
              Icons.chevron_right,
              size: 18,
              color: Colors.grey.shade400,
            ),
          ],
        ),
      ),
    );
  }
}
