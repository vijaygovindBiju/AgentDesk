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
  bool _isSubmitting = false;

  @override
  void dispose() {
    _promptController.dispose();
    _workspaceController.dispose();
    super.dispose();
  }

  Future<void> _submitPrompt() async {
    final prompt = _promptController.text.trim();
    if (prompt.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Please enter a prompt')),
      );
      return;
    }

    setState(() {
      _isSubmitting = true;
    });

    try {
      final workspace = _workspaceController.text.trim().isNotEmpty
          ? _workspaceController.text.trim()
          : null;
      final result = await widget.connection.startTask(
        prompt: prompt,
        workspace: workspace,
      );

      if (!mounted) return;

      if (result.ok) {
        _promptController.clear();
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Prompt sent to AgentDesk terminal! 🚀'),
            backgroundColor: Colors.green,
          ),
        );
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to submit prompt: ${result.error?.wireName ?? "unknown error"}'),
            backgroundColor: Colors.red,
          ),
        );
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Error: $e'),
          backgroundColor: Colors.red,
        ),
      );
    } finally {
      if (mounted) {
        setState(() {
          _isSubmitting = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.connection,
      builder: (context, _) {
        final isConnected = widget.connection.status == ConnectionStatus.connected;

        return Scaffold(
          appBar: AppBar(title: const Text('New Work')),
          body: ListView(
            padding: const EdgeInsets.all(20),
            children: [
              Text(
                'Send prompt to coding agent',
                style: Theme.of(context).textTheme.headlineSmall,
              ),
              const SizedBox(height: 8),
              const Text(
                'Send instructions directly to the active coding agent on your laptop. '
                'The prompt will be executed in the terminal session.',
              ),
              const SizedBox(height: 24),
              DropdownButtonFormField<String>(
                initialValue: 'Antigravity',
                decoration: const InputDecoration(labelText: 'Agent'),
                items: const [
                  DropdownMenuItem(
                    value: 'Antigravity',
                    child: Text('Antigravity CLI (agy)'),
                  ),
                ],
                onChanged: (_) {},
              ),
              const SizedBox(height: 16),
              TextField(
                controller: _workspaceController,
                decoration: const InputDecoration(
                  labelText: 'Workspace (Optional)',
                  hintText: '/projects/my-project',
                ),
              ),
              const SizedBox(height: 16),
              TextField(
                controller: _promptController,
                minLines: 5,
                maxLines: 8,
                decoration: const InputDecoration(
                  labelText: 'Prompt / Instruction',
                  hintText: 'Describe what you want the agent to do...',
                ),
              ),
              const SizedBox(height: 24),
              FilledButton.icon(
                onPressed: (isConnected && !_isSubmitting) ? _submitPrompt : null,
                icon: _isSubmitting
                    ? const SizedBox(
                        width: 18,
                        height: 18,
                        child: CircularProgressIndicator(strokeWidth: 2, color: Colors.white),
                      )
                    : const Icon(Icons.send),
                label: Text(_isSubmitting ? 'Sending...' : 'Send Prompt to Agent'),
              ),
              if (!isConnected) ...[
                const SizedBox(height: 12),
                Text(
                  'Phone is not connected to AgentDesk. Please pair or connect first.',
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ],
            ],
          ),
        );
      },
    );
  }
}
