import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter/services.dart';

const int maxIosClipboardTextBytes = 4 * 1024 * 1024;
const int maxIosClipboardPngBytes = 24 * 1024 * 1024;
const int maxIosClipboardPixels = 16 * 1024 * 1024;

class IosClipboardException implements Exception {
  const IosClipboardException(this.message);

  final String message;

  @override
  String toString() => message;
}

class IosClipboardPayload {
  const IosClipboardPayload({this.text = '', this.png});

  final String text;
  final Uint8List? png;

  bool get isEmpty => text.isEmpty && (png == null || png!.isEmpty);
}

int _readUint32BigEndian(Uint8List bytes, int offset) {
  return ByteData.sublistView(bytes, offset, offset + 4)
      .getUint32(0, Endian.big);
}

bool isValidIosClipboardPng(Uint8List png) {
  const signature = <int>[137, 80, 78, 71, 13, 10, 26, 10];
  if (png.length < 33 || png.length > maxIosClipboardPngBytes) return false;
  for (var i = 0; i < signature.length; i++) {
    if (png[i] != signature[i]) return false;
  }
  if (_readUint32BigEndian(png, 8) != 13 ||
      ascii.decode(png.sublist(12, 16), allowInvalid: true) != 'IHDR') {
    return false;
  }
  final width = _readUint32BigEndian(png, 16);
  final height = _readUint32BigEndian(png, 20);
  if (width == 0 || height == 0) return false;
  return width <= maxIosClipboardPixels ~/ height;
}

IosClipboardPayload validateIosClipboardPayload({
  String text = '',
  Uint8List? png,
}) {
  if (utf8.encode(text).length > maxIosClipboardTextBytes ||
      (png != null && png.length > maxIosClipboardPngBytes)) {
    throw const IosClipboardException('Clipboard is too large');
  }
  if (png != null && png.isNotEmpty && !isValidIosClipboardPng(png)) {
    throw const IosClipboardException('Clipboard image is invalid');
  }
  final payload = IosClipboardPayload(text: text, png: png);
  if (payload.isEmpty) {
    throw const IosClipboardException('Clipboard is empty');
  }
  return payload;
}

IosClipboardPayload parseIosClipboardPayload(Object? value) {
  if (value is! Map) {
    throw const IosClipboardException('Clipboard format is unsupported');
  }
  final text = value['text'];
  final png = value['png'];
  if (text != null && text is! String || png != null && png is! Uint8List) {
    throw const IosClipboardException('Clipboard format is unsupported');
  }
  return validateIosClipboardPayload(
    text: text as String? ?? '',
    png: png as Uint8List?,
  );
}

class IosClipboardBridge {
  IosClipboardBridge({
    MethodChannel channel =
        const MethodChannel('com.hdobby.hdobbydesk/clipboard'),
  }) : _channel = channel;

  final MethodChannel _channel;

  Future<IosClipboardPayload> readLocal() async {
    try {
      return parseIosClipboardPayload(
        await _channel.invokeMethod<Object?>('readClipboard'),
      );
    } on PlatformException catch (error) {
      throw IosClipboardException(error.code);
    }
  }

  Future<void> writeRemote({String text = '', Uint8List? png}) async {
    final payload = validateIosClipboardPayload(text: text, png: png);
    final arguments = <String, Object>{};
    if (payload.text.isNotEmpty) arguments['text'] = payload.text;
    if (payload.png != null && payload.png!.isNotEmpty) {
      arguments['png'] = payload.png!;
    }
    try {
      await _channel.invokeMethod<bool>('writeClipboard', arguments);
    } on PlatformException catch (error) {
      throw IosClipboardException(error.code);
    }
  }
}

final iosClipboardBridge = IosClipboardBridge();
