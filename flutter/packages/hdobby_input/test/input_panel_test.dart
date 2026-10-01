import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

void main() {
  Future<void> mount(
    WidgetTester tester, {
    bool enabled = true,
    bool textEnabled = true,
    bool mac = false,
    bool android = false,
    double scale = 1,
    Future<void> Function(String)? onText,
    Future<void> Function(InputAction)? onAction,
    VoidCallback? onClose,
  }) async {
    await tester.pumpWidget(MaterialApp(
        home: MediaQuery(
      data: MediaQueryData(
          size: const Size(800, 900), textScaler: TextScaler.linear(scale)),
      child: Scaffold(
          body: HdobbyInputPanel(
        peerLabel: 'Test host',
        enabled: enabled,
        textEnabled: textEnabled,
        macPeer: mac,
        androidPeer: android,
        windowsPeer: !mac && !android,
        onText: onText ?? (_) async {},
        onAction: onAction ?? (_) async {},
        onClose: onClose ?? () {},
      )),
    )));
    await tester.pumpAndSettle();
  }

  final draft = find.byKey(const ValueKey('text-draft'));
  final send = find.byKey(const ValueKey('send-text'));

  testWidgets('arrow controls announce their direction once', (tester) async {
    final semantics = tester.ensureSemantics();
    try {
      await mount(tester);
      final arrow = find.byKey(const ValueKey('left'));
      await tester.ensureVisible(arrow);
      await tester.pumpAndSettle();
      expect(tester.getSemantics(arrow).label, 'Left');
    } finally {
      semantics.dispose();
    }
  });

  test('remote language keys are mapped by target, not controller OS', () {
    final win = remoteImeKey(RemoteImeTarget.windows);
    expect(win.name, 'VK_HANGUL');
    expect(win.ctrl, isFalse);
    final mac = remoteImeKey(RemoteImeTarget.macOS);
    expect(mac.name, 'VK_SPACE');
    expect(mac.ctrl, isTrue);
  });

  testWidgets('remote language button sends one explicit action',
      (tester) async {
    final actions = <InputAction>[];
    await mount(tester, onAction: (action) async => actions.add(action));
    await tester.tap(find.byKey(const ValueKey('switch-remote-input')));
    await tester.pumpAndSettle();
    expect(actions, [InputAction.switchInput]);
  });

  testWidgets(
      'Korean IME edits stay local; one explicit send preserves Unicode',
      (tester) async {
    final sent = <String>[];
    await mount(tester, onText: (text) async => sent.add(text));
    await tester.showKeyboard(draft);
    for (final text in ['ㅎ', '하', '한', '한글 👩🏽‍💻\n  next  ']) {
      tester.testTextInput.updateEditingValue(TextEditingValue(
          text: text,
          selection: TextSelection.collapsed(offset: text.length),
          composing: TextRange(start: 0, end: text.length)));
      await tester.pump();
    }
    expect(sent, isEmpty);
    await tester.ensureVisible(send);
    await tester.tap(send);
    await tester.pumpAndSettle();
    expect(sent, ['한글 👩🏽‍💻\n  next  ']);
    expect(tester.widget<TextField>(draft).controller!.text, isEmpty);
  });

  testWidgets('rapid taps while sending cannot duplicate the request',
      (tester) async {
    final pending = Completer<void>();
    var count = 0;
    await mount(tester, onText: (_) {
      count++;
      return pending.future;
    });
    await tester.enterText(draft, 'hello');
    await tester.ensureVisible(send);
    await tester.tap(send);
    await tester.pump();
    await tester.tap(send);
    expect(count, 1);
    pending.complete();
    await tester.pumpAndSettle();
  });

  testWidgets('failure preserves draft and does not expose exception payload',
      (tester) async {
    var attempts = 0;
    await mount(tester, onText: (_) async {
      attempts++;
      throw StateError('private-payload');
    });
    await tester.enterText(draft, 'retry me');
    await tester.pumpAndSettle();
    await tester.ensureVisible(send);
    await tester.pumpAndSettle();
    await tester.tap(send);
    await tester.pumpAndSettle();
    expect(attempts, 1);
    expect(tester.widget<TextField>(draft).controller!.text, 'retry me');
    expect(find.byKey(const ValueKey('send-failed')), findsOneWidget);
    expect(find.textContaining('private-payload'), findsNothing);
    expect(find.byKey(const ValueKey('input-requested')), findsNothing);
  });

  testWidgets('view only disables actions but preserves local editing',
      (tester) async {
    await mount(tester, enabled: false);
    await tester.enterText(draft, 'keep locally');
    expect(tester.widget<FilledButton>(send).onPressed, isNull);
    expect(
        tester
            .widget<OutlinedButton>(find.byKey(const ValueKey('enter')))
            .onPressed,
        isNull);
    expect(tester.widget<TextField>(draft).controller!.text, 'keep locally');
  });

  testWidgets('permission revocation disables send without losing draft',
      (tester) async {
    await mount(tester);
    await tester.enterText(draft, 'unsent');
    await mount(tester, enabled: false);
    expect(tester.widget<FilledButton>(send).onPressed, isNull);
    expect(tester.widget<TextField>(draft).controller!.text, 'unsent');
  });

  testWidgets('text gate disables text without disabling key actions',
      (tester) async {
    await mount(tester, textEnabled: false);
    await tester.enterText(draft, 'gated');
    expect(tester.widget<FilledButton>(send).onPressed, isNull);
    expect(
        tester
            .widget<OutlinedButton>(find.byKey(const ValueKey('enter')))
            .onPressed,
        isNotNull);
  });

  testWidgets('Mac shortcuts use Cmd; other desktops use Ctrl', (tester) async {
    await mount(tester, mac: true);
    expect(find.text('Cmd+C'), findsOneWidget);
    expect(find.text('Ctrl+C'), findsNothing);
    await mount(tester, mac: false);
    expect(find.text('Ctrl+C'), findsOneWidget);
  });

  testWidgets('Android does not show desktop mouse or clipboard shortcuts',
      (tester) async {
    await mount(tester, android: true);
    expect(find.byKey(const ValueKey('leftClick')), findsNothing);
    expect(find.byKey(const ValueKey('copy')), findsNothing);
    expect(find.byKey(const ValueKey('enter')), findsOneWidget);
  });

  testWidgets('explicit click button dispatches only the selected action',
      (tester) async {
    final actions = <InputAction>[];
    await mount(tester, onAction: (action) async => actions.add(action));
    final button = find.byKey(const ValueKey('rightClick'));
    await tester.ensureVisible(button);
    await tester.tap(button);
    await tester.pumpAndSettle();
    expect(actions, [InputAction.rightClick]);
  });

  testWidgets('closing never sends a draft', (tester) async {
    var count = 0;
    var closed = false;
    await mount(tester, onText: (_) async {
      count++;
    }, onClose: () {
      closed = true;
    });
    await tester.enterText(draft, 'do not send');
    final close = find.byKey(const ValueKey('close-input-helper'));
    await tester.ensureVisible(close);
    await tester.tap(close);
    expect(closed, isTrue);
    expect(count, 0);
  });

  testWidgets('completing after disposal never updates disposed state',
      (tester) async {
    final pending = Completer<void>();
    await mount(tester, onText: (_) => pending.future);
    await tester.enterText(draft, 'test');
    await tester.ensureVisible(send);
    await tester.tap(send);
    await tester.pump();
    await tester.pumpWidget(const SizedBox());
    pending.complete();
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('320px viewport and 200% text can scroll to all controls',
      (tester) async {
    tester.view.physicalSize = const Size(320, 568);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    await mount(tester, scale: 2);
    final save = find.byKey(const ValueKey('save'));
    await tester.ensureVisible(save);
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
    expect(tester.getSize(save).height, greaterThanOrEqualTo(48));
  });
}
