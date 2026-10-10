import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'event_screen.dart';
import 'agent_control_overlay.dart';
import 'app_theme.dart';

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
      listenable: state,
      builder: (context, _) {
        final activeCount = state.activeWorkingCount;
        final isWorking = activeCount > 0;
        final isDark = Theme.of(context).brightness == Brightness.dark;

        // Get the latest working event for summary info
        final workingEntries = state.entriesForTier(3);
        final activeEntries = workingEntries
            .where((e) => !state.isFinished(e.eventId))
            .toList();

        return Scaffold(
          appBar: AppBar(
            title: const Text('Agent Status'),
            actions: [_buildControlAction(context), const SizedBox(width: 8)],
          ),
          body: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              children: [
                // Main status indicator
                _StatusCard(isWorking: isWorking, activeCount: activeCount),
                const SizedBox(height: 20),

                // Active task list
                if (activeEntries.isNotEmpty) ...[
                  Align(
                    alignment: Alignment.centerLeft,
                    child: Text(
                      'ACTIVE TASKS (${activeEntries.length})',
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.bold,
                        color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                        letterSpacing: 0.5,
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
                ] else ...[
                  // Idle view with Telemetry & Quota Card
                  Expanded(
                    child: Column(
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        Icon(
                          Icons.check_circle_outline,
                          size: 48,
                          color: isDark ? AppTheme.primaryEmerald : Colors.green.shade400,
                        ),
                        const SizedBox(height: 12),
                        const Text(
                          'No active tasks',
                          style: TextStyle(fontSize: 16, fontWeight: FontWeight.bold),
                        ),
                        const SizedBox(height: 4),
                        Text(
                          'The agent is ready for new work',
                          style: TextStyle(
                            fontSize: 13,
                            color: isDark ? AppTheme.textSecondary : Colors.grey.shade600,
                          ),
                        ),
                        const SizedBox(height: 24),

                        // Telemetry & Control Card
                        Container(
                          padding: const EdgeInsets.all(16),
                          decoration: BoxDecoration(
                            color: isDark ? AppTheme.surfaceCard : Colors.grey.shade100,
                            borderRadius: BorderRadius.circular(16),
                            border: Border.all(
                              color: isDark ? AppTheme.cardBorder : Colors.grey.shade300,
                            ),
                          ),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  const Icon(Icons.dns_outlined, size: 16, color: AppTheme.primaryEmerald),
                                  const SizedBox(width: 8),
                                  const Text(
                                    'TELEMETRY & MODEL',
                                    style: TextStyle(
                                      fontSize: 11,
                                      fontWeight: FontWeight.bold,
                                      letterSpacing: 0.5,
                                    ),
                                  ),
                                  const Spacer(),
                                  Container(
                                    padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                                    decoration: BoxDecoration(
                                      color: AppTheme.primaryEmerald.withValues(alpha: 0.15),
                                      borderRadius: BorderRadius.circular(6),
                                    ),
                                    child: const Text(
                                      'ONLINE',
                                      style: TextStyle(
                                        fontSize: 9,
                                        fontWeight: FontWeight.bold,
                                        color: AppTheme.primaryEmerald,
                                      ),
                                    ),
                                  ),
                                ],
                              ),
                              const SizedBox(height: 12),
                              _buildTelemetryRow('Daemon', 'v${state.welcome?.daemonVersion ?? "0.1.0"}'),
                              const SizedBox(height: 6),
                              _buildTelemetryRow('Transport', state.welcome?.transport.name.toUpperCase() ?? 'RELAY'),
                              const SizedBox(height: 6),
                              _buildTelemetryRow('Pipeline', state.welcome?.pipelineMode.name.toUpperCase() ?? 'AGENTDESK'),
                              const SizedBox(height: 6),
                              _buildTelemetryRow('Status', connection.status == ConnectionStatus.connected ? 'Connected' : connection.status.name),
                              const SizedBox(height: 14),
                              SizedBox(
                                width: double.infinity,
                                child: OutlinedButton.icon(
                                  onPressed: () => AgentControlOverlay.show(context, connection: connection),
                                  style: OutlinedButton.styleFrom(
                                    side: BorderSide(
                                      color: isDark ? AppTheme.cardBorder : Colors.grey.shade400,
                                    ),
                                    foregroundColor: isDark ? AppTheme.textPrimary : Colors.black87,
                                    padding: const EdgeInsets.symmetric(vertical: 12),
                                    shape: RoundedRectangleBorder(
                                      borderRadius: BorderRadius.circular(10),
                                    ),
                                  ),
                                  icon: const Icon(Icons.tune, size: 16),
                                  label: const Text('Agent Controls', style: TextStyle(fontSize: 13)),
                                ),
                              ),
                            ],
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }

  Widget _buildTelemetryRow(String label, String value) {
    return Row(
      mainAxisAlignment: MainAxisAlignment.spaceBetween,
      children: [
        Text(
          label,
          style: const TextStyle(fontSize: 12, color: AppTheme.textSecondary),
        ),
        Text(
          value,
          style: const TextStyle(
            fontSize: 12,
            fontFamily: 'monospace',
            fontWeight: FontWeight.w600,
          ),
        ),
      ],
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
    final isDark = Theme.of(context).brightness == Brightness.dark;

    return Container(
      width: double.infinity,
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: isDark
            ? (isWorking ? const Color(0xFF161F2E) : AppTheme.surfaceCard)
            : (isWorking ? Colors.blue.shade50 : Colors.green.shade50),
        borderRadius: BorderRadius.circular(16),
        border: Border.all(
          color: isWorking ? AppTheme.primaryCyan : AppTheme.primaryEmerald.withValues(alpha: 0.5),
          width: 1.5,
        ),
      ),
      child: Column(
        children: [
          Icon(
            isWorking ? Icons.bolt : Icons.check_circle_outline,
            size: 44,
            color: isDark
                ? (isWorking ? AppTheme.primaryCyan : AppTheme.primaryEmerald)
                : Colors.white,
          ),
          const SizedBox(height: 10),
          Text(
            isWorking ? 'Agent Working' : 'Agent Idle',
            style: TextStyle(
              fontSize: 22,
              fontWeight: FontWeight.bold,
              color: isDark ? AppTheme.textPrimary : Colors.white,
            ),
          ),
          const SizedBox(height: 4),
          Text(
            isWorking
                ? '$activeCount active task${activeCount == 1 ? '' : 's'}'
                : 'Ready for input',
            style: TextStyle(
              fontSize: 13,
              color: isDark ? AppTheme.textSecondary : Colors.white.withValues(alpha: 0.85),
            ),
          ),
          if (isWorking) ...[
            const SizedBox(height: 12),
            ClipRRect(
              borderRadius: BorderRadius.circular(4),
              child: LinearProgressIndicator(
                minHeight: 4,
                backgroundColor: isDark ? AppTheme.cardBorder : Colors.white.withValues(alpha: 0.25),
                valueColor: AlwaysStoppedAnimation(isDark ? AppTheme.primaryCyan : Colors.white),
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
    final isDark = Theme.of(context).brightness == Brightness.dark;

    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        color: isDark ? AppTheme.surfaceCard : Colors.white,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: isDark ? AppTheme.cardBorder : Colors.grey.shade300,
        ),
      ),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.symmetric(vertical: 12, horizontal: 14),
          child: Row(
            children: [
              Container(
                width: 8,
                height: 8,
                decoration: const BoxDecoration(
                  color: AppTheme.primaryCyan,
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
                    const SizedBox(height: 2),
                    Text(
                      '${event.agentName} · ${event.message}',
                      style: TextStyle(
                        fontSize: 11,
                        color: isDark ? AppTheme.textSecondary : Colors.grey.shade600,
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
                color: isDark ? AppTheme.textTertiary : Colors.grey.shade400,
              ),
            ],
          ),
        ),
      ),
    );
  }
}
