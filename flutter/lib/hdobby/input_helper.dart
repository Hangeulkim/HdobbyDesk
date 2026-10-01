import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:get/get.dart';
import 'package:hdobby_input/hdobby_input.dart';

import '../common.dart';
import '../consts.dart';
import '../models/model.dart';
import '../models/platform_model.dart';

String hdobbyInputHelperLabel(BuildContext context) =>
    hdobbyLanguage(context).split(RegExp('[-_]')).first == 'ko'
        ? '입력 도우미'
        : 'Input helper';

Future<void> switchHdobbyRemoteInput(FFI ffi) async {
  final sessionId = ffi.sessionId;
  final peerInfo = ffi.ffiModel.pi;
  final platform = peerInfo.platform;
  bool ready() =>
      !ffi.closed &&
      ffi.sessionId == sessionId &&
      identical(peerInfo, ffi.ffiModel.pi) &&
      peerInfo.isSet.value &&
      !ffi.ffiModel.viewOnly &&
      ffi.ffiModel.keyboard &&
      !ffi.ffiModel.waitForImageDialogShow.value;
  if (!ready() ||
      (platform != kPeerPlatformWindows && platform != kPeerPlatformMacOS)) {
    throw StateError('Remote IME unavailable');
  }
  // Translate mode can inject local text even after the remote IME changes.
  // Use the existing physical-key mode so subsequent hardware typing follows it.
  if (!bind.sessionIsKeyboardModeSupported(
      sessionId: sessionId, mode: kKeyMapMode)) {
    throw StateError('Remote keyboard mapping unavailable');
  }
  if (!ready()) throw StateError('Remote IME unavailable');
  ffi.inputModel.enterOrLeave(false);
  await bind.sessionSetKeyboardMode(sessionId: sessionId, value: kKeyMapMode);
  await ffi.inputModel.updateKeyboardMode();
  if (!ready()) throw StateError('Remote IME unavailable');
  final key = remoteImeKey(platform == kPeerPlatformWindows
      ? RemoteImeTarget.windows
      : RemoteImeTarget.macOS);
  await bind.sessionInputKey(
      sessionId: sessionId,
      name: key.name,
      down: false,
      press: true,
      alt: false,
      shift: false,
      ctrl: key.ctrl,
      command: false);
}

Future<void> showHdobbyInputHelper(BuildContext context, FFI ffi) async {
  final sessionId = ffi.sessionId;
  final peerId = ffi.id;
  final peerLabel = getDesktopTabLabel(
      peerId, bind.mainGetPeerOptionSync(id: peerId, key: 'alias'));
  final peerInfo = ffi.ffiModel.pi;
  final isMac = peerInfo.platform == kPeerPlatformMacOS;
  bool ready() =>
      !ffi.closed &&
      ffi.sessionId == sessionId &&
      identical(peerInfo, ffi.ffiModel.pi) &&
      peerInfo.isSet.value &&
      !ffi.ffiModel.waitForImageDialogShow.value &&
      !ffi.ffiModel.viewOnly &&
      ffi.ffiModel.keyboard &&
      ffi.connType == ConnType.defaultConn;

  void requireReady() {
    if (!ready()) throw StateError('Remote input unavailable');
  }

  Future<void> mouseClick(String button) async {
    requireReady();
    try {
      await bind.sessionSendMouse(
          sessionId: sessionId,
          msg: jsonEncode({'type': 'down', 'buttons': button}));
    } finally {
      // A failed call may have enqueued the press; still request one release.
      await bind.sessionSendMouse(
          sessionId: sessionId,
          msg: jsonEncode({'type': 'up', 'buttons': button}));
    }
  }

  Future<void> action(InputAction action) async {
    requireReady();
    if (action == InputAction.switchInput) {
      await switchHdobbyRemoteInput(ffi);
      return;
    }
    if (action == InputAction.leftClick || action == InputAction.doubleClick) {
      await mouseClick('left');
      if (action == InputAction.doubleClick) {
        await Future<void>.delayed(const Duration(milliseconds: 80));
        await mouseClick('left');
      }
      return;
    }
    if (action == InputAction.rightClick) {
      await mouseClick('right');
      return;
    }
    const keys = {
      InputAction.escape: 'VK_ESCAPE',
      InputAction.tab: 'VK_TAB',
      InputAction.backspace: 'VK_BACK',
      InputAction.enter: 'VK_ENTER',
      InputAction.left: 'VK_LEFT',
      InputAction.up: 'VK_UP',
      InputAction.down: 'VK_DOWN',
      InputAction.right: 'VK_RIGHT',
      InputAction.selectAll: 'VK_A',
      InputAction.copy: 'VK_C',
      InputAction.paste: 'VK_V',
      InputAction.undo: 'VK_Z',
      InputAction.save: 'VK_S',
    };
    final primary = action.index >= InputAction.selectAll.index;
    await bind.sessionInputKey(
        sessionId: sessionId,
        name: keys[action]!,
        down: false,
        press: true,
        alt: false,
        shift: false,
        ctrl: primary && !isMac,
        command: primary && isMac);
  }

  ffi.inputModel.enterOrLeave(false);
  try {
    if (isAndroid) await ffi.invokeMethod('enable_soft_keyboard', true);
    if (!context.mounted) return;
    await showDialog<void>(
      context: context,
      barrierDismissible: false,
      builder: (dialogContext) => AnimatedBuilder(
        animation: ffi.ffiModel,
        builder: (_, __) => Obx(() => HdobbyInputPanel(
              peerLabel: peerLabel,
              languageCode: hdobbyLanguage(dialogContext),
              enabled: ready(),
              textEnabled: ffi.inputModel.keyboardInputAllowed,
              macPeer: isMac,
              windowsPeer: peerInfo.platform == kPeerPlatformWindows,
              androidPeer: peerInfo.platform == kPeerPlatformAndroid,
              onText: (text) async {
                requireReady();
                if (!ffi.inputModel.keyboardInputAllowed) {
                  throw StateError('Text input unavailable');
                }
                await bind.sessionInputString(
                    sessionId: sessionId, value: text);
              },
              onAction: action,
              onClose: () => Navigator.of(dialogContext).pop(),
            )),
      ),
    );
  } finally {
    await SystemChannels.textInput.invokeMethod<void>('TextInput.hide');
    if (isAndroid && !ffi.closed) {
      await ffi.invokeMethod('enable_soft_keyboard', false);
    }
  }
}
