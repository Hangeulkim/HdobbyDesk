import 'dart:convert';
import 'dart:io';

import 'package:hdobby_input/hdobby_input.dart';
import 'package:url_launcher/url_launcher.dart';

import '../common.dart';
import '../consts.dart';
import '../models/platform_model.dart';
import '../models/server_model.dart';

class HdobbyHostSetup {
  HdobbyHostSetup(this.korean);
  final bool korean;
  String t(String ko, String en) => korean ? ko : en;

  Future<DirectHostDetails> prepare() async {
    final certificate = await bind.mainPrepareDirectTlsIdentity();
    final wasStopped = option2bool(
        kOptionStopService, await bind.mainGetOption(key: kOptionStopService));
    final previousDirect = await bind.mainGetOption(key: kOptionDirectServer);
    // Identity is persisted before incoming connections are enabled.
    try {
      await mainSetBoolOption(kOptionDirectServer, true);
      if (isAndroid) {
        if (!gFFI.serverModel.isStart) await gFFI.serverModel.toggleService();
      } else {
        await start_service(true);
        if (isMacOS) {
          if (!bind.mainIsCanScreenRecording(prompt: false)) {
            bind.mainIsCanScreenRecording(prompt: true);
          } else if (!bind.mainIsProcessTrusted(prompt: false)) {
            bind.mainIsProcessTrusted(prompt: true);
          }
        }
      }
      return await check(certificate);
    } catch (_) {
      // A setup error must not leave a newly enabled listener hidden behind an error dialog.
      if (wasStopped) await stop();
      await bind.mainSetOption(key: kOptionDirectServer, value: previousDirect);
      rethrow;
    }
  }

  Future<List<String>> _endpoints() async {
    final rawPort = await bind.mainGetOption(key: 'direct-access-port');
    final port = rawPort.isEmpty ? 21118 : int.tryParse(rawPort);
    if (port == null || port < 1 || port > 65535) return [];
    final selected = await bind.mainGetOption(key: 'direct-listen-ip');
    final addresses = <String>[];
    if (selected.isNotEmpty && selected != '0.0.0.0' && selected != '::') {
      if (InternetAddress.tryParse(selected) == null) return [];
      addresses.add(selected);
    } else {
      final interfaces = await NetworkInterface.list(includeLoopback: false);
      for (final interface in interfaces) {
        for (final address in interface.addresses) {
          if (address.isLinkLocal || address.address.contains('%')) continue;
          if (!addresses.contains(address.address)) {
            addresses.add(address.address);
          }
        }
      }
      addresses.sort((a, b) =>
          (a.contains(':') ? 1 : 0).compareTo(b.contains(':') ? 1 : 0));
    }
    return addresses
        .map((ip) => ip.contains(':') ? '[$ip]:$port' : '$ip:$port')
        .toList();
  }

  Future<bool> _probe(String endpoint, String code) async {
    Socket? socket;
    SecureSocket? tls;
    try {
      final uri = DirectConnectionCode.parseEndpoint(endpoint);
      final expected = DirectConnectionCode.certificateBytes(code);
      // PEM is accepted across Dart's platform TLS backends; raw DER is not.
      final encoded = base64Encode(expected);
      final lines = <String>[];
      for (var offset = 0; offset < encoded.length; offset += 64) {
        final end = offset + 64 < encoded.length ? offset + 64 : encoded.length;
        lines.add(encoded.substring(offset, end));
      }
      final pem = '-----BEGIN CERTIFICATE-----\n${lines.join('\n')}\n'
          '-----END CERTIFICATE-----\n';
      final security = SecurityContext(withTrustedRoots: false)
        ..setTrustedCertificatesBytes(utf8.encode(pem));
      socket = await Socket.connect(uri.host, uri.hasPort ? uri.port : 21118,
          timeout: const Duration(seconds: 2));
      tls = await SecureSocket.secure(socket,
              host: 'hdobby.direct',
              context: security,
              supportedProtocols: ['hdobby-direct/1'])
          .timeout(const Duration(seconds: 3));
      final actual = tls.peerCertificate?.der;
      return tls.selectedProtocol == 'hdobby-direct/1' &&
          actual != null &&
          base64Encode(actual) == base64Encode(expected);
    } catch (_) {
      return false;
    } finally {
      tls?.destroy();
      socket?.destroy();
    }
  }

  Future<DirectHostDetails> check(String certificate) async {
    final endpoints = await _endpoints();
    final stopped = option2bool(
        kOptionStopService, await bind.mainGetOption(key: kOptionStopService));
    var listening = false;
    if (!stopped && endpoints.isNotEmpty) {
      // Bounded local TLS probe. It does not log in, capture the screen or prove LAN reachability.
      for (var attempt = 0; attempt < 2 && !listening; attempt++) {
        if (attempt > 0) {
          await Future<void>.delayed(const Duration(milliseconds: 1200));
        }
        listening = await _probe(endpoints.first, certificate);
      }
    }
    final checks = <DirectHostCheck>[
      DirectHostCheck(
          'listener',
          t('직접 연결 수신', 'Direct listener'),
          listening
              ? t('이 기기에서 인증서가 일치하는 TLS 수신을 확인했습니다.',
                  'Local TLS listener verified against this certificate.')
              : t('수신을 확인하지 못했습니다. 서비스와 주소를 확인하세요.',
                  'Listener not verified. Check the service and address.'),
          listening ? DirectCheckState.ready : DirectCheckState.actionRequired,
          action: listening ? null : t('다시 시작', 'Start again')),
    ];
    if (isMacOS) {
      for (final entry in [
        (
          'screen',
          t('화면 기록', 'Screen recording'),
          bind.mainIsCanScreenRecording(prompt: false)
        ),
        (
          'input',
          t('키보드·마우스 제어', 'Keyboard and mouse control'),
          bind.mainIsProcessTrusted(prompt: false)
        ),
        (
          'monitor',
          t('입력 모니터링', 'Input monitoring'),
          bind.mainIsCanInputMonitoring(prompt: false)
        ),
      ]) {
        checks.add(DirectHostCheck(
            entry.$1,
            entry.$2,
            entry.$3
                ? t('허용됨', 'Allowed')
                : t('macOS에서 승인이 필요합니다.', 'Approve access in macOS.'),
            entry.$3 ? DirectCheckState.ready : DirectCheckState.actionRequired,
            action: entry.$3 ? null : t('권한 요청', 'Request access')));
      }
    } else if (isAndroid) {
      for (final entry in [
        ('screen', t('화면 공유', 'Screen sharing'), gFFI.serverModel.mediaOk),
        (
          'input',
          t('키보드·마우스 제어', 'Keyboard and mouse control'),
          gFFI.serverModel.inputOk
        ),
      ]) {
        checks.add(DirectHostCheck(
            entry.$1,
            entry.$2,
            entry.$3
                ? t('허용됨', 'Allowed')
                : t('Android에서 승인이 필요합니다.', 'Approve access in Android.'),
            entry.$3 ? DirectCheckState.ready : DirectCheckState.actionRequired,
            action: entry.$3 ? null : t('권한 요청', 'Request access')));
      }
    }
    checks.add(DirectHostCheck(
        'firewall',
        t('방화벽·다른 기기에서의 연결', 'Firewall and peer reachability'),
        t('다른 기기의 접속으로 확인해야 합니다. 필요한 경우 이 앱의 수신만 허용하세요.',
            'Verify by connecting from the other device. Allow incoming access for this app if required.'),
        DirectCheckState.unverified,
        action: isMacOS || isWindows ? t('설정 열기', 'Open settings') : null));
    final approval = await bind.mainGetOption(key: kOptionApproveMode);
    final verification =
        await bind.mainGetOption(key: kOptionVerificationMethod);
    final useTemporary =
        approval != 'click' && verification != kUsePermanentPassword;
    checks.add(DirectHostCheck(
        'approval',
        t('접속 승인', 'Connection approval'),
        approval == 'click'
            ? t('호스트에서 연결 요청을 눌러 수락합니다.', 'Accept the request on the host.')
            : verification == kUsePermanentPassword
                ? t('보안 설정에서 정한 고정 비밀번호를 사용합니다.',
                    'Use the permanent password set in Security settings.')
                : t('표시된 일회용 비밀번호를 상대 기기에 입력합니다.',
                    'Enter the displayed one-time password on the controller.'),
        DirectCheckState.unverified));
    return DirectHostDetails(
        certificate: certificate,
        endpoints: endpoints,
        checks: checks,
        password: useTemporary ? await bind.mainGetTemporaryPassword() : '');
  }

  Future<void> resolve(String id) async {
    if (id == 'listener') {
      await mainSetBoolOption(kOptionDirectServer, true);
      if (isAndroid) {
        if (!gFFI.serverModel.isStart) await gFFI.serverModel.toggleService();
      } else {
        await start_service(true);
      }
    } else if (isMacOS && id == 'screen') {
      bind.mainIsCanScreenRecording(prompt: true);
    } else if (isMacOS && id == 'input') {
      bind.mainIsProcessTrusted(prompt: true);
    } else if (isMacOS && id == 'monitor') {
      bind.mainIsCanInputMonitoring(prompt: true);
    } else if (isAndroid && id == 'screen') {
      if (!gFFI.serverModel.mediaOk) {
        // Request capture again without turning an existing service off.
        await gFFI.serverModel.startService();
      }
    } else if (isAndroid && id == 'input') {
      AndroidPermissionManager.startAction(kActionAccessibilitySettings);
    } else if (id == 'firewall') {
      // macOS moved Firewall from Security to the Network settings extension.
      // Detect the installed pane so older supported macOS versions keep their route.
      final modernMac = isMacOS &&
          await Directory(
                  '/System/Library/ExtensionKit/Extensions/Network.appex')
              .exists();
      final uri = Uri.parse(isMacOS
          ? modernMac
              ? 'x-apple.systempreferences:com.apple.Network-Settings.extension?Firewall'
              : 'x-apple.systempreferences:com.apple.preference.security?Firewall'
          : 'windowsdefender://network');
      if (!await launchUrl(uri)) {
        throw StateError('Could not open firewall settings');
      }
    }
  }

  Future<void> stop() async {
    if (isAndroid) {
      await gFFI.serverModel.stopService();
    } else {
      await start_service(false);
    }
  }
}
