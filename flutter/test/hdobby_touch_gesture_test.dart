import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/common/widgets/gestures.dart';

void main() {
  testWidgets('single touch tap reaches remote click beside multi-touch gestures',
      (tester) async {
    var taps = 0;
    const target = Key('remote-image');
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: RawGestureDetector(
          gestures: <Type, GestureRecognizerFactory>{
            TapGestureRecognizer:
                GestureRecognizerFactoryWithHandlers<TapGestureRecognizer>(
              () => TapGestureRecognizer(),
              (instance) => instance.onTapUp = (_) => taps++,
            ),
            HoldTapMoveGestureRecognizer: GestureRecognizerFactoryWithHandlers<
                HoldTapMoveGestureRecognizer>(
              () => HoldTapMoveGestureRecognizer(),
              (instance) => instance.onHoldDragStart = (_) {},
            ),
            DoubleFinerTapGestureRecognizer: GestureRecognizerFactoryWithHandlers<
                DoubleFinerTapGestureRecognizer>(
              () => DoubleFinerTapGestureRecognizer(),
              (instance) => instance.onDoubleFinerTap = (_) {},
            ),
            CustomTouchGestureRecognizer: GestureRecognizerFactoryWithHandlers<
                CustomTouchGestureRecognizer>(
              () => CustomTouchGestureRecognizer(),
              (instance) => instance.onOneFingerPanStart = (_) {},
            ),
          },
          child: const SizedBox.expand(
            child: ColoredBox(key: target, color: Colors.black),
          ),
        ),
      ),
    ));

    await tester.tap(find.byKey(target));
    await tester.pump(const Duration(milliseconds: 500));
    expect(taps, 1);
  });

  testWidgets('nested remote gesture regions still deliver one touch click',
      (tester) async {
    var taps = 0;
    const target = Key('nested-remote-image');
    Map<Type, GestureRecognizerFactory> gestures() => {
          TapGestureRecognizer:
              GestureRecognizerFactoryWithHandlers<TapGestureRecognizer>(
            () => TapGestureRecognizer(),
            (instance) => instance.onTapUp = (_) => taps++,
          ),
          HoldTapMoveGestureRecognizer: GestureRecognizerFactoryWithHandlers<
              HoldTapMoveGestureRecognizer>(
            () => HoldTapMoveGestureRecognizer(),
            (instance) => instance.onHoldDragStart = (_) {},
          ),
          DoubleFinerTapGestureRecognizer: GestureRecognizerFactoryWithHandlers<
              DoubleFinerTapGestureRecognizer>(
            () => DoubleFinerTapGestureRecognizer(),
            (instance) => instance.onDoubleFinerTap = (_) {},
          ),
          CustomTouchGestureRecognizer: GestureRecognizerFactoryWithHandlers<
              CustomTouchGestureRecognizer>(
            () => CustomTouchGestureRecognizer(),
            (instance) => instance.onOneFingerPanStart = (_) {},
          ),
        };
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: RawGestureDetector(
          gestures: gestures(),
          child: RawGestureDetector(
            gestures: gestures(),
            child: const SizedBox.expand(
              child: ColoredBox(key: target, color: Colors.black),
            ),
          ),
        ),
      ),
    ));

    await tester.tap(find.byKey(target));
    await tester.pump(const Duration(milliseconds: 500));
    expect(taps, 1);
  });

  testWidgets('duplicate simultaneous touch pointers deliver one click',
      (tester) async {
    var taps = 0;
    const target = Key('duplicate-touch-image');
    await tester.pumpWidget(MaterialApp(
      home: Scaffold(
        body: RawGestureDetector(
          gestures: <Type, GestureRecognizerFactory>{
            TapGestureRecognizer:
                GestureRecognizerFactoryWithHandlers<TapGestureRecognizer>(
              () => TapGestureRecognizer(),
              (instance) => instance.onTapUp = (_) => taps++,
            ),
            HoldTapMoveGestureRecognizer: GestureRecognizerFactoryWithHandlers<
                HoldTapMoveGestureRecognizer>(
              () => HoldTapMoveGestureRecognizer(),
              (instance) => instance.onHoldDragStart = (_) {},
            ),
            DoubleFinerTapGestureRecognizer: GestureRecognizerFactoryWithHandlers<
                DoubleFinerTapGestureRecognizer>(
              () => DoubleFinerTapGestureRecognizer(),
              (instance) => instance.onDoubleFinerTap = (_) {},
            ),
            CustomTouchGestureRecognizer: GestureRecognizerFactoryWithHandlers<
                CustomTouchGestureRecognizer>(
              () => CustomTouchGestureRecognizer(),
              (instance) => instance.onOneFingerPanStart = (_) {},
            ),
          },
          child: const SizedBox.expand(
            child: ColoredBox(key: target, color: Colors.black),
          ),
        ),
      ),
    ));

    final point = tester.getCenter(find.byKey(target));
    final first = await tester.startGesture(point, pointer: 4);
    final duplicate = await tester.startGesture(point, pointer: 104);
    await first.up();
    await duplicate.up();
    await tester.pump(const Duration(milliseconds: 500));
    expect(taps, 1);
  });
}
