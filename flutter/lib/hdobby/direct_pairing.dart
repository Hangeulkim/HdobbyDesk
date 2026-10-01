import 'package:flutter/material.dart';
import 'package:hdobby_input/hdobby_input.dart';

import '../common.dart';
import '../models/platform_model.dart';
import 'host_setup.dart';

class HdobbyDirectPairingButton extends StatelessWidget {
  const HdobbyDirectPairingButton({super.key, this.initialPeer = ''});
  final String initialPeer;

  @override
  Widget build(BuildContext context) {
    if (isWeb) return const SizedBox.shrink();
    final language = hdobbyLanguage(context);
    final korean = language.split(RegExp('[-_]')).first == 'ko';
    final setup = HdobbyHostSetup(korean);
    return FilledButton.icon(
      style: FilledButton.styleFrom(
        minimumSize: const Size(0, 48),
        padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 12),
      ),
      icon: const Icon(Icons.lock_outline),
      label: Text(korean ? '직접 연결 준비' : 'Prepare direct connection'),
      onPressed: () => showDialog<void>(
        context: context,
        barrierDismissible: false,
        builder: (dialogContext) => HdobbyDirectPairing(
          canHost: !isIOS && !bind.isOutgoingOnly(),
          languageCode: language,
          initialPeer: initialPeer,
          hostSettings: isWindows && bind.mainIsInstalled()
              ? HdobbyRdpHostSetting(
                  korean: korean,
                  read: () => bind.mainIsShareRdp(),
                  write: (enabled) => bind.mainSetShareRdp(enable: enabled),
                )
              : null,
          onPrepare: () => bind.mainPrepareDirectTlsIdentity(),
          onPrepareHost: setup.prepare,
          onCheckHost: setup.check,
          onResolveHost: setup.resolve,
          onStopHost: setup.stop,
          onConnect: (peer) async {
            Navigator.of(dialogContext).pop();
            await connect(context, peer);
          },
          onInspect: (code) => bind.mainInspectDirectTlsPairing(code: code),
          onReadPrevious: (peer) => bind.mainGetDirectTlsPairing(peer: peer),
          onSave: (peer, code, previous) => bind.mainSaveDirectTlsPairing(
              peer: peer, code: code, previous: previous),
          onClose: () => Navigator.of(dialogContext).pop(),
        ),
      ),
    );
  }
}
