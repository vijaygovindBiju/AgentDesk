import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:agentdesk/models/models.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:agentdesk/ui/new_work_screen.dart';

class MockConnectionService extends ConnectionService {
  String? lastSubmittedPrompt;
  String? lastSubmittedWorkspace;
  String? lastSubmittedMode;

  MockConnectionService() : super(state: AgentDeskState());

  @override
  ConnectionStatus get status => ConnectionStatus.connected;

  @override
  Future<CommandResult> startTask({
    required String prompt,
    String? workspace,
    String? agentId,
    String? mode,
  }) async {
    lastSubmittedPrompt = prompt;
    lastSubmittedWorkspace = workspace;
    lastSubmittedMode = mode;
    return const CommandResult(ok: true);
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  setUp(() {
    SharedPreferences.setMockInitialValues({
      'agentdesk_recent_workspaces': [
        '/media/pirate/Shared/currently working/AgentDesk',
        '/home/pirate/projects/myapp',
      ],
    });
  });

  testWidgets('NewWorkScreen renders workspace directory and execution mode selector', (tester) async {
    final mockConnection = MockConnectionService();

    await tester.pumpWidget(
      MaterialApp(
        home: NewWorkScreen(connection: mockConnection),
      ),
    );
    await tester.pumpAndSettle();

    // Verify Workspace section
    expect(find.text('WORKSPACE DIRECTORY'), findsOneWidget);
    expect(find.text('Select Dir'), findsOneWidget);
    expect(find.text('AgentDesk'), findsOneWidget);
    expect(find.text('myapp'), findsOneWidget);

    // Verify Execution Mode section
    expect(find.text('EXECUTION MODE'), findsOneWidget);
    expect(find.text('Default'), findsOneWidget);
    expect(find.text('--dangerously-skip-permissions'), findsOneWidget);

    // Verify default mode description
    expect(find.textContaining('Safe Default'), findsOneWidget);

    // Tap --dangerously-skip-permissions mode
    await tester.tap(find.text('--dangerously-skip-permissions'));
    await tester.pumpAndSettle();

    // Verify auto-approve description is shown
    expect(find.textContaining('Runs with --dangerously-skip-permissions'), findsOneWidget);

    // Enter prompt
    await tester.enterText(find.byType(TextField).at(1), 'Implement feature X');
    await tester.pumpAndSettle();

    // Scroll to and tap Launch Task button
    final launchBtn = find.text('Launch Task on Laptop');
    await tester.scrollUntilVisible(launchBtn, 200, scrollable: find.byType(Scrollable).first);
    await tester.pumpAndSettle();
    await tester.tap(launchBtn);
    await tester.pumpAndSettle();

    // Verify startTask was called with correct arguments
    expect(mockConnection.lastSubmittedPrompt, 'Implement feature X');
    expect(mockConnection.lastSubmittedWorkspace, '/media/pirate/Shared/currently working/AgentDesk');
    expect(mockConnection.lastSubmittedMode, 'dangerously-skip-permissions');
  });
}
