import 'package:flutter/material.dart';
import 'package:hdobby_input/hdobby_input.dart';

void main() => runApp(MaterialApp(
      title: 'hdobbyremotecontrol 입력 UI 미리보기',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
          colorSchemeSeed: const Color(0xff3157cc), useMaterial3: true),
      home: const InputPreview(),
    ));

/// UI-only harness. Events stay here and are never sent over the network.
class InputPreview extends StatefulWidget {
  const InputPreview({super.key});
  @override
  State<InputPreview> createState() => _InputPreviewState();
}

class _InputPreviewState extends State<InputPreview> {
  String _target = 'Windows';
  String _result = '아직 입력 요청이 없습니다.';
  bool _allowed = true;
  bool _fail = false;

  Future<void> _record(String event) async {
    if (_fail) throw StateError('Simulated transport failure');
    setState(() => _result = event);
  }

  Future<void> _open() async {
    await showDialog<void>(
        context: context,
        barrierDismissible: false,
        builder: (context) => HdobbyInputPanel(
              peerLabel: '$_target · UI 테스트 대상',
              languageCode: 'ko',
              enabled: _allowed,
              macPeer: _target == 'Mac',
              windowsPeer: _target == 'Windows',
              androidPeer: _target == 'Android',
              onText: (text) => _record('텍스트 요청: $text'),
              onAction: (action) => _record('버튼 요청: ${action.name}'),
              onClose: () => Navigator.of(context).pop(),
            ));
  }

  @override
  Widget build(BuildContext context) => Scaffold(
        appBar: AppBar(title: const Text('hdobbyremotecontrol')),
        body: Center(
            child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 680),
                child: ListView(
                    shrinkWrap: true,
                    padding: const EdgeInsets.all(24),
                    children: [
                      Text('입력 UI 미리보기',
                          style: Theme.of(context).textTheme.headlineMedium),
                      const SizedBox(height: 12),
                      const Text(
                          '실제 원격 연결이 없는 테스트 화면입니다. 입력 도우미의 표시·조작·실패 동작을 확인할 수 있습니다.'),
                      const SizedBox(height: 24),
                      DropdownButtonFormField<String>(
                          value: _target,
                          decoration:
                              const InputDecoration(labelText: '원격 기기 종류'),
                          items: ['Windows', 'Mac', 'Android']
                              .map((name) => DropdownMenuItem(
                                  value: name, child: Text(name)))
                              .toList(),
                          onChanged: (value) {
                            if (value != null) setState(() => _target = value);
                          }),
                      SwitchListTile(
                          title: const Text('입력 권한'),
                          value: _allowed,
                          onChanged: (value) =>
                              setState(() => _allowed = value)),
                      SwitchListTile(
                          title: const Text('전송 실패 재현'),
                          value: _fail,
                          onChanged: (value) => setState(() => _fail = value)),
                      const SizedBox(height: 16),
                      FilledButton.icon(
                          onPressed: _open,
                          icon: const Icon(Icons.edit_note),
                          label: const Text('입력 도우미 열기')),
                      const SizedBox(height: 24),
                      const Text('마지막 테스트 요청'),
                      SelectableText(_result),
                    ]))),
      );
}
