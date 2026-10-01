import 'dart:convert';
import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

void main() {
  test('connection code uses no-padding Base32 and preserves public data', () {
    for (final endpoint in [
      '127.0.0.1:21118',
      '[::1]:21118',
      'local.example:21118'
    ]) {
      final code = DirectConnectionCode(endpoint, 'hdobby2:MZXW6-YTB').encode();
      expect(code, startsWith(DirectConnectionCode.prefix));
      expect(code, isNot(contains('=')));
      expect(code, isNot(contains('+')));
      expect(code, isNot(contains('/')));
      final decoded = DirectConnectionCode.decode(code);
      expect(decoded.endpoint, endpoint);
      expect(decoded.certificate, 'hdobby2:MZXW6-YTB');
    }
  });

  test('current and legacy certificate encodings decode to the same bytes', () {
    expect(DirectConnectionCode.certificateBytes('hdobby2:MZXW6-YTB'),
        utf8.encode('fooba'));
    expect(DirectConnectionCode.certificateBytes('hdobby2:mzxw6ytb'),
        utf8.encode('fooba'));
    expect(DirectConnectionCode.certificateBytes('hdobby1:Zm9vYmE='),
        utf8.encode('fooba'));
  });

  test('legacy connection bundles remain readable', () {
    final payload = base64Url.encode(utf8.encode(jsonEncode({
      'endpoint': '127.0.0.1:21118',
      'certificate': 'hdobby1:Zm9vYmE=',
    })));
    final decoded = DirectConnectionCode.decode(
        DirectConnectionCode.legacyPrefix + payload);
    expect(decoded.endpoint, '127.0.0.1:21118');
    expect(decoded.certificate, 'hdobby1:Zm9vYmE=');
  });

  test('direct peer labels never expose host or port', () {
    expect(
        safePeerDisplayLabel(
            peerId: '203.0.113.8:21118', alias: '', hostname: 'workstation'),
        'workstation');
    expect(
        safePeerDisplayLabel(
            peerId: '[2001:db8::8]:21118',
            alias: 'Office PC',
            hostname: 'workstation'),
        'Office PC@workstation');
    expect(
        safePeerDisplayLabel(
            peerId: 'private.example:21118', alias: '', hostname: ''),
        'Remote device');
  });

  test('ordinary peer labels retain IDs, aliases and hostnames', () {
    expect(
        safePeerDisplayLabel(
            peerId: '123456789', alias: '', hostname: 'workstation'),
        '123456789@workstation');
    expect(
        safePeerDisplayLabel(
            peerId: '123456789', alias: 'Office PC', hostname: 'workstation'),
        'Office PC@workstation');
    expect(
        safePeerDisplayLabel(
            peerId: '123456789', alias: 'workstation', hostname: 'workstation'),
        'workstation');
  });

  test(
      'unexpected secret fields, malformed payload and unsafe addresses are rejected',
      () {
    final withPassword = DirectConnectionCode.legacyPrefix +
        base64Url.encode(utf8.encode(jsonEncode({
          'endpoint': '127.0.0.1:21118',
          'certificate': 'hdobby1:Zm9vYmE=',
          'password': 'test-only',
        })));
    expect(
        () => DirectConnectionCode.decode(withPassword), throwsFormatException);
    expect(() => DirectConnectionCode.decode('${DirectConnectionCode.prefix}!'),
        throwsFormatException);
    for (final endpoint in [
      '',
      'https://example.com',
      'user@host:21118',
      'host:0',
      'host:65536',
      'host/path',
      'host?query',
      'host#fragment',
      'host name'
    ]) {
      expect(() => DirectConnectionCode(endpoint, 'hdobby2:MZXW6-YTB').encode(),
          throwsFormatException,
          reason: endpoint);
    }
    for (final certificate in [
      '',
      'hdobby2:',
      'hdobby2:A',
      'hdobby2:MZ',
      'hdobby2:MZXW6=YTB',
      'hdobby1:!!!!',
    ]) {
      expect(
          () => DirectConnectionCode('127.0.0.1:21118', certificate).encode(),
          throwsFormatException,
          reason: certificate);
    }
  });
}
