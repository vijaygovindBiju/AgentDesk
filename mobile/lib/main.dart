import 'package:flutter/material.dart';

void main() {
  runApp(const AgentDeskApp());
}

/// AgentDesk mobile client. Phase 1 skeleton: no connection yet, only the
/// four-tier layout that Phase 7 fills in. See docs/ARCHITECTURE.md.
class AgentDeskApp extends StatelessWidget {
  const AgentDeskApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'AgentDesk',
      theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
      home: const HomeScreen(),
    );
  }
}

/// Tier order is fixed by the event model: request > error > completed > working.
const tierLabels = ['Requests', 'Errors', 'Completed', 'Working'];

class HomeScreen extends StatelessWidget {
  const HomeScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('AgentDesk')),
      body: ListView(
        children: [
          for (final label in tierLabels)
            ListTile(
              title: Text(label),
              subtitle: const Text('Not connected'),
            ),
        ],
      ),
    );
  }
}
