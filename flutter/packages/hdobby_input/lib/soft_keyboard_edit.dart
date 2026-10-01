import 'package:flutter/widgets.dart' show StringCharacters;

/// Replaces the changed tail of a soft-keyboard buffer at the remote caret.
/// Korean composition often changes a syllable without changing text length.
class SoftKeyboardEdit {
  const SoftKeyboardEdit(this.backspaces, this.text);

  final int backspaces;
  final String text;

  factory SoftKeyboardEdit.between(String previous, String current) {
    final before = previous.characters.toList();
    final after = current.characters.toList();
    var common = 0;
    while (common < before.length &&
        common < after.length &&
        before[common] == after[common]) {
      common++;
    }
    return SoftKeyboardEdit(
        before.length - common, after.skip(common).join());
  }
}
