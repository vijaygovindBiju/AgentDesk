import 'package:flutter/foundation.dart';

class ClientMetrics extends ChangeNotifier {
  int _summariesRendered = 0;
  int _taps = 0;
  int _logPagesRequested = 0;

  int get summariesRendered => _summariesRendered;
  int get taps => _taps;
  int get logPagesRequested => _logPagesRequested;

  void recordSummaryRendered([int count = 1]) {
    _summariesRendered += count;
    notifyListeners();
  }

  void recordTap() {
    _taps++;
    notifyListeners();
  }

  void recordLogPageRequested() {
    _logPagesRequested++;
    notifyListeners();
  }

  void reset() {
    _summariesRendered = 0;
    _taps = 0;
    _logPagesRequested = 0;
    notifyListeners();
  }
}
