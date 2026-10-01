import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

Future<void> tap(WidgetTester tester, String label) async {
  await tester.pumpAndSettle();
  await tester.ensureVisible(find.text(label));
  await tester.pumpAndSettle();
  await tester.tap(find.text(label));
  await tester.pumpAndSettle();
}

void main() {
  testWidgets(
      'one code reviews identity but only connects after confirmed storage',
      (tester) async {
    var saves = 0;
    final connected = <String>[];
    final code = DirectConnectionCode('127.0.0.1:21118', 'hdobby1:c3ludGhldGlj')
        .encode();
    tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, (call) async {
      if (call.method == 'Clipboard.getData') return {'text': code};
      return null;
    });
    addTearDown(() => tester.binding.defaultBinaryMessenger
        .setMockMethodCallHandler(SystemChannels.platform, null));
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: HdobbyDirectPairing(
      canHost: false,
      languageCode: 'ko',
      onPrepare: () async => '',
      onInspect: (_) async => 'LOCAL TEST FINGERPRINT',
      onReadPrevious: (_) async => '',
      onSave: (peer, certificate, previous) async {
        expect(peer, '127.0.0.1:21118');
        expect(certificate, 'hdobby1:c3ludGhldGlj');
        saves++;
        return 'LOCAL TEST FINGERPRINT';
      },
      onConnect: (peer) async {
        connected.add(peer);
      },
      onClose: () {},
    ))));
    await tap(tester, '연결 코드 붙여넣기');
    expect(find.text('LOCAL TEST FINGERPRINT'), findsOneWidget);
    expect(saves, 0);
    expect(connected, isEmpty);
    await tap(tester, '호스트 화면의 지문과 일치함을 확인했습니다.');
    await tap(tester, '확인하고 연결');
    expect(saves, 1);
    expect(connected, ['127.0.0.1:21118']);
    expect(
        tester
            .widget<FilledButton>(find.widgetWithText(FilledButton, '확인하고 연결'))
            .onPressed,
        isNull);
  });

  testWidgets(
      'host checks remain distinct and permission action rechecks actual status',
      (tester) async {
    var prepares = 0;
    var allowed = false;
    var stopped = false;
    var checks = 0;
    DirectHostDetails details() => DirectHostDetails(
        certificate: 'hdobby1:c3ludGhldGlj',
        endpoints: ['127.0.0.1:21118'],
        password: 'test-only',
        checks: [
          DirectHostCheck(
              'screen',
              '화면 기록',
              allowed ? '허용됨' : '승인 필요',
              allowed
                  ? DirectCheckState.ready
                  : DirectCheckState.actionRequired,
              action: allowed ? null : '권한 요청'),
          const DirectHostCheck(
              'firewall', '방화벽', '다른 기기 확인 필요', DirectCheckState.unverified),
        ]);
    await tester.pumpWidget(MaterialApp(
        home: Scaffold(
            body: HdobbyDirectPairing(
      canHost: true,
      languageCode: 'ko',
      onPrepare: () async => '',
      onPrepareHost: () async {
        prepares++;
        return details();
      },
      onCheckHost: (_) async {
        checks++;
        return details();
      },
      onResolveHost: (id) async {
        expect(id, 'screen');
        allowed = true;
      },
      onStopHost: () async {
        stopped = true;
      },
      onInspect: (_) async => 'LOCAL TEST FINGERPRINT',
      onReadPrevious: (_) async => '',
      onSave: (_, __, ___) async => '',
      onClose: () {},
    ))));
    expect(prepares, 0);
    await tap(tester, '이 기기 연결 준비');
    expect(prepares, 1);
    expect(find.text('승인 필요'), findsOneWidget);
    expect(find.textContaining('test-only'), findsNothing);
    await tap(tester, '권한 요청');
    expect(find.text('허용됨'), findsOneWidget);
    expect(find.text('다른 기기 확인 필요'), findsNothing);
    await tap(tester, '연결이 안 될 때');
    expect(find.text('다른 기기 확인 필요'), findsOneWidget);
    await tap(tester, '문제 해결 닫기');
    expect(find.text('다른 기기 확인 필요'), findsNothing);
    final previousChecks = checks;
    allowed = false;
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.inactive);
    tester.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await tester.pumpAndSettle();
    expect(checks, previousChecks + 1);
    expect(find.text('승인 필요'), findsOneWidget);
    await tap(tester, '연결 받기 중지');
    expect(stopped, isTrue);
  });
}
