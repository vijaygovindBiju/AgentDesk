import 'package:agentdesk/main.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('home shows the four tiers in fixed order', (tester) async {
    await tester.pumpWidget(const AgentDeskApp());

    final tiles = tester.widgetList<ListTile>(find.byType(ListTile)).toList();
    final titles = tiles.map((t) => (t.title as Text).data).toList();
    expect(titles, ['Requests', 'Errors', 'Completed', 'Working']);
  });
}
