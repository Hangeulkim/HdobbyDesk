import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

void main() {
  test('manual retry is limited to known transport failures', () {
    expect(
        directConnectionCanRetry(
            'Direct TLS connection closed during handshake'),
        isTrue);
    expect(directConnectionCanRetry('Direct TLS handshake timed out'), isTrue);
    expect(directConnectionCanRetry('Connection reset by peer (os error 104)'),
        isTrue);
    expect(directConnectionCanRetry('Reset by the peer'), isTrue);
    for (final error in [
      'Direct TLS certificate expired',
      'Direct TLS certificate rejected',
      'Direct TLS protocol negotiation failed',
      'Direct TLS host authentication failed; no plaintext fallback',
      'Wrong password'
    ]) {
      expect(directConnectionCanRetry(error), isFalse);
    }
  });

  test('only exact peer transport resets qualify for session recovery', () {
    expect(isPeerTransportReset('Connection reset by peer (os error 104)'),
        isTrue);
    expect(isPeerTransportReset('Connection reset by peer (os error 10054)'),
        isTrue);
    expect(isPeerTransportReset('Reset by the peer'), isTrue);
    expect(isPeerTransportReset('Connection reset by peer (os error 999)'),
        isFalse);
    expect(isPeerTransportReset('Direct TLS certificate rejected'), isFalse);
    expect(isPeerTransportReset('Wrong password'), isFalse);
  });

  test('startup retries stop showing a transient error only twice', () {
    const closed = 'Direct TLS connection closed during handshake';
    expect(
        shouldRetryDirectConnectionSilently(closed,
            authenticated: false, attempt: 1),
        isTrue);
    expect(
        shouldRetryDirectConnectionSilently(closed,
            authenticated: false, attempt: 2),
        isTrue);
    expect(
        shouldRetryDirectConnectionSilently(closed,
            authenticated: false, attempt: 3),
        isFalse);
    expect(
        shouldRetryDirectConnectionSilently('Direct TLS certificate rejected',
            authenticated: false, attempt: 1),
        isFalse);
    expect(
        shouldRetryDirectConnectionSilently('Wrong password',
            authenticated: false, attempt: 1),
        isFalse);
    expect(
        shouldRetryDirectConnectionSilently(
            'Connection reset by peer (os error 104)',
            authenticated: true,
            attempt: 3),
        isTrue);
  });

  test('a cold direct connection stops after two transport retries', () {
    const timedOut = 'Direct TCP connection timed out';
    expect(
        shouldAutoRetryDirectConnection(timedOut,
            authenticated: false, attempts: 0),
        isTrue);
    expect(
        shouldAutoRetryDirectConnection(timedOut,
            authenticated: false, attempts: 1),
        isTrue);
    expect(
        shouldAutoRetryDirectConnection(timedOut,
            authenticated: false, attempts: 2),
        isFalse);
    expect(
        shouldAutoRetryDirectConnection(timedOut,
            authenticated: true, attempts: 2),
        isTrue);
    expect(
        shouldAutoRetryDirectConnection('Direct TLS certificate rejected',
            authenticated: false, attempts: 0),
        isFalse);
  });

  test(
      'an interrupted handshake does not tell the user their certificate failed',
      () {
    final text = directConnectionErrorMessage(
        'Direct TLS connection closed during handshake',
        languageCode: 'ko-KR')!;
    expect(text, contains('연결이 끊겼습니다'));
    expect(text, contains('다시 시도'));
    expect(text, isNot(contains('인증서')));
  });

  test('a Windows desktop handoff explains an abrupt secure close', () {
    const raw = 'peer closed connection without sending TLS close_notify: '
        'https://docs.rs/rustls/latest/rustls/manual/_03_howto/index.html#unexpected-eof';
    final message = directConnectionErrorMessage(raw, languageCode: 'ko-KR')!;
    expect(message, contains('Windows 화면을 전환'));
    expect(message, contains('보안 프로그램 알림'));
    expect(message, isNot(contains('docs.rs')));
    expect(directConnectionCanRetry(raw), isFalse);
  });

  test('certificate rejection asks for a fingerprint check before new trust',
      () {
    final text = directConnectionErrorMessage('Direct TLS certificate rejected',
        languageCode: 'ko')!;
    expect(text, contains('지문'));
    expect(text, contains('확인하기 전에는 새 인증서를 신뢰하지 마세요'));
    expect(text, isNot(contains('VPN')));
  });

  test('validity errors distinguish expired certificates from an early clock',
      () {
    final expired = directConnectionErrorMessage(
        'Direct TLS certificate expired',
        languageCode: 'ko')!;
    final early = directConnectionErrorMessage(
        'Direct TLS certificate not yet valid',
        languageCode: 'ko')!;
    expect(expired, contains('만료'));
    expect(early, contains('아직 유효하지'));
    expect(early, contains('날짜와 시간'));
  });

  test('unknown and password messages remain with the existing translator', () {
    for (final message in [
      'Wrong password',
      'Direct TLS unknown detail',
      'other'
    ]) {
      expect(directConnectionErrorMessage(message, languageCode: 'ko'), isNull);
    }
  });

  test('legacy generic failures remain inconclusive and English is available',
      () {
    final legacy = directConnectionErrorMessage(
        'Direct TLS host authentication failed; no plaintext fallback',
        languageCode: 'ko')!;
    expect(legacy, contains('보안 연결을 완료하지 못했습니다'));
    final english = directConnectionErrorMessage(
        'Direct TLS connection closed during handshake',
        languageCode: 'en-US')!;
    expect(english, contains('connection closed'));
    expect(english, isNot(contains('certificate')));
  });
}
