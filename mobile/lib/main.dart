import 'package:flutter/material.dart';
import 'services/client_metrics.dart';
import 'services/connection_service.dart';
import 'state/agentdesk_state.dart';
import 'ui/home_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final state = AgentDeskState();
  final connection = ConnectionService(state: state);
  final metrics = ClientMetrics();

  // Load the persisted non-secret settings and secure token before any
  // connection is attempted. A complete configuration reconnects on launch.
  await connection.restoreConfiguration();

  runApp(AgentDeskApp(state: state, connection: connection, metrics: metrics));

  if (connection.isConfigurationComplete) {
    connection.connect();
  }
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
      theme: ThemeData(colorSchemeSeed: Colors.indigo, useMaterial3: true),
      home: HomeScreen(state: state, connection: connection, metrics: metrics),
    );
  }
}
