import 'package:flutter/material.dart';
import 'brand_logo.dart';

class OnboardingScreen extends StatefulWidget {
  final Future<void> Function() onComplete;

  const OnboardingScreen({super.key, required this.onComplete});

  @override
  State<OnboardingScreen> createState() => _OnboardingScreenState();
}

class _OnboardingScreenState extends State<OnboardingScreen> {
  final _controller = PageController();
  int _page = 0;
  bool _isSaving = false;

  static const _pages = <_OnboardingPageData>[
    _OnboardingPageData(
      title: 'Your coding agents, wherever you are.',
      body:
          'AgentDesk lets you supervise coding agents from your phone without mirroring the entire terminal.',
      icon: Icons.center_focus_strong,
    ),
    _OnboardingPageData(
      title: 'Work stays on your laptop.',
      body:
          'The agent does the work on your laptop. AgentDesk brings only decisions and attention requests to your phone.',
      icon: Icons.device_hub,
    ),
    _OnboardingPageData(
      title: 'Answer when the agent needs you.',
      body:
          'Approve or deny permissions, choose one or more options, or enter any free-form answer. The real question and options always come from the agent.',
      icon: Icons.touch_app,
    ),
    _OnboardingPageData(
      title: 'Start and supervise work.',
      body:
          'New Work starts a task, Working shows progress, Requests brings you decisions, and Completed keeps the history.',
      icon: Icons.route,
    ),
    _OnboardingPageData(
      title: 'You are ready.',
      body:
          'Start a task from New Work, then let AgentDesk bring you in only when your decision or input is needed.',
      icon: Icons.check_circle_outline,
    ),
  ];

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _finish() async {
    if (_isSaving) return;
    setState(() => _isSaving = true);
    await widget.onComplete();
  }

  void _next() {
    if (_page == _pages.length - 1) {
      _finish();
      return;
    }
    _controller.nextPage(
      duration: const Duration(milliseconds: 260),
      curve: Curves.easeOutCubic,
    );
  }

  @override
  Widget build(BuildContext context) {
    final isLast = _page == _pages.length - 1;
    return Scaffold(
      body: SafeArea(
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 24, 24, 8),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  const BrandLogo(size: 44, showWordmark: true),
                  Text('${_page + 1}/${_pages.length}'),
                ],
              ),
            ),
            Expanded(
              child: PageView.builder(
                controller: _controller,
                itemCount: _pages.length,
                onPageChanged: (page) => setState(() => _page = page),
                itemBuilder: (context, index) {
                  final page = _pages[index];
                  return Padding(
                    padding: const EdgeInsets.fromLTRB(24, 24, 24, 12),
                    child: SingleChildScrollView(
                      child: ConstrainedBox(
                        constraints: BoxConstraints(
                          minHeight: MediaQuery.sizeOf(context).height * .38,
                        ),
                        child: Column(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            Container(
                              width: 144,
                              height: 144,
                              decoration: BoxDecoration(
                                color: Theme.of(
                                  context,
                                ).colorScheme.primary.withValues(alpha: .12),
                                borderRadius: BorderRadius.circular(40),
                              ),
                              child: Icon(
                                page.icon,
                                size: 72,
                                color: Theme.of(context).colorScheme.primary,
                              ),
                            ),
                            const SizedBox(height: 36),
                            Text(
                              page.title,
                              textAlign: TextAlign.center,
                              style: Theme.of(context).textTheme.headlineSmall
                                  ?.copyWith(fontWeight: FontWeight.w800),
                            ),
                            const SizedBox(height: 16),
                            Text(
                              page.body,
                              textAlign: TextAlign.center,
                              style: Theme.of(
                                context,
                              ).textTheme.bodyLarge?.copyWith(height: 1.5),
                            ),
                          ],
                        ),
                      ),
                    ),
                  );
                },
              ),
            ),
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 8, 24, 24),
              child: Column(
                children: [
                  LinearProgressIndicator(
                    value: (_page + 1) / _pages.length,
                    minHeight: 6,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  const SizedBox(height: 20),
                  SizedBox(
                    width: double.infinity,
                    child: FilledButton(
                      key: const Key('onboarding_primary_button'),
                      onPressed: _isSaving ? null : _next,
                      child: Text(
                        isLast
                            ? 'Start Using AgentDesk'
                            : _page == 0
                            ? 'Get Started'
                            : 'Next',
                      ),
                    ),
                  ),
                  if (!isLast)
                    TextButton(
                      onPressed: _isSaving ? null : _finish,
                      child: const Text('Skip'),
                    ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _OnboardingPageData {
  final String title;
  final String body;
  final IconData icon;

  const _OnboardingPageData({
    required this.title,
    required this.body,
    required this.icon,
  });
}
