import 'dart:math';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
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
            icon: const Icon(Icons.copy_rounded),
            tooltip: 'Copy all logs',
            onPressed: _lines.isEmpty
                ? null
                : () {
                    widget.metrics.recordTap();
                    final fullText = _lines.map((l) => l.text).join('\n');
                    Clipboard.setData(ClipboardData(text: fullText));
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                        content: Text('All logs copied to clipboard! 📋'),
                        duration: Duration(seconds: 2),
                      ),
                    );
                  },
          ),
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: 'Refresh tail',
            onPressed: _isLoading ? null : _loadTail,
          ),
        ],
      ),
      body: SelectionArea(
        child: Column(
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
                      final text = line.text;
                      final lower = text.toLowerCase();
                      final isUser = text.startsWith('> Prompt:') ||
                          text.startsWith('> User') ||
                          lower.startsWith('> user selected:') ||
                          lower.startsWith('> user approved') ||
                          lower.startsWith('> user denied') ||
                          lower.startsWith('> user input:') ||
                          text.startsWith('User:');
                      final isAgent = text.startsWith('Agent:') ||
                          text.startsWith('●') ||
                          text.startsWith('▸') ||
                          lower.startsWith('executing prompt:');
                      
                      Color textColor;
                      if (lower.contains('error') || lower.contains('fail') || lower.contains('fatal')) {
                        textColor = const Color(0xFFF85149);
                      } else if (lower.contains('warning') || lower.contains('warn')) {
                        textColor = const Color(0xFFD29922);
                      } else if (lower.contains('ok') || lower.contains('passed') || lower.contains('success') || lower.contains('✓')) {
                        textColor = const Color(0xFF7EE787);
                      } else if (isUser) {
                        textColor = const Color(0xFF58D68D);
                      } else if (isAgent) {
                        textColor = const Color(0xFF58A6FF);
                      } else if (text.startsWith('>') || text.startsWith('\$')) {
                        textColor = const Color(0xFF79C0FF);
                      } else {
                        textColor = Theme.of(context).brightness == Brightness.dark
                            ? const Color(0xFFC9D1D9)
                            : Colors.grey.shade800;
                      }

                      return InkWell(
                        onLongPress: () {
                          Clipboard.setData(ClipboardData(text: line.text));
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                              content: Text('Copied line to clipboard! 📋\n"${line.text}"'),
                              duration: const Duration(seconds: 2),
                            ),
                          );
                        },
                        child: Container(
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
                                  style: TextStyle(
                                    fontFamily: 'monospace',
                                    fontSize: 12,
                                    fontWeight: (isUser || isAgent) ? FontWeight.bold : FontWeight.normal,
                                    color: textColor,
                                  ),
                                ),
                              ),
                            ],
                          ),
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
      ),
    );
  }
}
