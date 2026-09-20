import 'package:shared_preferences/shared_preferences.dart';

abstract interface class OnboardingStore {
  Future<bool> isComplete();
  Future<void> markComplete();
  Future<void> reset();
}

class SharedPreferencesOnboardingStore implements OnboardingStore {
  static const _completedKey = 'agentdesk.onboarding.completed';

  @override
  Future<bool> isComplete() async {
    final preferences = await SharedPreferences.getInstance();
    return preferences.getBool(_completedKey) ?? false;
  }

  @override
  Future<void> markComplete() async {
    final preferences = await SharedPreferences.getInstance();
    await preferences.setBool(_completedKey, true);
  }

  @override
  Future<void> reset() async {
    final preferences = await SharedPreferences.getInstance();
    await preferences.remove(_completedKey);
  }
}
