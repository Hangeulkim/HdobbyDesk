import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'direct_connection_code.dart';

/// Explicit identity preparation and peer trust, with optional verified connection.
class HdobbyDirectPairing extends StatefulWidget {
  const HdobbyDirectPairing({
    super.key,
    required this.canHost,
    required this.languageCode,
    required this.onPrepare,
    required this.onInspect,
    required this.onReadPrevious,
    required this.onSave,
    required this.onClose,
    this.initialPeer = '',
    this.onPrepareHost,
    this.onCheckHost,
    this.onResolveHost,
    this.onStopHost,
    this.onConnect,
  });

  final bool canHost;
  final String languageCode;
  final String initialPeer;
  final Future<String> Function() onPrepare;
  final Future<String> Function(String code) onInspect;
  final Future<String> Function(String peer) onReadPrevious;
  final Future<String> Function(String peer, String code, String previous)
      onSave;
  final VoidCallback onClose;
  final Future<DirectHostDetails> Function()? onPrepareHost;
  final Future<DirectHostDetails> Function(String certificate)? onCheckHost;
  final Future<void> Function(String check)? onResolveHost;
  final Future<void> Function()? onStopHost;
  final Future<void> Function(String peer)? onConnect;

  @override
  State<HdobbyDirectPairing> createState() => _HdobbyDirectPairingState();
}

class _HdobbyDirectPairingState extends State<HdobbyDirectPairing>
    with WidgetsBindingObserver {
  late final TextEditingController _peer;
  final _code = TextEditingController();
  final _ownCode = TextEditingController();
  String _ownFingerprint = '';
  String? _newFingerprint;
  String _oldFingerprint = '';
  String _previous = '';
  String _message = '';
  bool _failed = false;
  bool _busy = false;
  bool _confirmed = false;
  bool _saved = false;
  int _operation = 0;
  DirectHostDetails? _host;
  String? _ownEndpoint;
  bool _showPassword = false;
  bool _showConnectionHelp = false;
  bool _recheckOnResume = false;

  bool get _ko => widget.languageCode.split(RegExp('[-_]')).first == 'ko';
  String t(String ko, String en) => _ko ? ko : en;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _peer = TextEditingController(text: widget.initialPeer);
    _peer.addListener(_edited);
    _code.addListener(_edited);
  }

  void _edited() {
    setState(() {
      _newFingerprint = null;
      _oldFingerprint = '';
      _previous = '';
      _confirmed = false;
      _saved = false;
      _message = '';
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _peer.dispose();
    _code.dispose();
    _ownCode.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed || widget.onCheckHost == null)
      return;
    if (_busy) {
      _recheckOnResume = true;
    } else if (_host != null) {
      _checkHost();
    }
  }

  Future<void> _run(
      Future<void> Function(bool Function() current) action, String failure,
      {bool waitForUser = false}) async {
    if (_busy) return;
    final operation = ++_operation;
    bool current() => mounted && _operation == operation;
    setState(() {
      _busy = true;
      _message = '';
      _failed = false;
    });
    try {
      if (waitForUser) {
        await action(current);
      } else {
        await action(current).timeout(const Duration(seconds: 15));
      }
    } catch (_) {
      if (current())
        setState(() {
          _message = failure;
          _failed = true;
        });
    } finally {
      if (current()) {
        setState(() => _busy = false);
        _operation++;
        if (_recheckOnResume && _host != null) {
          _recheckOnResume = false;
          // Permissions may finish after the platform start-service call has returned.
          scheduleMicrotask(() {
            if (mounted) _checkHost();
          });
        }
      }
    }
  }

  Future<void> _prepare() => _run((current) async {
        final host = await widget.onPrepareHost?.call();
        final code = host?.certificate ?? await widget.onPrepare();
        final fingerprint = await widget.onInspect(code);
        if (!current()) return;
        setState(() {
          _ownCode.text = code;
          _ownFingerprint = fingerprint;
          _host = host;
          _ownEndpoint = host?.endpoints.firstOrNull;
        });
      },
          t('인증서를 준비하지 못했습니다. 앱의 로컬 저장 권한을 확인한 뒤 다시 시도하세요.',
              'Could not prepare the certificate. Check local application storage and retry.'),
          waitForUser: widget.onPrepareHost != null);

  Future<void> _checkHost({String? resolve}) => _run((current) async {
        if (resolve == 'stop') {
          await widget.onStopHost?.call();
        } else if (resolve != null) {
          await widget.onResolveHost?.call(resolve);
        }
        final host = await widget.onCheckHost?.call(_ownCode.text);
        if (!current() || host == null) return;
        setState(() {
          _host = host;
          if (!host.endpoints.contains(_ownEndpoint)) {
            _ownEndpoint = host.endpoints.firstOrNull;
          }
        });
      },
          t('준비 상태를 확인하지 못했습니다. 다시 확인하세요.',
              'Could not check readiness. Try again.'),
          waitForUser: resolve != null);

  Future<void> _pasteCode() async {
    bool importedConnection = false;
    await _run((current) async {
      final text =
          (await Clipboard.getData(Clipboard.kTextPlain))?.text?.trim();
      if (!current()) return;
      if (text?.startsWith(DirectConnectionCode.prefix) == true ||
          text?.startsWith(DirectConnectionCode.legacyPrefix) == true) {
        final connection = DirectConnectionCode.decode(text!);
        _peer.text = connection.endpoint;
        _code.text = connection.certificate;
        importedConnection = true;
        return;
      }
      if (text == null ||
          !(text.toLowerCase().startsWith('hdobby2:') ||
              text.startsWith('hdobby1:'))) {
        throw const FormatException(
            'Clipboard does not contain a pairing code');
      }
      // Controller listeners invalidate any previous review or confirmation.
      _code.text = text;
    },
        t('클립보드에 인증서 코드가 없습니다. 기존 입력은 유지됩니다.',
            'The clipboard does not contain a certificate code. Existing input is preserved.'));
    if (mounted && importedConnection) await _review();
  }

  Future<void> _review() async {
    if (_busy) return;
    if (_code.text.trim().startsWith(DirectConnectionCode.prefix) ||
        _code.text.trim().startsWith(DirectConnectionCode.legacyPrefix)) {
      try {
        final connection = DirectConnectionCode.decode(_code.text.trim());
        _peer.text = connection.endpoint;
        _code.text = connection.certificate;
      } catch (_) {
        setState(() {
          _message = t('연결 코드를 확인하세요.', 'Check the connection code.');
          _failed = true;
        });
        return;
      }
    }
    if (_peer.text.trim().isEmpty || _code.text.trim().isEmpty) {
      setState(() {
        _message = t('상대 주소와 인증서 코드를 입력하세요.',
            'Enter the peer address and certificate code.');
        _failed = true;
      });
      return;
    }
    if (_busy) return;
    setState(() {
      _newFingerprint = null;
      _confirmed = false;
      _saved = false;
    });
    final peer = _peer.text.trim();
    final code = _code.text.trim();
    await _run((current) async {
      final previous = await widget.onReadPrevious(peer);
      final fingerprint = await widget.onInspect(code);
      var old = '';
      if (previous.isNotEmpty) {
        try {
          old = await widget.onInspect(previous);
        } catch (_) {
          old = t('기존 인증서를 확인할 수 없습니다.',
              'The previous certificate could not be inspected.');
        }
      }
      if (!current()) return;
      setState(() {
        _previous = previous;
        _newFingerprint = fingerprint;
        _oldFingerprint = old;
        _confirmed = false;
      });
    },
        t('주소 또는 인증서 코드를 확인하세요. 변경된 등록 정보는 창을 다시 열어 확인하세요.',
            'Check the address and certificate code. Reopen this dialog if the saved pairing changed.'));
  }

  Future<void> _save() async {
    if (!_confirmed || _newFingerprint == null || _saved) return;
    final peer = _peer.text.trim();
    final code = _code.text.trim();
    final fingerprint = _newFingerprint;
    await _run((current) async {
      final stored = await widget.onSave(peer, code, _previous);
      if (stored != fingerprint) throw StateError('Stored fingerprint differs');
      if (!current()) return;
      setState(() {
        _saved = true;
        _message = t('등록했습니다. 창을 닫고 같은 주소로 연결하세요.',
            'Paired. Close this dialog and connect to the same address.');
      });
    },
        t('등록 저장을 확인하지 못했습니다. 입력은 보존됐습니다. 창을 다시 열어 현재 등록을 확인하세요.',
            'Could not confirm pairing was saved. Your input was preserved. Reopen to review the current pairing.'));
    if (mounted && _saved && widget.onConnect != null) {
      await widget.onConnect!(peer);
    }
  }

  @override
  Widget build(BuildContext context) {
    final replacing = _previous.isNotEmpty && _previous != _code.text.trim();
    return PopScope(
      canPop: !_busy,
      child: AlertDialog(
        title: Text(t('직접 연결 준비', 'Prepare direct connection')),
        content: SizedBox(
          width: 520,
          child: SingleChildScrollView(
            child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(t(
                      '조작받을 기기에서 연결을 준비하고, 조작할 기기에 연결 코드를 붙여넣으세요. 처음에는 호스트의 지문을 확인합니다.',
                      'Prepare the host, then paste its connection code on the controller. Verify the host fingerprint the first time.')),
                  if (widget.canHost) ...[
                    const SizedBox(height: 18),
                    Text(t('이 기기를 호스트로', 'Use this device as a host'),
                        style: Theme.of(context).textTheme.titleMedium),
                    Text(t(
                        '연결 준비를 누르면 인증서를 준비합니다. 서비스 준비 기능이 있는 경우 연결 대기를 시작하고 필요한 권한을 요청합니다.',
                        'Prepare the certificate here. When host setup is available, this also starts listening and requests required access.')),
                    OutlinedButton.icon(
                        onPressed: _busy ? null : _prepare,
                        icon: const Icon(Icons.key),
                        label: Text(widget.onPrepareHost == null
                            ? t('인증서 준비 / 확인', 'Prepare / show certificate')
                            : t('이 기기 연결 준비', 'Prepare this host'))),
                    if (_host != null) ...[
                      for (final check in _host!.checks.where((check) =>
                          check.id != 'firewall' || _showConnectionHelp))
                        ListTile(
                          contentPadding: EdgeInsets.zero,
                          leading: Icon(check.state == DirectCheckState.ready
                              ? Icons.check_circle_outline
                              : Icons.info_outline),
                          title: Text(check.title),
                          subtitle: Text(check.detail),
                          trailing: check.action == null
                              ? null
                              : TextButton(
                                  onPressed: _busy
                                      ? null
                                      : () => _checkHost(resolve: check.id),
                                  child: Text(check.action!)),
                        ),
                      TextButton(
                          onPressed: () => setState(
                              () => _showConnectionHelp = !_showConnectionHelp),
                          child: Text(_showConnectionHelp
                              ? t('문제 해결 닫기', 'Hide troubleshooting')
                              : t('연결이 안 될 때',
                                  'Troubleshoot a failed connection'))),
                      TextButton(
                          onPressed: _busy ? null : () => _checkHost(),
                          child: Text(t('상태 다시 확인', 'Check again'))),
                      if (widget.onStopHost != null)
                        TextButton(
                            onPressed: _busy
                                ? null
                                : () => _checkHost(resolve: 'stop'),
                            child: Text(
                                t('연결 받기 중지', 'Stop accepting connections'))),
                      Text(t(
                          '창 닫기는 서비스 상태를 바꾸지 않습니다. 연결 받기를 끝내려면 중지 버튼을 사용하세요.',
                          'Closing this window does not change the service state. Use the stop button to end incoming access.')),
                      if (_host!.endpoints.isNotEmpty)
                        DropdownButtonFormField<String>(
                            value: _ownEndpoint,
                            isExpanded: true,
                            decoration: InputDecoration(
                                labelText: t('상대가 연결할 주소',
                                    'Address for the controller')),
                            items: _host!.endpoints
                                .map((endpoint) => DropdownMenuItem(
                                    value: endpoint, child: Text(endpoint)))
                                .toList(),
                            onChanged: _busy
                                ? null
                                : (value) =>
                                    setState(() => _ownEndpoint = value)),
                      if (_host!.password.isNotEmpty)
                        Row(children: [
                          Expanded(
                              child: SelectableText(
                                  t('일회용 비밀번호: ', 'One-time password: ') +
                                      (_showPassword
                                          ? _host!.password
                                          : '••••••'))),
                          IconButton(
                              onPressed: () => setState(
                                  () => _showPassword = !_showPassword),
                              tooltip: t('비밀번호 표시 전환', 'Show or hide password'),
                              icon: Icon(_showPassword
                                  ? Icons.visibility_off
                                  : Icons.visibility)),
                        ]),
                      Text(t(
                          '연결 코드는 주소와 공개 인증서만 포함합니다. 접속 비밀번호나 호스트 수락은 별도입니다.',
                          'The code contains the address and public certificate. Access still requires a password or host acceptance.')),
                    ],
                    if (_ownCode.text.isNotEmpty) ...[
                      TextField(
                          controller: _ownCode,
                          readOnly: true,
                          minLines: 2,
                          maxLines: 4,
                          decoration: InputDecoration(
                              labelText: t('이 기기의 공개 인증서 코드',
                                  'This device’s public certificate code'))),
                      SelectableText(_ownFingerprint,
                          style: const TextStyle(fontFamily: 'monospace')),
                      TextButton.icon(
                          onPressed: _busy
                              ? null
                              : () => _run((current) async {
                                    await Clipboard.setData(ClipboardData(
                                        text: _ownEndpoint == null
                                            ? _ownCode.text
                                            : DirectConnectionCode(
                                                    _ownEndpoint!,
                                                    _ownCode.text)
                                                .encode()));
                                    if (current())
                                      setState(() => _message = _ownEndpoint ==
                                              null
                                          ? t('공개 인증서 코드를 복사했습니다.',
                                              'Public certificate code copied.')
                                          : t('연결 코드를 복사했습니다.',
                                              'Connection code copied.'));
                                  }, t('복사하지 못했습니다.', 'Could not copy.')),
                          icon: const Icon(Icons.copy),
                          label: Text(_ownEndpoint == null
                              ? t('공개 코드 복사', 'Copy public code')
                              : t('연결 코드 복사', 'Copy connection code'))),
                    ],
                    const Divider(height: 28),
                  ],
                  Text(t('상대 기기 등록', 'Pair the other device'),
                      style: Theme.of(context).textTheme.titleMedium),
                  TextField(
                      key: const ValueKey('pair-peer'),
                      controller: _peer,
                      enabled: !_busy,
                      autocorrect: false,
                      enableSuggestions: false,
                      decoration: InputDecoration(
                          labelText:
                              t('상대 IP 또는 호스트:포트', 'Peer IP or host:port'))),
                  const SizedBox(height: 10),
                  TextField(
                      key: const ValueKey('pair-code'),
                      controller: _code,
                      enabled: !_busy,
                      autocorrect: false,
                      enableSuggestions: false,
                      minLines: 2,
                      maxLines: 4,
                      decoration: InputDecoration(
                          labelText: t('상대 기기에서 받은 인증서 코드',
                              'Certificate code from the host'))),
                  TextButton.icon(
                      onPressed: _busy ? null : _pasteCode,
                      icon: const Icon(Icons.content_paste),
                      label: Text(widget.onConnect == null
                          ? t('인증서 코드 붙여넣기', 'Paste certificate code')
                          : t('연결 코드 붙여넣기', 'Paste connection code'))),
                  OutlinedButton(
                      onPressed: _busy ? null : _review,
                      child: Text(t('지문 확인', 'Review fingerprint'))),
                  if (_newFingerprint != null) ...[
                    if (replacing) ...[
                      Text(t('기존 등록을 변경합니다. 기존 지문:',
                          'This replaces the saved pairing. Previous fingerprint:')),
                      SelectableText(_oldFingerprint,
                          style: const TextStyle(fontFamily: 'monospace')),
                    ],
                    Text(t('등록할 지문:', 'Fingerprint to register:')),
                    SelectableText(_newFingerprint!,
                        style: const TextStyle(fontFamily: 'monospace')),
                    CheckboxListTile(
                        contentPadding: EdgeInsets.zero,
                        value: _confirmed,
                        onChanged: _busy || _saved
                            ? null
                            : (v) => setState(() => _confirmed = v ?? false),
                        title: Text(t('호스트 화면의 지문과 일치함을 확인했습니다.',
                            'I verified this matches the fingerprint on the host.'))),
                    FilledButton(
                        onPressed:
                            _busy || !_confirmed || _saved ? null : _save,
                        child: Text(widget.onConnect != null
                            ? t('확인하고 연결', 'Verify and connect')
                            : replacing
                                ? t('확인한 기기로 변경',
                                    'Replace with verified device')
                                : t('확인한 기기 등록', 'Register verified device'))),
                  ],
                  if (_busy)
                    const Padding(
                        padding: EdgeInsets.symmetric(vertical: 12),
                        child: LinearProgressIndicator()),
                  if (_message.isNotEmpty)
                    Padding(
                        padding: const EdgeInsets.only(top: 12),
                        child: Semantics(
                            liveRegion: true,
                            child: Text(_message,
                                style: TextStyle(
                                    color: _failed
                                        ? Theme.of(context).colorScheme.error
                                        : null)))),
                ]),
          ),
        ),
        actions: [
          TextButton(
              onPressed: _busy ? null : widget.onClose,
              child: Text(t('닫기', 'Close')))
        ],
      ),
    );
  }
}
