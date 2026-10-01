import 'package:flutter/material.dart';

export 'direct_pairing.dart';
export 'direct_connection_code.dart';
export 'soft_keyboard_edit.dart';
export 'direct_connection_error.dart';
export 'session_resilience.dart';
export 'windows_session_choice.dart';

enum InputAction {
  leftClick,
  doubleClick,
  rightClick,
  escape,
  tab,
  backspace,
  enter,
  left,
  up,
  down,
  right,
  selectAll,
  copy,
  paste,
  undo,
  save,
  switchInput,
}

enum RemoteImeTarget { windows, macOS }

class RemoteImeKey {
  const RemoteImeKey(this.name, {this.ctrl = false});
  final String name;
  final bool ctrl;
}

RemoteImeKey remoteImeKey(RemoteImeTarget target) =>
    target == RemoteImeTarget.windows
        ? const RemoteImeKey('VK_HANGUL')
        : const RemoteImeKey('VK_SPACE', ctrl: true);

/// Local composition is deliberately separate from sending remote input.
class HdobbyInputPanel extends StatefulWidget {
  const HdobbyInputPanel({
    super.key,
    required this.peerLabel,
    required this.enabled,
    required this.onText,
    required this.onAction,
    required this.onClose,
    this.macPeer = false,
    this.windowsPeer = false,
    this.androidPeer = false,
    this.textEnabled = true,
    this.languageCode,
  });

  final String peerLabel;
  final bool enabled;
  final bool textEnabled;
  final bool macPeer;
  final bool windowsPeer;
  final bool androidPeer;
  final String? languageCode;
  final Future<void> Function(String) onText;
  final Future<void> Function(InputAction) onAction;
  final VoidCallback onClose;

  @override
  State<HdobbyInputPanel> createState() => _HdobbyInputPanelState();
}

class _HdobbyInputPanelState extends State<HdobbyInputPanel> {
  final _text = TextEditingController();
  bool _busy = false;
  bool _failed = false;
  bool _sent = false;
  String _lastText = '';

  bool get _ko =>
      (widget.languageCode ?? Localizations.localeOf(context).languageCode)
          .split(RegExp('[-_]'))
          .first ==
      'ko';
  String _label(String ko, String en) => _ko ? ko : en;
  bool get _canAct => widget.enabled && !_busy;
  bool get _composing =>
      _text.value.composing.isValid && !_text.value.composing.isCollapsed;

  @override
  void initState() {
    super.initState();
    _text.addListener(_edited);
  }

  void _edited() {
    setState(() {
      // Focus/IME composition changes can arrive after a send fails. Only a
      // content edit should dismiss that result, not a selection update.
      if (_lastText != _text.text) {
        _lastText = _text.text;
        _sent = false;
        _failed = false;
      }
    });
  }

  @override
  void dispose() {
    _text.removeListener(_edited);
    _text.dispose();
    super.dispose();
  }

  Future<void> _run(Future<void> Function() send,
      {bool clearText = false}) async {
    if (!_canAct) return;
    setState(() {
      _busy = true;
      _failed = false;
      _sent = false;
    });
    try {
      await send();
      if (!mounted) return;
      if (clearText) _text.clear();
      setState(() => _sent = true);
    } catch (_) {
      // Errors can contain user text. Keep the draft, never display/log payloads.
      if (mounted) setState(() => _failed = true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Widget _action(InputAction action, String label, {IconData? icon}) {
    return OutlinedButton(
      key: ValueKey(action.name),
      style: OutlinedButton.styleFrom(
        minimumSize: const Size(48, 48),
        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      ),
      onPressed: _canAct ? () => _run(() => widget.onAction(action)) : null,
      child: icon == null
          ? Text(label)
          : Tooltip(
              message: label,
              excludeFromSemantics: true,
              child: Semantics(label: label, child: Icon(icon)),
            ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final primary = widget.macPeer ? 'Cmd' : 'Ctrl';
    final canSend = _canAct && widget.textEnabled && _text.text.isNotEmpty;
    return SafeArea(
      child: Dialog(
        insetPadding: const EdgeInsets.all(12),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560),
          child: SingleChildScrollView(
            padding: const EdgeInsets.all(20),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(children: [
                  Expanded(
                      child: Text(
                    _label('입력 도우미', 'Input helper'),
                    style: Theme.of(context).textTheme.titleLarge,
                  )),
                  IconButton(
                    key: const ValueKey('close-input-helper'),
                    tooltip: _label('닫기', 'Close'),
                    onPressed: widget.onClose,
                    icon: const Icon(Icons.close),
                  ),
                ]),
                Text(widget.peerLabel,
                    style: Theme.of(context).textTheme.bodySmall),
                const SizedBox(height: 16),
                if (widget.macPeer || widget.windowsPeer) ...[
                  FilledButton.tonalIcon(
                    key: const ValueKey('switch-remote-input'),
                    style:
                        FilledButton.styleFrom(minimumSize: const Size(48, 48)),
                    onPressed: _canAct
                        ? () =>
                            _run(() => widget.onAction(InputAction.switchInput))
                        : null,
                    icon: const Icon(Icons.language),
                    label: Text(_label('원격 한/영 전환', 'Switch remote input')),
                  ),
                  const SizedBox(height: 8),
                  Text(widget.macPeer
                      ? _label(
                          'Mac의 이전 입력 소스로 전환합니다. Mac에 한국어·영어와 Control+Space 단축키가 설정되어 있어야 합니다.',
                          'Selects the previous Mac input source. Configure Korean/English and Control+Space on the Mac.')
                      : _label(
                          'Windows의 한국어 IME에서 한/영을 전환합니다. 먼저 원격 작업 표시줄에서 한국어 IME를 선택하세요.',
                          'Toggles Hangul in Windows Korean IME. Select Korean IME on the remote taskbar first.')),
                  Text(_label('전환 후 물리 키보드 입력에는 원격 기기의 키보드 배열이 적용됩니다.',
                      'After switching, physical keys use the remote keyboard layout.')),
                  const SizedBox(height: 20),
                ],
                if (!widget.enabled)
                  Text(
                      _label('현재 입력할 수 없습니다. 연결과 제어 권한을 확인하세요.',
                          'Input unavailable. Check the connection and control permission.'),
                      key: const ValueKey('input-unavailable')),
                Text(_label('한글·문장을 여기서 완성한 뒤 전송하세요. 입력 중에는 원격 기기로 보내지 않습니다.',
                    'Compose text here, then send it. Editing stays on this device.')),
                const SizedBox(height: 12),
                TextField(
                  key: const ValueKey('text-draft'),
                  controller: _text,
                  enabled: !_busy,
                  minLines: 3,
                  maxLines: 6,
                  maxLength: 4096,
                  keyboardType: TextInputType.multiline,
                  textInputAction: TextInputAction.newline,
                  enableSuggestions: false,
                  autocorrect: false,
                  decoration: InputDecoration(
                    border: const OutlineInputBorder(),
                    labelText: _label('전송할 텍스트', 'Text to send'),
                    helperText: _composing
                        ? _label('전송하면 현재 보이는 텍스트를 보냅니다.',
                            'Send uses the text currently shown.')
                        : null,
                  ),
                ),
                if (!widget.textEnabled)
                  Text(_label('이 연결에서는 텍스트 전송이 비활성화되어 있습니다.',
                      'Text sending is disabled for this connection.')),
                const SizedBox(height: 8),
                FilledButton.icon(
                  key: const ValueKey('send-text'),
                  style:
                      FilledButton.styleFrom(minimumSize: const Size(48, 48)),
                  onPressed: canSend
                      ? () {
                          final draft = _text.text;
                          FocusScope.of(context).unfocus();
                          _run(() => widget.onText(draft), clearText: true);
                        }
                      : null,
                  icon: const Icon(Icons.send),
                  label: Text(_label('텍스트 전송', 'Send text')),
                ),
                if (_busy) const LinearProgressIndicator(),
                if (_failed)
                  Text(
                      _label('전송을 확인할 수 없습니다. 원격 화면을 확인한 뒤 다시 시도하세요.',
                          'Sending could not be confirmed. Check the remote screen before retrying.'),
                      key: const ValueKey('send-failed')),
                if (_sent)
                  Text(
                      _label('전송을 요청했습니다. 원격 화면에서 결과를 확인하세요.',
                          'Input requested. Check the result on the remote screen.'),
                      key: const ValueKey('input-requested')),
                const SizedBox(height: 20),
                if (!widget.androidPeer) ...[
                  Text(_label('클릭은 현재 원격 커서 위치에 적용됩니다.',
                      'Clicks use the current remote cursor position.')),
                  const SizedBox(height: 8),
                  Wrap(spacing: 8, runSpacing: 8, children: [
                    _action(
                        InputAction.leftClick, _label('왼쪽 클릭', 'Left click')),
                    _action(InputAction.doubleClick,
                        _label('더블 클릭', 'Double click')),
                    _action(InputAction.rightClick,
                        _label('오른쪽 클릭', 'Right click')),
                  ]),
                  const SizedBox(height: 16),
                ],
                Wrap(spacing: 8, runSpacing: 8, children: [
                  _action(InputAction.escape, 'Esc'),
                  _action(InputAction.tab, 'Tab'),
                  _action(InputAction.backspace, 'Backspace'),
                  _action(InputAction.enter, 'Enter'),
                  _action(InputAction.left, _label('왼쪽', 'Left'),
                      icon: Icons.arrow_back),
                  _action(InputAction.up, _label('위', 'Up'),
                      icon: Icons.arrow_upward),
                  _action(InputAction.down, _label('아래', 'Down'),
                      icon: Icons.arrow_downward),
                  _action(InputAction.right, _label('오른쪽', 'Right'),
                      icon: Icons.arrow_forward),
                  if (!widget.androidPeer) ...[
                    _action(InputAction.selectAll, '$primary+A'),
                    _action(InputAction.copy, '$primary+C'),
                    _action(InputAction.paste, '$primary+V'),
                    _action(InputAction.undo, '$primary+Z'),
                    _action(InputAction.save, '$primary+S'),
                  ],
                ]),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
