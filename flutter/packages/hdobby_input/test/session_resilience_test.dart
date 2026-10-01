import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

void main() {
  test('mobile sessions stay awake by default and preserve explicit choice',
      () {
    expect(
        keepAwakeForOutgoingSession(isMobile: true, storedValue: ''), isTrue);
    expect(
        keepAwakeForOutgoingSession(isMobile: false, storedValue: ''), isFalse);
    expect(
        keepAwakeForOutgoingSession(isMobile: true, storedValue: 'N'), isFalse);
    expect(
        keepAwakeForOutgoingSession(isMobile: false, storedValue: 'Y'), isTrue);
  });

  test('reconnect delay grows, caps, and stops within its finite budget', () {
    final start = DateTime(2026, 9, 14);
    final backoff = SessionReconnectBackoff();
    final delays = <int>[];
    var now = start;
    while (true) {
      final delay = backoff.nextDelay(now);
      if (delay == null) break;
      delays.add(delay.inSeconds);
      now = now.add(delay);
    }
    expect(delays.take(6), [1, 2, 4, 8, 16, 30]);
    expect(delays.every((seconds) => seconds <= 30), isTrue);
    expect(delays.length, lessThanOrEqualTo(14));
    expect(delays.fold<int>(0, (sum, value) => sum + value),
        lessThanOrEqualTo(300));
  });

  test('reconnect budget resets after a successful session', () {
    final backoff = SessionReconnectBackoff(maximumAttempts: 1);
    final now = DateTime(2026, 9, 14);
    expect(backoff.nextDelay(now), const Duration(seconds: 1));
    expect(backoff.nextDelay(now), isNull);
    backoff.reset();
    expect(backoff.nextDelay(now), const Duration(seconds: 1));
  });
}
