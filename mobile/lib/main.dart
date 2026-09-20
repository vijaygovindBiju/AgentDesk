import 'package:flutter/material.dart';
import 'services/client_metrics.dart';
import 'services/connection_service.dart';
import 'services/onboarding_store.dart';
import 'state/agentdesk_state.dart';
import 'ui/app_shell.dart';
import 'ui/app_theme.dart';
import 'ui/onboarding_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final state = AgentDeskState();
  final connection = ConnectionService(state: state);
  final metrics = ClientMetrics();
  final onboardingStore = SharedPreferencesOnboardingStore();

  // Load the persisted non-secret settings and secure token before any
  // connection is attempted. A complete configuration reconnects on launch.
  await connection.restoreConfiguration();
  final onboardingComplete = await onboardingStore.isComplete();

  runApp(
    AgentDeskApp(
      state: state,
      connection: connection,
      metrics: metrics,
      onboardingComplete: onboardingComplete,
      onboardingStore: onboardingStore,
    ),
  );

  if (connection.isConfigurationComplete) {
    connection.connect();
  }
}

class AgentDeskApp extends StatefulWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;
  final bool onboardingComplete;
  final OnboardingStore onboardingStore;

  const AgentDeskApp({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
    required this.onboardingComplete,
    required this.onboardingStore,
  });

  @override
  State<AgentDeskApp> createState() => _AgentDeskAppState();
}

class _AgentDeskAppState extends State<AgentDeskApp> {
  late bool _onboardingComplete = widget.onboardingComplete;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'AgentDesk',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light(),
      darkTheme: AppTheme.dark(),
      themeMode: ThemeMode.system,
      home: _onboardingComplete
          ? AppShell(
              state: widget.state,
              connection: widget.connection,
              metrics: widget.metrics,
              onboardingStore: widget.onboardingStore,
            )
          : OnboardingScreen(
              onComplete: () async {
                await widget.onboardingStore.markComplete();
                if (mounted) setState(() => _onboardingComplete = true);
              },
            ),
    );
  }
}
