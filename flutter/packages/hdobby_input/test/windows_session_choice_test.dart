import 'package:flutter_test/flutter_test.dart';
import 'package:hdobby_input/windows_session_choice.dart';

void main() {
  const console = WindowsSessionChoice(sid: '1', name: 'Console:1');
  const rdp = WindowsSessionChoice(sid: '4', name: 'RDP:4');

  test('parses host-reported physical and separate Windows session scopes', () {
    expect(
      parseWindowsRemoteSessionScope('physical_console'),
      WindowsRemoteSessionScope.physicalConsole,
    );
    expect(
      parseWindowsRemoteSessionScope('separate_desktop'),
      WindowsRemoteSessionScope.separateDesktop,
    );
    expect(
      parseWindowsRemoteSessionScope('future-value'),
      WindowsRemoteSessionScope.unknown,
    );
    expect(
      parseWindowsRemoteSessionScope(null),
      WindowsRemoteSessionScope.unknown,
    );
  });
  const ica = WindowsSessionChoice(sid: '7', name: 'ICA:7');

  test('classifies and explains Windows session types', () {
    expect(console.kind, WindowsSessionKind.console);
    expect(rdp.kind, WindowsSessionKind.rdp);
    expect(ica.kind, WindowsSessionKind.ica);
    expect(
      console.displayLabel(
        physicalMonitorLabel: 'Same as physical monitor',
        separateWindowsDesktopLabel: 'Separate Windows desktop',
        separateRemoteDesktopLabel: 'Separate remote desktop',
      ),
      'Console:1 · Same as physical monitor',
    );
    expect(
      rdp.displayLabel(
        physicalMonitorLabel: 'Same as physical monitor',
        separateWindowsDesktopLabel: 'Separate Windows desktop',
        separateRemoteDesktopLabel: 'Separate remote desktop',
      ),
      'RDP:4 · Separate Windows desktop',
    );
  });

  test('defaults to the physical console when there is no saved choice', () {
    expect(chooseInitialWindowsSession([rdp, console], '').sid, console.sid);
  });

  test('keeps the console as initial when a saved RDP session also exists', () {
    expect(
      chooseInitialWindowsSession([console, rdp], rdp.sid).sid,
      console.sid,
    );
    expect(
      chooseInitialWindowsSession([console, rdp], console.sid).sid,
      console.sid,
    );
    expect(chooseInitialWindowsSession([rdp, ica], rdp.sid).sid, rdp.sid);
  });

  test('only auto connects a separate desktop when no console is available',
      () {
    expect(
        canAutoConnectSavedWindowsSession([console, rdp], console.sid), isTrue);
    expect(canAutoConnectSavedWindowsSession([console, rdp], rdp.sid), isFalse);
    expect(canAutoConnectSavedWindowsSession([rdp, ica], rdp.sid), isTrue);
    expect(
        canAutoConnectSavedWindowsSession([console, rdp], 'missing'), isFalse);
  });

  test('reattaches one saved Console when its numeric ID changes', () {
    const newConsole = WindowsSessionChoice(sid: '9', name: 'Console:9');
    expect(
        findAutomaticWindowsSession(
            [rdp, newConsole], console.sid, 'console'),
        newConsole);
    expect(findAutomaticWindowsSession([rdp], console.sid, 'console'), isNull);
    expect(
        findAutomaticWindowsSession(
            [console, newConsole], 'missing', 'console'),
        isNull);
    expect(findAutomaticWindowsSession([newConsole], rdp.sid, 'rdp'), isNull);
  });

  test('rejects an empty choice list', () {
    expect(
        () => chooseInitialWindowsSession(const [], ''), throwsArgumentError);
  });
}
