enum WindowsSessionKind { console, rdp, ica, other }

enum WindowsRemoteSessionScope { physicalConsole, separateDesktop, unknown }

WindowsRemoteSessionScope parseWindowsRemoteSessionScope(Object? value) {
  return switch (value) {
    'physical_console' => WindowsRemoteSessionScope.physicalConsole,
    'separate_desktop' => WindowsRemoteSessionScope.separateDesktop,
    _ => WindowsRemoteSessionScope.unknown,
  };
}

class WindowsSessionChoice {
  const WindowsSessionChoice({required this.sid, required this.name});

  final String sid;
  final String name;

  WindowsSessionKind get kind {
    final normalized = name.trim().toLowerCase();
    if (normalized.startsWith('console')) return WindowsSessionKind.console;
    if (normalized.startsWith('rdp')) return WindowsSessionKind.rdp;
    if (normalized.startsWith('ica')) return WindowsSessionKind.ica;
    return WindowsSessionKind.other;
  }

  bool get mirrorsPhysicalMonitor => kind == WindowsSessionKind.console;

  String displayLabel({
    required String physicalMonitorLabel,
    required String separateWindowsDesktopLabel,
    required String separateRemoteDesktopLabel,
  }) {
    final base = name.trim().isEmpty ? sid : name.trim();
    final suffix = switch (kind) {
      WindowsSessionKind.console => physicalMonitorLabel,
      WindowsSessionKind.rdp => separateWindowsDesktopLabel,
      WindowsSessionKind.ica => separateRemoteDesktopLabel,
      WindowsSessionKind.other => '',
    };
    return suffix.isEmpty ? base : '$base · $suffix';
  }
}

WindowsSessionChoice chooseInitialWindowsSession(
  List<WindowsSessionChoice> choices,
  String savedSid,
) {
  if (choices.isEmpty) {
    throw ArgumentError.value(choices, 'choices', 'must not be empty');
  }
  final console = choices.firstWhere(
    (choice) => choice.mirrorsPhysicalMonitor,
    orElse: () => choices.first,
  );
  final hasConsole = console.mirrorsPhysicalMonitor;
  for (final choice in choices) {
    if (choice.sid == savedSid &&
        (choice.mirrorsPhysicalMonitor || !hasConsole)) {
      return choice;
    }
  }
  return console;
}

/// A saved separate desktop must not silently win over an available physical
/// console. If no console exists, reusing the only reachable session keeps
/// unattended reconnection working.
bool canAutoConnectSavedWindowsSession(
  List<WindowsSessionChoice> choices,
  String savedSid,
) {
  WindowsSessionChoice? saved;
  for (final choice in choices) {
    if (choice.sid == savedSid) {
      saved = choice;
      break;
    }
  }
  if (saved == null) return false;
  if (saved.mirrorsPhysicalMonitor) return true;
  return !choices.any((choice) => choice.mirrorsPhysicalMonitor);
}

/// Reattach a previously chosen physical Console when Windows changes its
/// numeric session ID after an RDP/Console handoff. Never guess among multiple
/// Consoles or silently replace a saved separate desktop with Console.
WindowsSessionChoice? findAutomaticWindowsSession(
  List<WindowsSessionChoice> choices,
  String savedSid,
  String savedKind,
) {
  if (canAutoConnectSavedWindowsSession(choices, savedSid)) {
    for (final choice in choices) {
      if (choice.sid == savedSid) return choice;
    }
  }
  if (savedKind == WindowsSessionKind.console.name) {
    final consoles = choices.where((choice) => choice.mirrorsPhysicalMonitor);
    if (consoles.length == 1) return consoles.single;
  }
  return null;
}
