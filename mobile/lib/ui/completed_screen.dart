import 'package:flutter/material.dart';
import '../state/agentdesk_state.dart';

class CompletedScreen extends StatelessWidget {
  final AgentDeskState state;

  const CompletedScreen({super.key, required this.state});

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: state,
      builder: (context, _) {
        final entries = state.entriesForTier(2);
        return Scaffold(
          appBar: AppBar(title: const Text('Completed')),
          body: entries.isEmpty
              ? const Center(child: Text('No completed tasks yet.'))
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
                        leading: const CircleAvatar(
                          backgroundColor: Colors.green,
                          child: Icon(Icons.check, color: Colors.white),
                        ),
                        title: Text(event.summary),
                        subtitle: Text(
                          '${event.agentName} · ${event.project}\n${event.message}',
                        ),
                        isThreeLine: true,
                        trailing: const Icon(Icons.chevron_right),
                      ),
                    );
                  },
                ),
        );
      },
    );
  }
}
