import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

void main() {
  Widget app(bool Function() read, Future<void> Function(bool) write) =>
      MaterialApp(
          home: Scaffold(body: HdobbyRdpHostSetting(read: read, write: write)));

  testWidgets('host setting persists both enabled and disabled states',
      (tester) async {
    var value = false;
    await tester.pumpWidget(app(() => value, (v) async {
      value = v;
    }));
    await tester.tap(find.byType(Switch));
    await tester.pumpAndSettle();
    expect(value, true);
    expect(tester.widget<Switch>(find.byType(Switch)).value, true);
    await tester.tap(find.byType(Switch));
    await tester.pumpAndSettle();
    expect(value, false);
  });

  testWidgets('cancelled elevation does not report success', (tester) async {
    await tester.pumpWidget(app(() => false, (_) async {}));
    await tester.tap(find.byType(Switch));
    await tester.pumpAndSettle();
    expect(tester.widget<Switch>(find.byType(Switch)).value, false);
    expect(find.textContaining('Setting was not applied'), findsOneWidget);
  });

  testWidgets('pending save is locked and disposal is safe', (tester) async {
    final wait = Completer<void>();
    await tester.pumpWidget(app(() => false, (_) => wait.future));
    await tester.tap(find.byType(Switch));
    await tester.pump();
    expect(tester.widget<Switch>(find.byType(Switch)).onChanged, isNull);
    await tester.pumpWidget(const SizedBox.shrink());
    wait.complete();
    await tester.pump();
    expect(tester.takeException(), isNull);
  });
}
