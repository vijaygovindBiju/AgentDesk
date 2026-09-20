import 'package:flutter/material.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'completed_screen.dart';
import 'home_screen.dart';
import 'new_work_screen.dart';
import 'requests_screen.dart';
import 'settings_screen.dart';
import 'working_screen.dart';

class AppShell extends StatefulWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;

  const AppShell({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
  });

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  int _index = 0;

  @override
  Widget build(BuildContext context) {
    final pages = [
      HomeScreen(
        state: widget.state,
        connection: widget.connection,
        metrics: widget.metrics,
      ),
      RequestsScreen(
        state: widget.state,
        connection: widget.connection,
        metrics: widget.metrics,
      ),
      WorkingScreen(state: widget.state),
      CompletedScreen(state: widget.state),
      NewWorkScreen(connection: widget.connection),
      SettingsScreen(connection: widget.connection),
    ];

    return Scaffold(
      body: IndexedStack(index: _index, children: pages),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (index) => setState(() => _index = index),
        destinations: const [
          NavigationDestination(
            icon: Icon(Icons.home_outlined),
            selectedIcon: Icon(Icons.home),
            label: 'Home',
          ),
          NavigationDestination(
            icon: Icon(Icons.inbox_outlined),
            selectedIcon: Icon(Icons.inbox),
            label: 'Requests',
          ),
          NavigationDestination(
            icon: Icon(Icons.bolt_outlined),
            selectedIcon: Icon(Icons.bolt),
            label: 'Working',
          ),
          NavigationDestination(
            icon: Icon(Icons.check_circle_outline),
            selectedIcon: Icon(Icons.check_circle),
            label: 'Completed',
          ),
          NavigationDestination(
            icon: Icon(Icons.add_circle_outline),
            selectedIcon: Icon(Icons.add_circle),
            label: 'New Work',
          ),
          NavigationDestination(
            icon: Icon(Icons.settings_outlined),
            selectedIcon: Icon(Icons.settings),
            label: 'Settings',
          ),
        ],
      ),
    );
  }
}
