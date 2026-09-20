import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:agentdesk/services/connection_service.dart';
import 'package:agentdesk/services/onboarding_store.dart';
import 'package:agentdesk/state/agentdesk_state.dart';
import 'package:agentdesk/ui/brand_logo.dart';
import 'package:agentdesk/ui/onboarding_screen.dart';
import 'package:agentdesk/ui/settings_screen.dart';

class MemoryOnboardingStore implements OnboardingStore {
  bool complete = false;

  @override
  Future<bool> isComplete() async => complete;

  @override
  Future<void> markComplete() async => complete = true;

  @override
  Future<void> reset() async => complete = false;
}

void main() {
  testWidgets('first launch shows welcome and navigates through onboarding', (
    tester,
  ) async {
    var completed = false;
    await tester.pumpWidget(
      MaterialApp(
        home: OnboardingScreen(onComplete: () async => completed = true),
      ),
    );

    expect(find.text('Your coding agents, wherever you are.'), findsOneWidget);
    expect(find.text('AgentDesk'), findsOneWidget);
    expect(find.byKey(const Key('onboarding_primary_button')), findsOneWidget);

    for (var page = 0; page < 4; page++) {
      await tester.tap(find.byKey(const Key('onboarding_primary_button')));
      await tester.pumpAndSettle();
    }
    expect(find.text('You are ready.'), findsOneWidget);
    await tester.tap(find.byKey(const Key('onboarding_primary_button')));
    await tester.pumpAndSettle();
    expect(completed, isTrue);
  });

  testWidgets('onboarding supports skip and renders logo asset', (
    tester,
  ) async {
    var completed = false;
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData.light(),
        home: OnboardingScreen(onComplete: () async => completed = true),
      ),
    );

    expect(find.byType(BrandLogo), findsOneWidget);
    expect(find.byType(Image), findsOneWidget);
    await tester.tap(find.text('Skip'));
    await tester.pumpAndSettle();
    expect(completed, isTrue);
  });

  testWidgets('onboarding remains readable in dark theme', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData.dark(),
        home: OnboardingScreen(onComplete: () async {}),
      ),
    );
    expect(find.text('Your coding agents, wherever you are.'), findsOneWidget);
    expect(find.byType(LinearProgressIndicator), findsOneWidget);
  });

  test(
    'onboarding completion persists through the existing preferences store',
    () async {
      SharedPreferences.setMockInitialValues({});
      final store = SharedPreferencesOnboardingStore();
      expect(await store.isComplete(), isFalse);
      await store.markComplete();
      expect(await store.isComplete(), isTrue);
      await store.reset();
      expect(await store.isComplete(), isFalse);
    },
  );

  testWidgets('Settings exposes Show Introduction Again', (tester) async {
    var requested = false;
    await tester.pumpWidget(
      MaterialApp(
        home: SettingsScreen(
          connection: ConnectionService(state: AgentDeskState()),
          onShowIntroduction: () => requested = true,
        ),
      ),
    );

    await tester.drag(find.byType(ListView), const Offset(0, -500));
    await tester.pump();
    await tester.tap(find.byKey(const Key('show_introduction_button')));
    expect(requested, isTrue);
  });
}
