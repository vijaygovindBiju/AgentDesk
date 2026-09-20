import 'package:flutter/material.dart';
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
        final requests = state.entriesForTier(0);
        return Scaffold(
          appBar: AppBar(title: const Text('Requests')),
          body: requests.isEmpty
              ? const _EmptyPage(
                  icon: Icons.inbox_outlined,
                  title: 'No pending requests',
                  message: 'Human attention requests will appear here.',
                )
              : ListView.separated(
                  padding: const EdgeInsets.all(16),
                  itemCount: requests.length,
                  separatorBuilder: (_, index) => const SizedBox(height: 12),
                  itemBuilder: (context, index) {
                    final entry = requests[index];
                    final event = state.eventFor(entry.eventId);
                    if (event == null) return const SizedBox.shrink();
                    final isQuestion = event.request?.questionType != null;
                    return Card(
                      child: ListTile(
                        contentPadding: const EdgeInsets.all(16),
                        leading: CircleAvatar(
                          backgroundColor: isQuestion
                              ? Colors.orange.withValues(alpha: .14)
                              : Colors.amber.withValues(alpha: .14),
                          child: Icon(
                            isQuestion
                                ? Icons.help_outline
                                : Icons.lock_outline,
                            color: isQuestion
                                ? Colors.orange
                                : Colors.amber.shade800,
                          ),
                        ),
                        title: Text(
                          isQuestion ? 'Input Required' : 'Approval Required',
                          style: const TextStyle(fontWeight: FontWeight.bold),
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
                  },
                ),
        );
      },
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
