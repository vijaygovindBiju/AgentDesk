import 'package:flutter/material.dart';

class BrandLogo extends StatelessWidget {
  final double size;
  final bool showWordmark;

  const BrandLogo({super.key, this.size = 56, this.showWordmark = false});

  @override
  Widget build(BuildContext context) {
    final mark = Image.asset(
      'assets/branding/agentdesk_logo.png',
      width: size,
      height: size,
      semanticLabel: 'AgentDesk logo',
    );
    if (!showWordmark) return mark;
    final isCompact = size <= 28;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        mark,
        SizedBox(width: isCompact ? 6 : 12),
        Text(
          'AgentDesk',
          style: (isCompact
                  ? Theme.of(context).textTheme.titleMedium
                  : Theme.of(context).textTheme.titleLarge)
              ?.copyWith(
            fontWeight: FontWeight.w800,
            letterSpacing: -0.4,
          ),
        ),
      ],
    );
  }
}
