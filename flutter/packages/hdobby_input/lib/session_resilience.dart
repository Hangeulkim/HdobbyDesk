/// Keeps an explicitly saved preference, while making active mobile remote
/// control usable on a fresh install. Desktop keeps its existing opt-in default.
bool keepAwakeForOutgoingSession({
  required bool isMobile,
  required String storedValue,
}) {
  if (storedValue == 'Y') return true;
  if (storedValue == 'N') return false;
  return isMobile;
}

/// Finite exponential retry schedule for transient session failures.
///
/// A finite budget avoids retrying forever when a host is deliberately offline,
/// while the 30-second cap tolerates short mobile network transitions.
class SessionReconnectBackoff {
  SessionReconnectBackoff({
    this.maximumElapsed = const Duration(minutes: 5),
    this.maximumAttempts = 14,
  });

  final Duration maximumElapsed;
  final int maximumAttempts;
  DateTime? _startedAt;
  int _attempts = 0;

  int get attempts => _attempts;

  Duration? nextDelay(DateTime now) {
    _startedAt ??= now;
    if (_attempts >= maximumAttempts ||
        now.difference(_startedAt!) >= maximumElapsed) {
      return null;
    }
    final seconds = _attempts >= 5 ? 30 : 1 << _attempts;
    final delay = Duration(seconds: seconds);
    if (now.difference(_startedAt!) + delay > maximumElapsed) {
      return null;
    }
    _attempts++;
    return delay;
  }

  void reset() {
    _startedAt = null;
    _attempts = 0;
  }
}
