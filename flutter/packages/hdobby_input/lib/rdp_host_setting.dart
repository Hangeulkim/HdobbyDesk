import 'package:flutter/material.dart';

/// Read back the host setting after elevation: a cancelled OS prompt must not
/// leave the UI claiming that RDP sharing was enabled.
class HdobbyRdpHostSetting extends StatefulWidget {
  const HdobbyRdpHostSetting({
    super.key,
    required this.read,
    required this.write,
    this.korean = false,
    this.enabled = true,
  });
  final bool Function() read;
  final Future<void> Function(bool) write;
  final bool korean;
  final bool enabled;

  @override
  State<HdobbyRdpHostSetting> createState() => _HdobbyRdpHostSettingState();
}

class _HdobbyRdpHostSettingState extends State<HdobbyRdpHostSetting> {
  bool? _value;
  bool _saving = false;
  String? _error;
  String t(String ko, String en) => widget.korean ? ko : en;

  @override
  void initState() {
    super.initState();
    try {
      _value = widget.read();
    } catch (_) {
      _error = t('호스트 설정을 읽지 못했습니다.', 'Could not read host settings.');
    }
  }

  Future<void> _change(bool requested) async {
    setState(() {
      _saving = true;
      _error = null;
    });
    try {
      await widget.write(requested);
      final actual = widget.read();
      if (!mounted) return;
      setState(() {
        _value = actual;
        if (actual != requested) {
          _error = t('설정이 적용되지 않았습니다. 관리자 승인 후 다시 시도하세요.',
              'Setting was not applied. Approve administrator access and retry.');
        }
      });
    } catch (_) {
      if (mounted) {
        setState(
            () => _error = t('설정을 저장하지 못했습니다.', 'Could not save the setting.'));
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  @override
  Widget build(BuildContext context) => Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: Text(
                t('별도 RDP 바탕 화면 제어 허용', 'Control the separate RDP desktop')),
            subtitle: Text(t(
                '끄면 물리 모니터의 Console만 공유합니다. 켜면 접속자가 RDP 세션도 선택할 수 있습니다. 저장할 때 관리자 승인이 필요할 수 있습니다.',
                'Off shares the physical Console only. On also lets a controller choose an RDP session. Saving may require administrator approval.')),
            value: _value ?? false,
            onChanged:
                widget.enabled && !_saving && _value != null ? _change : null,
          ),
          if (_saving) const LinearProgressIndicator(),
          if (_error != null)
            Text(_error!,
                style: TextStyle(color: Theme.of(context).colorScheme.error)),
        ],
      );
}
