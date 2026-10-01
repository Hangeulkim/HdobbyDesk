import 'dart:convert';

/// Public identity and an explicit destination. Never includes passwords or private keys.
class DirectConnectionCode {
  const DirectConnectionCode(this.endpoint, this.certificate);
  static const prefix = 'hdobby-connect2:';
  static const legacyPrefix = 'hdobby-connect1:';
  final String endpoint;
  final String certificate;

  static Uri parseEndpoint(String endpoint) {
    final uri = Uri.tryParse('tcp://$endpoint');
    final match = RegExp(r'^(?:\[[0-9a-fA-F:.]+\]|[^:@/?#\s\[\]]+):([0-9]+)$')
        .firstMatch(endpoint);
    final port = int.tryParse(match?.group(1) ?? '');
    if (endpoint.length > 260 ||
        RegExp(r'\s').hasMatch(endpoint) ||
        uri == null ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty ||
        uri.path.isNotEmpty ||
        uri.hasQuery ||
        uri.hasFragment ||
        port == null ||
        port < 1 ||
        port > 65535) {
      throw const FormatException('Invalid direct endpoint');
    }
    return uri;
  }

  String encode() {
    parseEndpoint(endpoint);
    certificateBytes(certificate);
    return prefix +
        _base32Encode(utf8.encode(jsonEncode({
          'endpoint': endpoint,
          'certificate': certificate,
        })));
  }

  static DirectConnectionCode decode(String value) {
    if (value.length > 32768 ||
        (!value.startsWith(prefix) && !value.startsWith(legacyPrefix))) {
      throw const FormatException('Invalid connection code');
    }
    final body = value.substring(
        value.startsWith(prefix) ? prefix.length : legacyPrefix.length);
    final bytes = value.startsWith(prefix)
        ? _base32Decode(body, allowSeparators: false)
        : base64Url.decode(base64Url.normalize(body));
    final data = jsonDecode(utf8.decode(bytes));
    if (data is! Map ||
        data.length != 2 ||
        data['endpoint'] is! String ||
        data['certificate'] is! String) {
      throw const FormatException('Invalid connection code fields');
    }
    final result = DirectConnectionCode(data['endpoint'], data['certificate']);
    parseEndpoint(result.endpoint);
    certificateBytes(result.certificate);
    return result;
  }

  /// Decode the public certificate from the current no-padding Base32 form or
  /// the legacy Base64 form. This is display encoding only; TLS authenticates
  /// the certificate bytes after decoding.
  static List<int> certificateBytes(String value) {
    if (value.length > 16384) {
      throw const FormatException('Invalid certificate code');
    }
    List<int> result;
    if (value.toLowerCase().startsWith('hdobby2:')) {
      result = _base32Decode(value.substring('hdobby2:'.length),
          allowSeparators: true);
    } else if (value.startsWith('hdobby1:')) {
      result = base64.decode(value.substring('hdobby1:'.length));
    } else {
      throw const FormatException('Invalid certificate code');
    }
    if (result.isEmpty || result.length > 4096) {
      throw const FormatException('Invalid certificate code');
    }
    return result;
  }

  static const _base32Alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';

  static String _base32Encode(List<int> bytes) {
    var buffer = 0;
    var bits = 0;
    final output = StringBuffer();
    for (final byte in bytes) {
      buffer = (buffer << 8) | byte;
      bits += 8;
      while (bits >= 5) {
        bits -= 5;
        output.write(_base32Alphabet[(buffer >> bits) & 31]);
      }
      buffer &= (1 << bits) - 1;
    }
    if (bits > 0) output.write(_base32Alphabet[(buffer << (5 - bits)) & 31]);
    return output.toString();
  }

  static List<int> _base32Decode(String value,
      {required bool allowSeparators}) {
    var buffer = 0;
    var bits = 0;
    final output = <int>[];
    var symbols = 0;
    for (final rune in value.runes) {
      final character = String.fromCharCode(rune);
      if (allowSeparators &&
          (character == '-' || RegExp(r'\s').hasMatch(character))) {
        continue;
      }
      final index = _base32Alphabet.indexOf(character.toUpperCase());
      if (index < 0) throw const FormatException('Invalid Base32 data');
      buffer = (buffer << 5) | index;
      bits += 5;
      symbols++;
      while (bits >= 8) {
        bits -= 8;
        output.add((buffer >> bits) & 255);
      }
      buffer &= (1 << bits) - 1;
    }
    if (symbols == 0 || bits >= 5 || buffer != 0) {
      throw const FormatException('Invalid Base32 data');
    }
    return output;
  }
}

/// Use a friendly label anywhere the UI can be captured or shared.
///
/// Direct peers are addressed as `host:port`; displaying that value in a
/// window title leaks network details into screenshots and screen sharing.
/// The address remains available in the connection editor where it is needed.
String safePeerDisplayLabel({
  required String peerId,
  required String alias,
  required String hostname,
  String fallback = 'Remote device',
}) {
  var direct = false;
  try {
    DirectConnectionCode.parseEndpoint(peerId);
    direct = true;
  } on FormatException {
    // Ordinary rendezvous IDs keep their existing display behavior.
  }

  var label = alias.trim();
  final host = hostname.trim();
  if (label.isEmpty) {
    label = direct ? (host.isEmpty ? fallback : host) : peerId;
  }
  if (host.isNotEmpty &&
      !label.toLowerCase().contains(host.toLowerCase()) &&
      (!direct || alias.trim().isNotEmpty)) {
    label += '@$host';
  }
  return label;
}

enum DirectCheckState { ready, actionRequired, unverified }

class DirectHostCheck {
  const DirectHostCheck(this.id, this.title, this.detail, this.state,
      {this.action});
  final String id;
  final String title;
  final String detail;
  final DirectCheckState state;
  final String? action;
}

class DirectHostDetails {
  const DirectHostDetails(
      {required this.certificate,
      required this.endpoints,
      required this.checks,
      required this.password});
  final String certificate;
  final List<String> endpoints;
  final List<DirectHostCheck> checks;
  final String password;
}
