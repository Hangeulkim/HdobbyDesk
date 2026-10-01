import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:flutter_hbb/hdobby/ios_clipboard.dart';

Uint8List pngHeader({int width = 1, int height = 1}) {
  final bytes = Uint8List(33);
  bytes.setAll(0, const [137, 80, 78, 71, 13, 10, 26, 10]);
  ByteData.sublistView(bytes, 8, 12).setUint32(0, 13, Endian.big);
  bytes.setAll(12, 'IHDR'.codeUnits);
  ByteData.sublistView(bytes, 16, 20).setUint32(0, width, Endian.big);
  ByteData.sublistView(bytes, 20, 24).setUint32(0, height, Endian.big);
  return bytes;
}

void main() {
  test('accepts an atomic text and PNG payload', () {
    final png = pngHeader();
    final payload = parseIosClipboardPayload(<String, Object>{
      'text': '안녕',
      'png': png,
    });

    expect(payload.text, '안녕');
    expect(payload.png, same(png));
  });

  test('rejects empty and unexpected platform values', () {
    expect(
      () => parseIosClipboardPayload(<String, Object>{}),
      throwsA(isA<IosClipboardException>().having(
        (error) => error.message,
        'message',
        'Clipboard is empty',
      )),
    );
    expect(
      () => parseIosClipboardPayload(<String, Object>{'png': 'not bytes'}),
      throwsA(isA<IosClipboardException>().having(
        (error) => error.message,
        'message',
        'Clipboard format is unsupported',
      )),
    );
  });

  test('rejects invalid PNG structure and oversized dimensions', () {
    expect(isValidIosClipboardPng(Uint8List.fromList([1, 2, 3])), isFalse);
    expect(
      isValidIosClipboardPng(pngHeader(width: 8192, height: 8192)),
      isFalse,
    );
    expect(
      () => validateIosClipboardPayload(png: Uint8List.fromList([1, 2, 3])),
      throwsA(isA<IosClipboardException>()),
    );
  });

  test('text limit counts encoded bytes', () {
    final oversized = List<String>.filled(
      maxIosClipboardTextBytes ~/ 3 + 1,
      '한',
    ).join();
    expect(
      () => validateIosClipboardPayload(text: oversized),
      throwsA(isA<IosClipboardException>().having(
        (error) => error.message,
        'message',
        'Clipboard is too large',
      )),
    );
  });
}
