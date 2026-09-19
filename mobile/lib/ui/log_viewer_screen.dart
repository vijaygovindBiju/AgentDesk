import 'dart:math';
import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';

class LogViewerScreen extends StatefulWidget {
  final String eventId;
  final ConnectionService connection;
  final ClientMetrics metrics;

  const LogViewerScreen({
    super.key,
    required this.eventId,
    required this.connection,
    required this.metrics,
  });

  @override
  State<LogViewerScreen> createState() => _LogViewerScreenState();
}

class _LogViewerScreenState extends State<LogViewerScreen> {
  static const int pageSize = 100;

  final List<LogLine> _lines = [];
  int _firstOffset = -1;
  int _totalAvailable = 0;
  bool _evicted = false;
  bool _isLoading = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _loadTail();
  }

  Future<void> _loadTail() async {
    setState(() {
      _isLoading = true;
      _error = null;
    });
    widget.metrics.recordLogPageRequested();

    try {
      final res = await widget.connection.getEventLogs(
        widget.eventId,
        logOffsetTail,
        pageSize,
      );
      if (!mounted) return;
      setState(() {
        _lines.clear();
        _lines.addAll(res.lines);
        _firstOffset = res.offset;
        _totalAvailable = res.total;
        _evicted = res.evicted;
        _isLoading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _isLoading = false;
        _error = e.toString();
      });
    }
  }

  Future<void> _loadEarlier() async {
    if (_isLoading || _firstOffset <= 0 || _evicted) return;

    final targetOffset = max(0, _firstOffset - pageSize);
    final countToFetch = _firstOffset - targetOffset;
    if (countToFetch <= 0) return;

    setState(() {
      _isLoading = true;
      _error = null;
    });
    widget.metrics.recordLogPageRequested();

    try {
      final res = await widget.connection.getEventLogs(
        widget.eventId,
        targetOffset,
        countToFetch,
      );
      if (!mounted) return;
      setState(() {
        _lines.insertAll(0, res.lines);
        _firstOffset = res.offset;
        _totalAvailable = res.total;
        _evicted = res.evicted;
        _isLoading = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _isLoading = false;
        _error = e.toString();
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final canLoadEarlier = _firstOffset > 0 && !_evicted;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Event Logs'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: 'Refresh tail',
            onPressed: _isLoading ? null : _loadTail,
          ),
        ],
      ),
      body: Column(
        children: [
          if (_isLoading) const LinearProgressIndicator(),
          if (_error != null)
            Container(
              color: Colors.red.shade100,
              padding: const EdgeInsets.all(12),
              width: double.infinity,
              child: Text(
                'Error loading logs: $_error',
                style: TextStyle(color: Colors.red.shade900),
              ),
            ),
          if (_evicted)
            Container(
              key: const Key('evicted_banner'),
              color: Colors.amber.shade100,
              padding: const EdgeInsets.all(8),
              width: double.infinity,
              child: Text(
                'Earlier logs no longer available (evicted from ring buffer)',
                textAlign: TextAlign.center,
                style: TextStyle(
                  color: Colors.amber.shade900,
                  fontWeight: FontWeight.bold,
                ),
              ),
            ),
          if (canLoadEarlier)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 4.0),
              child: TextButton.icon(
                key: const Key('load_earlier_button'),
                onPressed: _isLoading ? null : _loadEarlier,
                icon: const Icon(Icons.arrow_upward),
                label: Text('Load earlier (from offset ${_firstOffset - 1})'),
              ),
            ),
          Expanded(
            child: _lines.isEmpty && !_isLoading
                ? const Center(child: Text('No log lines recorded'))
                : ListView.builder(
                    itemCount: _lines.length,
                    itemBuilder: (context, index) {
                      final line = _lines[index];
                      return Container(
                        padding: const EdgeInsets.symmetric(
                            horizontal: 12, vertical: 2),
                        color: index % 2 == 0
                            ? Colors.black.withValues(alpha: 0.02)
                            : Colors.transparent,
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            SizedBox(
                              width: 55,
                              child: Text(
                                '${line.offset}',
                                style: const TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 12,
                                  color: Colors.grey,
                                ),
                              ),
                            ),
                            Expanded(
                              child: Text(
                                line.text,
                                style: const TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 12,
                                ),
                              ),
                            ),
                          ],
                        ),
                      );
                    },
                  ),
          ),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            color: Theme.of(context).colorScheme.surfaceContainerHighest,
            child: Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                Text(
                  'Lines shown: ${_lines.length} / Total: $_totalAvailable',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                Text(
                  _firstOffset >= 0 ? 'Start offset: $_firstOffset' : '',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
