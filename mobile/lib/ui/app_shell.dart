import 'package:flutter/material.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../services/onboarding_store.dart';
import '../state/agentdesk_state.dart';
import 'completed_screen.dart';
import 'home_screen.dart';
import 'new_work_screen.dart';
import 'requests_screen.dart';
import 'working_screen.dart';
import 'onboarding_screen.dart';
import 'app_theme.dart';

class AppShell extends StatefulWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;
  final OnboardingStore onboardingStore;

  const AppShell({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
    required this.onboardingStore,
  });

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  int _index = 0;
  bool _showOnboarding = false;
  String? _newWorkInitialPrompt;

  @override
  Widget build(BuildContext context) {
    if (_showOnboarding) {
      return OnboardingScreen(
        onComplete: () async {
          await widget.onboardingStore.markComplete();
          if (mounted) setState(() => _showOnboarding = false);
        },
      );
    }

    return ListenableBuilder(
      listenable: widget.state,
      builder: (context, _) {
        final requestCount = widget.state.pendingRequestCount;
        final activeWorkingCount = widget.state.activeWorkingCount;
        final completedCount = widget.state.entriesForTier(2).length;

        final pages = [
          HomeScreen(
            state: widget.state,
            connection: widget.connection,
            metrics: widget.metrics,
            onShowIntroduction: () => setState(() => _showOnboarding = true),
            onNavigateToTab: (index, {prompt}) {
              setState(() {
                _index = index;
                if (prompt != null) {
                  _newWorkInitialPrompt = prompt;
                }
              });
            },
          ),
          RequestsScreen(
            state: widget.state,
            connection: widget.connection,
            metrics: widget.metrics,
          ),
          WorkingScreen(
            state: widget.state,
            connection: widget.connection,
            metrics: widget.metrics,
          ),
          CompletedScreen(
            state: widget.state,
            connection: widget.connection,
            metrics: widget.metrics,
          ),
          NewWorkScreen(
            connection: widget.connection,
            initialPrompt: _newWorkInitialPrompt,
            onPromptConsumed: () {
              _newWorkInitialPrompt = null;
            },
            onSubmitted: () {
              setState(() => _index = 0);
            },
          ),
        ];

        return Scaffold(
          body: IndexedStack(index: _index, children: pages),
          bottomNavigationBar: NavigationBar(
            selectedIndex: _index,
            onDestinationSelected: (index) => setState(() => _index = index),
            destinations: [
              const NavigationDestination(
                icon: Icon(Icons.home_outlined),
                selectedIcon: Icon(Icons.home),
                label: 'Home',
              ),
              NavigationDestination(
                icon: Badge(
                  isLabelVisible: requestCount > 0,
                  label: Text('$requestCount'),
                  backgroundColor: AppTheme.warningAmber,
                  textColor: Colors.black,
                  child: const Icon(Icons.inbox_outlined),
                ),
                selectedIcon: Badge(
                  isLabelVisible: requestCount > 0,
                  label: Text('$requestCount'),
                  backgroundColor: AppTheme.warningAmber,
                  textColor: Colors.black,
                  child: const Icon(Icons.inbox),
                ),
                label: 'Requests',
              ),
              NavigationDestination(
                icon: activeWorkingCount > 0
                    ? Badge(
                        label: Text('$activeWorkingCount'),
                        backgroundColor: AppTheme.primaryCyan,
                        textColor: Colors.black,
                        child: const Icon(Icons.bolt, color: AppTheme.primaryCyan),
                      )
                    : Badge(
                        smallSize: 8,
                        backgroundColor: AppTheme.primaryEmerald,
                        child: const Icon(Icons.check_circle_outline, color: AppTheme.primaryEmerald),
                      ),
                selectedIcon: activeWorkingCount > 0
                    ? Badge(
                        label: Text('$activeWorkingCount'),
                        backgroundColor: AppTheme.primaryCyan,
                        textColor: Colors.black,
                        child: const Icon(Icons.bolt, color: AppTheme.primaryCyan),
                      )
                    : Badge(
                        smallSize: 8,
                        backgroundColor: AppTheme.primaryEmerald,
                        child: const Icon(Icons.check_circle, color: AppTheme.primaryEmerald),
                      ),
                label: activeWorkingCount > 0 ? 'Working' : 'Idle',
              ),
              NavigationDestination(
                icon: Badge(
                  isLabelVisible: completedCount > 0,
                  label: Text('$completedCount'),
                  backgroundColor: AppTheme.primaryEmerald,
                  textColor: Colors.black,
                  child: const Icon(Icons.check_circle_outline),
                ),
                selectedIcon: Badge(
                  isLabelVisible: completedCount > 0,
                  label: Text('$completedCount'),
                  backgroundColor: AppTheme.primaryEmerald,
                  textColor: Colors.black,
                  child: const Icon(Icons.check_circle),
                ),
                label: 'Completed',
              ),
              const NavigationDestination(
                icon: Icon(Icons.add_circle_outline),
                selectedIcon: Icon(Icons.add_circle),
                label: 'New Work',
              ),
            ],
          ),
        );
      },
    );
  }
}
