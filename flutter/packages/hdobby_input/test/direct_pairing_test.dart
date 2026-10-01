import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

class LocalPairingHarness {
  int preparations = 0;
  int reads = 0;
  int saves = 0;
  int closes = 0;
  String previous = '';
  bool failSave = false;
  bool mismatchSave = false;
  Completer<String>? prepareWait;
  List<String>? storedArguments;

  Widget app({bool canHost = false}) => MaterialApp(
          home: Scaffold(
              body: HdobbyDirectPairing(
        canHost: canHost,
        languageCode: 'ko',
        onPrepare: () async {
          preparations++;
          return prepareWait == null
              ? 'synthetic-own-code'
              : await prepareWait!.future;
        },
        onInspect: (code) async => code == 'synthetic-old-code'
            ? 'OLD FINGERPRINT'
            : 'NEW FINGERPRINT',
        onReadPrevious: (peer) async {
          reads++;
          return previous;
        },
        onSave: (peer, code, old) async {
          saves++;
          storedArguments = [peer, code, old];
          if (failSave) throw StateError('Local simulated storage failure');
          return mismatchSave ? 'DIFFERENT FINGERPRINT' : 'NEW FINGERPRINT';
        },
        onClose: () => closes++,
      )));
}

Future<void> tapText(WidgetTester tester, String text) async {
  final target = find.text(text);
  await tester.pumpAndSettle();
  await tester.ensureVisible(target);
  await tester.pumpAndSettle();
  await tester.tap(target);
  await tester.pumpAndSettle();
}

Future<void> enterPairing(WidgetTester tester) async {
  await tester.enterText(
      find.byKey(const ValueKey('pair-peer')), '127.0.0.1:21118');
  await tester.enterText(
      find.byKey(const ValueKey('pair-code')), 'synthetic-new-code');
  await tapText(tester, '지문 확인');
}

void main() {
  testWidgets(
      'clipboard import is explicit and invalidates previously verified trust',
      (tester) async {
    var clipboardReads = 0;
    var clipboardText = 'hdobby1:local-synthetic-pairing';
    tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.getData') {
        clipboardReads++;
        return {'text': clipboardText};
      }
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, null));
    final h = LocalPairingHarness();
    await tester.pumpWidget(h.app());
    expect(clipboardReads, 0);
    await enterPairing(tester);
    await tapText(tester, '호스트 화면의 지문과 일치함을 확인했습니다.');
    await tapText(tester, '인증서 코드 붙여넣기');
    expect(clipboardReads, 1);
    expect(
        tester
            .widget<TextField>(find.byKey(const ValueKey('pair-code')))
            .controller!
            .text,
        clipboardText);
    expect(find.widgetWithText(FilledButton, '확인한 기기 등록'), findsNothing);
    expect(h.saves, 0);
    clipboardText = 'unrelated clipboard content';
    await tapText(tester, '인증서 코드 붙여넣기');
    expect(
        tester
            .widget<TextField>(find.byKey(const ValueKey('pair-code')))
            .controller!
            .text,
        'hdobby1:local-synthetic-pairing');
    expect(find.text('클립보드에 인증서 코드가 없습니다. 기존 입력은 유지됩니다.'), findsOneWidget);
    expect(h.saves, 0);
  });

  testWidgets(
      'opening and closing have no generation, storage or connection side effects',
      (tester) async {
    final h = LocalPairingHarness();
    await tester.pumpWidget(h.app(canHost: true));
    expect(h.preparations + h.reads + h.saves, 0);
    await tapText(tester, '닫기');
    expect(h.closes, 1);
    expect(h.preparations + h.reads + h.saves, 0);
  });

  testWidgets('controller-only platform hides host creation', (tester) async {
    await tester.pumpWidget(LocalPairingHarness().app());
    expect(find.text('인증서 준비 / 확인'), findsNothing);
    expect(find.text('상대 기기 등록'), findsOneWidget);
  });

  testWidgets('empty input never reads or saves a peer', (tester) async {
    final h = LocalPairingHarness();
    await tester.pumpWidget(h.app());
    await tapText(tester, '지문 확인');
    expect(h.reads + h.saves, 0);
    expect(find.text('상대 주소와 인증서 코드를 입력하세요.'), findsOneWidget);
  });

  testWidgets('pairing requires review and explicit fingerprint confirmation',
      (tester) async {
    final h = LocalPairingHarness();
    await tester.pumpWidget(h.app());
    await enterPairing(tester);
    final save = find.widgetWithText(FilledButton, '확인한 기기 등록');
    expect(tester.widget<FilledButton>(save).onPressed, isNull);
    expect(h.saves, 0);
    await tapText(tester, '호스트 화면의 지문과 일치함을 확인했습니다.');
    await tapText(tester, '확인한 기기 등록');
    expect(h.saves, 1);
    expect(h.storedArguments, ['127.0.0.1:21118', 'synthetic-new-code', '']);
    expect(tester.widget<FilledButton>(save).onPressed, isNull);
    expect(find.text('등록했습니다. 창을 닫고 같은 주소로 연결하세요.'), findsOneWidget);
  });

  testWidgets('changing either field invalidates the reviewed identity',
      (tester) async {
    final h = LocalPairingHarness();
    await tester.pumpWidget(h.app());
    await enterPairing(tester);
    await tapText(tester, '호스트 화면의 지문과 일치함을 확인했습니다.');
    await tester.enterText(
        find.byKey(const ValueKey('pair-peer')), 'localhost:21118');
    await tester.pump();
    expect(find.text('확인한 기기 등록'), findsNothing);
    expect(h.saves, 0);
  });

  testWidgets(
      'replacement shows old fingerprint and passes previous value to compare on save',
      (tester) async {
    final h = LocalPairingHarness()..previous = 'synthetic-old-code';
    await tester.pumpWidget(h.app());
    await enterPairing(tester);
    expect(find.text('OLD FINGERPRINT'), findsOneWidget);
    expect(find.text('NEW FINGERPRINT'), findsOneWidget);
    await tapText(tester, '호스트 화면의 지문과 일치함을 확인했습니다.');
    await tapText(tester, '확인한 기기로 변경');
    expect(h.storedArguments!.last, 'synthetic-old-code');
  });

  testWidgets(
      'storage failure or mismatched readback preserves draft without success',
      (tester) async {
    for (final mismatch in [false, true]) {
      final h = LocalPairingHarness()
        ..failSave = !mismatch
        ..mismatchSave = mismatch;
      await tester.pumpWidget(h.app());
      await enterPairing(tester);
      await tapText(tester, '호스트 화면의 지문과 일치함을 확인했습니다.');
      await tapText(tester, '확인한 기기 등록');
      expect(find.text('synthetic-new-code'), findsOneWidget);
      expect(find.text('등록했습니다. 창을 닫고 같은 주소로 연결하세요.'), findsNothing);
      expect(find.textContaining('등록 저장을 확인하지 못했습니다.'), findsOneWidget);
      await tester.pumpWidget(const SizedBox());
    }
  });

  testWidgets(
      'host preparation is explicit, bounded, and stale completion does not update UI',
      (tester) async {
    final h = LocalPairingHarness()..prepareWait = Completer<String>();
    await tester.pumpWidget(h.app(canHost: true));
    await tester.ensureVisible(find.text('인증서 준비 / 확인'));
    await tester.tap(find.text('인증서 준비 / 확인'));
    await tester.tap(find.text('인증서 준비 / 확인'));
    await tester.pump();
    expect(h.preparations, 1);
    expect(
        tester
            .widget<TextButton>(find.widgetWithText(TextButton, '닫기'))
            .onPressed,
        isNull);
    await tester.pump(const Duration(seconds: 16));
    await tester.pumpAndSettle();
    h.prepareWait!.complete('late-synthetic-code');
    await tester.pumpAndSettle();
    expect(find.text('late-synthetic-code'), findsNothing);
    expect(find.textContaining('인증서를 준비하지 못했습니다.'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('phone width and enlarged text retain scrollable controls',
      (tester) async {
    tester.view.physicalSize = const Size(320, 640);
    tester.view.devicePixelRatio = 1;
    tester.platformDispatcher.textScaleFactorTestValue = 2;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    final h = LocalPairingHarness();
    await tester.pumpWidget(h.app(canHost: true));
    await tester.ensureVisible(find.byKey(const ValueKey('pair-peer')));
    await tester.enterText(
        find.byKey(const ValueKey('pair-peer')), '127.0.0.1');
    await tester.ensureVisible(find.byKey(const ValueKey('pair-code')));
    await tester.enterText(
        find.byKey(const ValueKey('pair-code')), 'synthetic-new-code');
    await tapText(tester, '지문 확인');
    await tapText(tester, '호스트 화면의 지문과 일치함을 확인했습니다.');
    await tapText(tester, '확인한 기기 등록');
    expect(h.saves, 1);
    expect(tester.takeException(), isNull);
  });
}
