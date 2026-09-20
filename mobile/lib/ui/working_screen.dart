import 'package:flutter/material.dart';
import '../state/agentdesk_state.dart';

class WorkingScreen extends StatelessWidget {
  final AgentDeskState state;

  const WorkingScreen({super.key, required this.state});

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: state,
      builder: (context, _) {
        final entries = state.entriesForTier(3);
        return Scaffold(
          appBar: AppBar(title: const Text('Working')),
          body: entries.isEmpty
              ? const Center(child: Text('No active background tasks.'))
              : ListView.separated(
                  padding: const EdgeInsets.all(16),
                  itemCount: entries.length,
                  separatorBuilder: (_, index) => const SizedBox(height: 12),
                  itemBuilder: (context, index) {
                    final event = state.eventFor(entries[index].eventId);
                    if (event == null) return const SizedBox.shrink();
                    return Card(
                      child: ListTile(
                        contentPadding: const EdgeInsets.all(16),
                        leading: const CircleAvatar(child: Icon(Icons.bolt)),
                        title: Text(event.agentName),
                        subtitle: Text('${event.project}\n${event.message}'),
                        isThreeLine: true,
                        trailing: Chip(
                          label: const Text('Running'),
                          backgroundColor: Colors.blue.withValues(alpha: .12),
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
