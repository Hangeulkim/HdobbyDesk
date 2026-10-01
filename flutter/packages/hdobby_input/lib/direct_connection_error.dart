/// A retry keeps the same endpoint and certificate; it never changes trust.
bool directConnectionCanRetry(String error) =>
    isPeerTransportReset(error) ||
    const {
      'Direct TLS connection closed during handshake',
      'Direct TLS connection failed during handshake',
      'Direct TCP connection timed out',
      'Direct TLS handshake timed out',
    }.contains(error);

/// Exact transport closures seen when Windows switches between Console and RDP.
/// Authentication, certificate and protocol failures must never match.
bool isPeerTransportReset(String error) =>
    error == 'Reset by the peer' ||
    error == 'Connection reset by peer (os error 104)' ||
    error == 'Connection reset by peer (os error 10054)';

/// A fresh direct connection may race the host listener coming online. Give
/// only known transport failures two brief retries before showing an error.
/// Once authenticated, a peer reset uses the normal finite reconnect budget.
bool shouldRetryDirectConnectionSilently(String error,
    {required bool authenticated, required int attempt}) {
  if (authenticated) return isPeerTransportReset(error);
  return attempt <= 2 && directConnectionCanRetry(error);
}

/// A failed first connection should not use the long recovery window meant for
/// an established session. [attempts] counts retries already scheduled.
bool shouldAutoRetryDirectConnection(String error,
    {required bool authenticated, required int attempts}) {
  if (!directConnectionCanRetry(error)) return false;
  return authenticated || attempts < 2;
}

/// Maps only known native error categories; raw diagnostic data is not shown.
/// Null leaves unrelated application messages to the existing translator.
String? directConnectionErrorMessage(String error,
    {required String languageCode}) {
  final korean = languageCode.split(RegExp('[-_]')).first == 'ko';
  String text(String ko, String en) => korean ? ko : en;
  if (error
      .startsWith('peer closed connection without sending TLS close_notify')) {
    return text(
        '상대 기기에서 보안 연결이 갑자기 종료됐습니다. Windows 화면을 전환하는 중이었다면 호스트 앱의 실행 허용 창이나 보안 프로그램 알림을 확인한 뒤 다시 연결하세요.',
        'The host closed the secure connection unexpectedly. If this happened while switching Windows desktops, check for a host app launch or security prompt, then reconnect.');
  }
  switch (error) {
    case 'Direct TLS connection closed during handshake':
      return text(
          '보안 연결을 준비하던 중 연결이 끊겼습니다. 다시 시도하고, 반복되면 상대 기기의 수신 상태와 네트워크·VPN 경로를 확인하세요.',
          'The connection closed during secure setup. Retry, then check the host listener and network or VPN path if it continues.');
    case 'Direct TCP connection timed out':
    case 'Direct TLS handshake timed out':
      return text(
          '상대 기기의 연결 응답을 기다리는 시간이 초과됐습니다. 주소와 수신 상태, 네트워크 경로를 확인한 뒤 다시 시도하세요.',
          'The connection timed out. Check the address, host listener and network path, then retry.');
    case 'Direct TLS certificate expired':
      return text(
          '상대 기기의 인증서가 만료되었습니다. 양쪽 기기의 날짜를 확인하고, 호스트에서 인증서를 갱신한 뒤 지문을 다시 확인해 주세요.',
          'The host certificate has expired. Check both device clocks; renew the host certificate and verify its fingerprint before pairing again.');
    case 'Direct TLS certificate not yet valid':
      return text('상대 기기의 인증서가 아직 유효하지 않습니다. 양쪽 기기의 날짜와 시간을 확인해 주세요.',
          'The host certificate is not valid yet. Check the date and time on both devices.');
    case 'Direct TLS certificate rejected':
      return text(
          '등록한 인증서로 상대 기기를 확인하지 못했습니다. 주소와 호스트에 표시된 지문을 대조해 주세요. 확인하기 전에는 새 인증서를 신뢰하지 마세요.',
          'The host certificate could not be verified. Check the address and compare the fingerprint on the host before trusting a replacement.');
    case 'Direct TLS protocol negotiation failed':
    case 'Direct TLS peer or protocol does not match the pairing':
      return text('상대 기기의 보안 연결 정보를 확인하지 못했습니다. 주소와 양쪽 앱 버전, 등록한 인증서를 확인해 주세요.',
          'The secure connection details did not match. Check the address, both app versions and the paired certificate.');
    case 'Direct TLS connection failed during handshake':
    // Older native builds used this for both certificate and transport errors.
    case 'Direct TLS host authentication failed; no plaintext fallback':
      return text('보안 연결을 완료하지 못했습니다. 상대 기기의 수신 상태와 네트워크 경로, 등록한 인증서를 확인해 주세요.',
          'Secure setup did not complete. Check the host listener, network path and paired certificate.');
    default:
      return null;
  }
}
