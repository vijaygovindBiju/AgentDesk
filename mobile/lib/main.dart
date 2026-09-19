import 'package:flutter/material.dart';
import 'services/client_metrics.dart';
import 'services/connection_service.dart';
import 'state/agentdesk_state.dart';
import 'ui/home_screen.dart';

void main() {
  final state = AgentDeskState();
  final connection = ConnectionService(state: state);
  final metrics = ClientMetrics();

  runApp(AgentDeskApp(
    state: state,
    connection: connection,
    metrics: metrics,
  ));
}

class AgentDeskApp extends StatelessWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;

  const AgentDeskApp({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
  });

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'AgentDesk',
      theme: ThemeData(
        colorSchemeSeed: Colors.indigo,
        useMaterial3: true,
      ),
      home: HomeScreen(
        state: state,
        connection: connection,
        metrics: metrics,
      ),
    );
  }
}
