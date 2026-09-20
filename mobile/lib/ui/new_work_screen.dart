import 'package:flutter/material.dart';
import '../services/connection_service.dart';

class NewWorkScreen extends StatefulWidget {
  final ConnectionService connection;

  const NewWorkScreen({super.key, required this.connection});

  @override
  State<NewWorkScreen> createState() => _NewWorkScreenState();
}

class _NewWorkScreenState extends State<NewWorkScreen> {
  final _promptController = TextEditingController();
  final _workspaceController = TextEditingController();

  @override
  void dispose() {
    _promptController.dispose();
    _workspaceController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('New Work')),
      body: ListView(
        padding: const EdgeInsets.all(20),
        children: [
          Text(
            'Start an independent agent task',
            style: Theme.of(context).textTheme.headlineSmall,
          ),
          const SizedBox(height: 8),
          const Text(
            'The daemon currently launches its agent session at startup. '
            'Starting a new runtime session from the phone is not exposed by the server yet.',
          ),
          const SizedBox(height: 24),
          DropdownButtonFormField<String>(
            initialValue: 'Antigravity',
            decoration: const InputDecoration(labelText: 'Agent'),
            items: const [
              DropdownMenuItem(
                value: 'Antigravity',
                child: Text('Antigravity'),
              ),
            ],
            onChanged: null,
          ),
          const SizedBox(height: 16),
          TextField(
            controller: _workspaceController,
            decoration: const InputDecoration(
              labelText: 'Workspace',
              hintText: '/projects/my-project',
            ),
          ),
          const SizedBox(height: 16),
          TextField(
            controller: _promptController,
            minLines: 5,
            maxLines: 8,
            decoration: const InputDecoration(
              labelText: 'What do you want to build?',
              hintText: 'Describe a completely new task...',
            ),
          ),
          const SizedBox(height: 24),
          FilledButton.icon(
            onPressed: null,
            icon: const Icon(Icons.play_arrow),
            label: const Text('Start Work'),
          ),
          const SizedBox(height: 12),
          Text(
            'Unavailable until the generic start-task command is implemented by the AgentDesk server.',
            style: TextStyle(color: Theme.of(context).colorScheme.error),
          ),
        ],
      ),
    );
  }
}
