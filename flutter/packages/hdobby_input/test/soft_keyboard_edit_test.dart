import 'package:flutter/widgets.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/hdobby_input.dart';

void main() {
  void replay(List<String> edits) {
    final sentinel = '1' * 1024;
    var previous = '';
    var remote = 'existing: ';
    for (final value in edits) {
      final edit = SoftKeyboardEdit.between(sentinel + previous, sentinel + value);
      final characters = remote.characters.toList();
      remote = characters.take(characters.length - edit.backspaces).join() +
          edit.text;
      expect(remote, 'existing: $value', reason: '$previous → $value');
      previous = value;
    }
  }

  test('Samsung Korean composition replaces syllables of the same length', () {
    replay(['ㅎ', '하', '한', '한ㄱ', '한그', '한글', '한글 ']);
  });

  test('composition backspace and cancellation preserve preceding text', () {
    replay(['한', '하', 'ㅎ', '', 'a', 'ab', 'abc123', 'abc', '']);
  });

  test('equal-length replacement and autocorrection replace the full tail', () {
    replay(['hellp', 'hello', 'hello world', 'hello work', 'hello\nnext']);
  });

  test('emoji and combining marks do not delete UTF-16 fragments', () {
    replay(['😀', '', '👩🏽‍💻', '👩🏽‍💻 한글', '👩🏽‍💻', '', 'e', 'e\u0301', '']);
    final edit = SoftKeyboardEdit.between('👩🏽‍💻', '');
    expect(edit.backspaces, 1);
  });

  test('unchanged editing value produces no remote input', () {
    final edit = SoftKeyboardEdit.between('한글', '한글');
    expect(edit.backspaces, 0);
    expect(edit.text, isEmpty);
  });
}
