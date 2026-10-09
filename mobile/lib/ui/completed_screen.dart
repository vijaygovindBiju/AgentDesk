import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'event_screen.dart';
import 'agent_control_overlay.dart';
import 'app_theme.dart';

class CompletedScreen extends StatelessWidget {
  final AgentDeskState state;
  final ConnectionService? connection;
  final ClientMetrics? metrics;

  const CompletedScreen({
    super.key,
    required this.state,
    this.connection,
    this.metrics,
  });

  Widget _buildControlAction(BuildContext context) {
    if (connection == null) return const SizedBox.shrink();
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
      onPressed: () => AgentControlOverlay.show(context, connection: connection!),
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: state,
      builder: (context, _) {
        final entries = state.entriesForTier(2);
        final isDark = Theme.of(context).brightness == Brightness.dark;

        return Scaffold(
          appBar: AppBar(
            title: const Text('Completed'),
            actions: [_buildControlAction(context), const SizedBox(width: 8)],
          ),
          body: entries.isEmpty
              ? const Center(child: Text('No completed tasks yet.'))
              : ListView.separated(
                  padding: const EdgeInsets.all(16),
                  itemCount: entries.length,
                  separatorBuilder: (_, index) => const SizedBox(height: 12),
                  itemBuilder: (context, index) {
                    final event = state.eventFor(entries[index].eventId);
                    if (event == null) return const SizedBox.shrink();
                    final finalResult = _extractDisplayResult(event);
                    final metaLine = _extractMetaLine(event);

                    return Card(
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(14),
                        side: BorderSide(
                          color: isDark ? AppTheme.cardBorder : Colors.grey.shade300,
                        ),
                      ),
                      child: ListTile(
                        contentPadding: const EdgeInsets.all(16),
                        leading: CircleAvatar(
                          backgroundColor: isDark
                              ? AppTheme.surfaceCardElevated
                              : Colors.green.shade50,
                          child: Icon(
                            Icons.check,
                            color: isDark ? AppTheme.primaryEmerald : Colors.green.shade700,
                          ),
                        ),
                        title: Text(
                          event.summary,
                          style: const TextStyle(fontWeight: FontWeight.bold),
                        ),
                        subtitle: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            const SizedBox(height: 2),
                            Text(
                              '${event.agentName} · ${event.project}',
                              style: TextStyle(
                                color: Colors.grey.shade600,
                                fontSize: 12,
                                fontWeight: FontWeight.w500,
                              ),
                            ),
                            const SizedBox(height: 4),
                            Text(
                              finalResult,
                              maxLines: 2,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(fontWeight: FontWeight.w500),
                            ),
                            if (metaLine != null) ...[
                              const SizedBox(height: 4),
                              Text(
                                metaLine,
                                style: TextStyle(
                                  color: Colors.grey.shade500,
                                  fontSize: 11,
                                ),
                              ),
                            ],
                          ],
                        ),
                        isThreeLine: true,
                        trailing: const Icon(Icons.chevron_right),
                        onTap: connection == null
                            ? null
                            : () {
                                metrics?.recordTap();
                                Navigator.of(context).push(
                                  MaterialPageRoute(
                                    builder: (_) => EventScreen(
                                      eventId: event.eventId,
                                      state: state,
                                      connection: connection!,
                                      metrics: metrics ?? ClientMetrics(),
                                    ),
                                  ),
                                );
                              },
                      ),
                    );
                  },
                ),
        );
      },
    );
  }

  String _extractDisplayResult(Event event) {
    final finalRes = event.details['final_result'];
    if (finalRes != null && finalRes.toString().trim().isNotEmpty) {
      return finalRes.toString().trim();
    }
    // Fallback: extract the final human result block from the message
    final lines = event.message.split('\n');
    final resultLines = <String>[];
    for (int i = lines.length - 1; i >= 0; i--) {
      final t = lines[i].trim();
      if (t.isEmpty) {
        if (resultLines.isNotEmpty && resultLines.last.isNotEmpty) {
          resultLines.add('');
        }
        continue;
      }
      if (t.startsWith('●') ||
          t.startsWith('▸') ||
          t.startsWith('Thought for') ||
          t.startsWith('>') ||
          t.startsWith('───') ||
          t.contains('(ctrl+o to expand)')) {
        if (resultLines.isNotEmpty) {
          break;
        }
        continue;
      }
      resultLines.add(t);
    }
    if (resultLines.isNotEmpty) {
      final reversed = resultLines.reversed.toList();
      while (reversed.isNotEmpty && reversed.first.isEmpty) {
        reversed.removeAt(0);
      }
      while (reversed.isNotEmpty && reversed.last.isEmpty) {
        reversed.removeLast();
      }
      final joined = reversed.join('\n').trim();
      if (joined.isNotEmpty) {
        return joined;
      }
    }
    return event.summary;
  }

  String? _extractMetaLine(Event event) {
    final parts = <String>[];
    final toolCount = event.details['tool_count'];
    if (toolCount != null) {
      parts.add('$toolCount tool call${toolCount == 1 ? '' : 's'}');
    } else {
      final count = event.message
          .split('\n')
          .where((String l) => l.trim().startsWith('● '))
          .length;
      if (count > 0) {
        parts.add('$count tool call${count == 1 ? '' : 's'}');
      }
    }

    final match = RegExp(r'Thought for ([\w\d]+)').firstMatch(event.message);
    if (match != null) {
      parts.add(match.group(1)!);
    }

    return parts.isEmpty ? null : parts.join(' · ');
  }
}
