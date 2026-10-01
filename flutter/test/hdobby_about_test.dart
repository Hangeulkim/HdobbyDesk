import 'package:flutter_hbb/hdobby/about.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('about title uses only the configured product name', () {
    expect(
      hdobbyAboutTitle(korean: false, appName: 'HdobbyDesk'),
      'About HdobbyDesk',
    );
    expect(
      hdobbyAboutTitle(korean: true, appName: 'HdobbyDesk'),
      'HdobbyDesk 정보',
    );
  });
}
