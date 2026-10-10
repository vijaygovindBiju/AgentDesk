import 'dart:async';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'settings_screen.dart';
import 'qr_scan_screen.dart';
import 'brand_logo.dart';
import 'agent_control_overlay.dart';
import 'log_viewer_screen.dart';
import 'app_theme.dart';

class HomeScreen extends StatelessWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;
  final VoidCallback? onShowIntroduction;
  final void Function(int index, {String? prompt})? onNavigateToTab;

  const HomeScreen({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
    this.onShowIntroduction,
    this.onNavigateToTab,
  });


  void _openQrScanner(BuildContext context) {
    metrics.recordTap();
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => QRScanScreen(connection: connection),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: Listenable.merge([state, connection]),
      builder: (context, _) {
        final entries = state.entries;
        if (entries.isNotEmpty) {
          WidgetsBinding.instance.addPostFrameCallback((_) {
            metrics.recordSummaryRendered(entries.length);
          });
        }

        final status = connection.status;
        final isConnected = status == ConnectionStatus.connected;

        return Scaffold(
          appBar: AppBar(
            titleSpacing: 12,
            title: const BrandLogo(size: 22, showWordmark: true),
            actions: [
              IconButton(
                key: const Key('home_scan_qr_button'),
                padding: const EdgeInsets.all(6),
                constraints: const BoxConstraints(),
                icon: const Icon(Icons.qr_code_scanner, size: 20),
                tooltip: 'Scan QR Code to Pair',
                onPressed: () => _openQrScanner(context),
              ),
              const SizedBox(width: 4),
              // Connection status indicator chip
              InkWell(
                onTap: () {
                  metrics.recordTap();
                  Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => SettingsScreen(
                        connection: connection,
                        onShowIntroduction: onShowIntroduction,
                      ),
                    ),
                  );
                },
                borderRadius: BorderRadius.circular(16),
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 6.0,
                    vertical: 4.0,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Container(
                        key: const Key('status_indicator_dot'),
                        width: 8,
                        height: 8,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: isConnected
                              ? Colors.greenAccent.shade700
                              : status == ConnectionStatus.configurationRequired
                              ? Colors.orange
                              : status == ConnectionStatus.reconnecting
                              ? Colors.orange
                              : Colors.red,
                        ),
                      ),
                      const SizedBox(width: 5),
                      Text(status.name, style: const TextStyle(fontSize: 11)),
                    ],
                  ),
                ),
              ),
              const SizedBox(width: 4),
              // Agent Control & Quota Pill
              IconButton(
                key: const Key('home_agent_control_button'),
                padding: const EdgeInsets.symmetric(horizontal: 4),
                constraints: const BoxConstraints(),
                icon: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 3),
                  decoration: BoxDecoration(
                    color: AppTheme.primaryEmerald.withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(12),
                    border: Border.all(
                      color: AppTheme.primaryEmerald.withValues(alpha: 0.35),
                    ),
                  ),
                  child: const Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(Icons.tune, size: 13, color: AppTheme.primaryEmerald),
                      SizedBox(width: 4),
                      Text(
                        'Controls',
                        style: TextStyle(
                          fontSize: 11,
                          fontWeight: FontWeight.bold,
                          color: AppTheme.primaryEmerald,
                        ),
                      ),
                    ],
                  ),
                ),
                tooltip: 'Agent Controls',
                onPressed: () {
                  metrics.recordTap();
                  AgentControlOverlay.show(context, connection: connection);
                },
              ),
              const SizedBox(width: 4),
              IconButton(
                key: const Key('settings_button'),
                padding: const EdgeInsets.all(6),
                constraints: const BoxConstraints(),
                icon: const Icon(Icons.settings_outlined, size: 20),
                tooltip: 'Settings',
                onPressed: () {
                  metrics.recordTap();
                  Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => SettingsScreen(
                        connection: connection,
                        onShowIntroduction: onShowIntroduction,
                      ),
                    ),
                  );
                },
              ),
              const SizedBox(width: 8),
            ],
          ),
          body: SelectionArea(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(12, 6, 12, 12),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  if (!isConnected)
                    Padding(
                      padding: const EdgeInsets.only(bottom: 8.0),
                      child: Container(
                        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                        decoration: BoxDecoration(
                          color: Theme.of(context).colorScheme.primaryContainer.withValues(alpha: 0.7),
                          borderRadius: BorderRadius.circular(12),
                          border: Border.all(
                            color: Theme.of(context).colorScheme.primary.withValues(alpha: 0.3),
                          ),
                        ),
                        child: Row(
                          children: [
                            Icon(
                              Icons.qr_code_scanner,
                              size: 24,
                              color: Theme.of(context).colorScheme.primary,
                            ),
                            const SizedBox(width: 12),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  Text(
                                    'Connect to Laptop',
                                    style: Theme.of(context).textTheme.titleSmall?.copyWith(
                                          fontWeight: FontWeight.bold,
                                        ),
                                  ),
                                  Text(
                                    'Run "agentdesk pair" on laptop to scan',
                                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                                          fontSize: 11,
                                        ),
                                  ),
                                ],
                              ),
                            ),
                            FilledButton(
                              key: const Key('home_banner_scan_button'),
                              style: FilledButton.styleFrom(
                                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                                minimumSize: Size.zero,
                                tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                              ),
                              onPressed: () => _openQrScanner(context),
                              child: const Text('Scan QR', style: TextStyle(fontSize: 12, fontWeight: FontWeight.bold)),
                            ),
                          ],
                        ),
                      ),
                    ),
                  Expanded(
                    child: LiveTerminalCard(
                      state: state,
                      connection: connection,
                      metrics: metrics,
                      onNavigateToTab: onNavigateToTab,
                    ),
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }


}

class LiveTerminalCard extends StatefulWidget {
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;
  final void Function(int index, {String? prompt})? onNavigateToTab;

  const LiveTerminalCard({
    super.key,
    required this.state,
    required this.connection,
    required this.metrics,
    this.onNavigateToTab,
  });

  @override
  State<LiveTerminalCard> createState() => _LiveTerminalCardState();
}

class _LiveTerminalCardState extends State<LiveTerminalCard> {
  final ScrollController _scrollController = ScrollController();
  List<LogLine> _logLines = [];
  Timer? _pollingTimer;
  String? _activeEventId;
  bool _userScrolledUp = false;

  @override
  void initState() {
    super.initState();
    _resolveActiveEventAndFetch();
    widget.state.addListener(_onStateChange);

    // Only poll in non-test environments to prevent pumpAndSettle timeouts
    if (!WidgetsBinding.instance.runtimeType.toString().contains('Test')) {
      _pollingTimer = Timer.periodic(const Duration(seconds: 2), (_) {
        if (mounted && widget.connection.status == ConnectionStatus.connected) {
          _fetchLatestLogs(silent: true);
        }
      });
    }
  }

  @override
  void dispose() {
    _pollingTimer?.cancel();
    widget.state.removeListener(_onStateChange);
    _scrollController.dispose();
    super.dispose();
  }

  void _onStateChange() {
    if (!mounted) return;
    _resolveActiveEventAndFetch();
  }

  void _resolveActiveEventAndFetch() {
    final isWorking = widget.state.activeWorkingCount > 0;
    String? targetId;

    if (isWorking) {
      final workingEntries = widget.state.entriesForTier(3);
      if (workingEntries.isNotEmpty) {
        final sorted = workingEntries.toList()
          ..sort((a, b) {
            final evA = widget.state.eventFor(a.eventId);
            final evB = widget.state.eventFor(b.eventId);
            final seqA = evA?.seq ?? a.seq;
            final seqB = evB?.seq ?? b.seq;
            return seqB.compareTo(seqA);
          });
        targetId = sorted.first.eventId;
      }
    } else if (widget.state.pendingRequests.isNotEmpty) {
      final sorted = widget.state.pendingRequests.toList()
        ..sort((a, b) {
          final evA = widget.state.eventFor(a.eventId);
          final evB = widget.state.eventFor(b.eventId);
          final seqA = evA?.seq ?? a.seq;
          final seqB = evB?.seq ?? b.seq;
          return seqB.compareTo(seqA);
        });
      targetId = sorted.first.eventId;
    } else {
      final completed = widget.state.entriesForTier(2);
      if (completed.isNotEmpty) {
        final sorted = completed.toList()
          ..sort((a, b) {
            final evA = widget.state.eventFor(a.eventId);
            final evB = widget.state.eventFor(b.eventId);
            final seqA = evA?.seq ?? a.seq;
            final seqB = evB?.seq ?? b.seq;
            return seqB.compareTo(seqA);
          });
        targetId = sorted.first.eventId;
      }
    }

    // Fallback: If no tiered queue entry is found, pick the most recent known event
    if (targetId == null && widget.state.events.isNotEmpty) {
      final sortedEvents = widget.state.events.values.toList()
        ..sort((a, b) => b.seq.compareTo(a.seq));
      targetId = sortedEvents.first.eventId;
    }

    if (targetId != _activeEventId) {
      _activeEventId = targetId;
      _fetchLatestLogs();
    } else if (_activeEventId != null && _logLines.isEmpty) {
      _fetchLatestLogs(silent: true);
    }
  }

  Future<void> _fetchLatestLogs({bool silent = false}) async {
    if (_activeEventId == null) return;
    try {
      final res = await widget.connection.getEventLogs(
        _activeEventId!,
        logOffsetTail,
        150,
      );
      if (!mounted) return;
      var lines = res.lines;
      if (lines.isEmpty) {
        final event = widget.state.eventFor(_activeEventId!);
        if (event != null && (event.summary.isNotEmpty || event.message.isNotEmpty)) {
          final syntheticLines = <LogLine>[];
          final prompt = event.details['prompt'] as String?;
          if (prompt != null && prompt.isNotEmpty) {
            syntheticLines.add(LogLine(offset: 0, ts: event.ts, text: '> Prompt: $prompt'));
            for (final l in prompt.split('\n')) {
              syntheticLines.add(LogLine(offset: syntheticLines.length, ts: event.ts, text: 'User: $l'));
            }
          }
          if (event.summary.isNotEmpty) {
            for (final l in event.summary.split('\n')) {
              syntheticLines.add(LogLine(offset: syntheticLines.length, ts: event.ts, text: l));
            }
          }
          if (event.message.isNotEmpty && event.message != event.summary) {
            for (final l in event.message.split('\n')) {
              syntheticLines.add(LogLine(offset: syntheticLines.length, ts: event.ts, text: l));
            }
          }
          lines = syntheticLines;
        }
      }
      setState(() {
        _logLines = lines;
      });
      _scrollToBottom();
    } catch (_) {}
  }

  void _scrollToBottom({bool force = false}) {
    if (!force && _userScrolledUp) {
      return;
    }
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_scrollController.hasClients) {
        _scrollController.animateTo(
          _scrollController.position.maxScrollExtent,
          duration: const Duration(milliseconds: 200),
          curve: Curves.easeOut,
        );
      }
    });
  }

  Color _colorForLine(String line) {
    final lower = line.toLowerCase();
    if (lower.contains('error') || lower.contains('fail') || lower.contains('fatal')) {
      return const Color(0xFFF85149); // Red
    }
    if (lower.contains('warning') || lower.contains('warn')) {
      return const Color(0xFFD29922); // Amber
    }
    if (lower.contains('ok') || lower.contains('passed') || lower.contains('success') || lower.contains('✓')) {
      return const Color(0xFF7EE787); // Emerald
    }
    if (line.startsWith('> Prompt:') ||
        line.startsWith('> User') ||
        lower.startsWith('> user selected:') ||
        lower.startsWith('> user approved') ||
        lower.startsWith('> user denied') ||
        lower.startsWith('> user input:') ||
        line.startsWith('User:')) {
      return const Color(0xFF58D68D); // Bright Mint / Emerald for user input
    }
    if (line.startsWith('Agent:') ||
        line.startsWith('●') ||
        line.startsWith('▸') ||
        lower.startsWith('executing prompt:') ||
        lower.startsWith('agent:')) {
      return const Color(0xFF79C0FF); // Light Blue / Cyan for agent text
    }
    if (line.startsWith('>') || line.startsWith('\$') || lower.contains('compiling') || lower.contains('running')) {
      return const Color(0xFF58A6FF); // Cyan / Blue
    }
    return const Color(0xFFC9D1D9); // Monospace light gray
  }

  @override
  Widget build(BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final isWorking = widget.state.activeWorkingCount > 0;
    final event = _activeEventId != null ? widget.state.eventFor(_activeEventId!) : null;

    final terminalBg = isDark ? const Color(0xFF070A0F) : const Color(0xFF161B22);
    final borderColor = isDark ? const Color(0xFF21262D) : const Color(0xFF30363D);

    return Container(
      key: const Key('home_live_terminal_card'),
      decoration: BoxDecoration(
        color: terminalBg,
        borderRadius: BorderRadius.circular(14),
        border: Border.all(
          color: isWorking ? AppTheme.primaryCyan.withValues(alpha: 0.7) : borderColor,
          width: isWorking ? 1.5 : 1.0,
        ),
        boxShadow: [
          BoxShadow(
            color: Colors.black.withValues(alpha: 0.45),
            blurRadius: 10,
            offset: const Offset(0, 4),
          ),
          if (isWorking)
            BoxShadow(
              color: AppTheme.primaryCyan.withValues(alpha: 0.18),
              blurRadius: 16,
              spreadRadius: 2,
            ),
        ],
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Title bar with window controls & actions
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
            decoration: BoxDecoration(
              color: isDark ? const Color(0xFF0D1117) : const Color(0xFF21262D),
              borderRadius: const BorderRadius.vertical(top: Radius.circular(13)),
              border: Border(bottom: BorderSide(color: borderColor)),
            ),
            child: Row(
              children: [
                // Terminal header title
                const Icon(
                  Icons.terminal,
                  size: 16,
                  color: Color(0xFF8B949E),
                ),
                const SizedBox(width: 8),
                const Text(
                  'TERMINAL STREAM',
                  style: TextStyle(
                    fontFamily: 'monospace',
                    fontSize: 11,
                    fontWeight: FontWeight.w700,
                    letterSpacing: 0.8,
                    color: Color(0xFF8B949E),
                  ),
                ),
                const Spacer(),
                // Live Stream Indicator
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                  decoration: BoxDecoration(
                    color: (isWorking ? AppTheme.primaryCyan : AppTheme.primaryEmerald).withValues(alpha: 0.15),
                    borderRadius: BorderRadius.circular(6),
                    border: Border.all(
                      color: (isWorking ? AppTheme.primaryCyan : AppTheme.primaryEmerald).withValues(alpha: 0.4),
                    ),
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Container(
                        width: 6,
                        height: 6,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: isWorking ? AppTheme.primaryCyan : AppTheme.primaryEmerald,
                        ),
                      ),
                      const SizedBox(width: 4),
                      Text(
                        isWorking ? 'STREAMING' : 'LIVE',
                        style: TextStyle(
                          fontSize: 9,
                          fontWeight: FontWeight.w800,
                          fontFamily: 'monospace',
                          letterSpacing: 0.5,
                          color: isWorking ? AppTheme.primaryCyan : AppTheme.primaryEmerald,
                        ),
                      ),
                    ],
                  ),
                ),
                // Copy Button
                Tooltip(
                  message: 'Copy terminal output',
                  child: InkWell(
                    borderRadius: BorderRadius.circular(6),
                    onTap: () {
                      widget.metrics.recordTap();
                      final fullText = _logLines.isNotEmpty
                          ? _logLines.map((l) => l.text).join('\n')
                          : [
                              'agentdesk-daemon v${widget.state.welcome?.daemonVersion ?? "0.1.0"} • ${widget.connection.status == ConnectionStatus.connected ? "relay connected" : "standby"}',
                              if (widget.state.welcome != null)
                                'pipeline: ${widget.state.welcome!.pipelineMode.name} • transport: ${widget.state.welcome!.transport.name}',
                              if (event != null)
                                'task: ${event.summary}',
                              if (event != null && event.message.isNotEmpty)
                                'detail: ${event.message}',
                              '> agent waiting for prompt',
                            ].join('\n');
                      Clipboard.setData(ClipboardData(text: fullText));
                      ScaffoldMessenger.of(context).showSnackBar(
                        const SnackBar(
                          content: Text('Terminal output copied to clipboard! 📋'),
                          duration: Duration(seconds: 2),
                        ),
                      );
                    },
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 7.0, vertical: 5.0),
                      child: Icon(
                        Icons.copy_rounded,
                        size: 15,
                        color: isDark ? const Color(0xFF8B949E) : Colors.grey.shade400,
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 4),
                // Fullscreen Button
                Tooltip(
                  message: 'Fullscreen viewer',
                  child: InkWell(
                    borderRadius: BorderRadius.circular(6),
                    onTap: () {
                      widget.metrics.recordTap();
                      final evId = _activeEventId ?? (widget.state.events.isNotEmpty ? widget.state.events.values.first.eventId : null);
                      if (evId != null) {
                        Navigator.of(context).push(
                          MaterialPageRoute(
                            builder: (_) => LogViewerScreen(
                              eventId: evId,
                              connection: widget.connection,
                              metrics: widget.metrics,
                            ),
                          ),
                        );
                      }
                    },
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 7.0, vertical: 5.0),
                      child: Icon(
                        Icons.open_in_full,
                        size: 15,
                        color: isDark ? const Color(0xFF8B949E) : Colors.grey.shade400,
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
          // Terminal Output Stream Body
          Expanded(
            child: SelectionContainer.disabled(
              child: Container(
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
                child: _logLines.isNotEmpty
                    ? Stack(
                        children: [
                          NotificationListener<ScrollNotification>(
                            onNotification: (notification) {
                              if (_scrollController.hasClients) {
                                final max = _scrollController.position.maxScrollExtent;
                                final current = _scrollController.position.pixels;
                                final isScrolledUp = (max - current) > 15;
                                if (isScrolledUp != _userScrolledUp) {
                                  setState(() {
                                    _userScrolledUp = isScrolledUp;
                                  });
                                }
                              }
                              return false;
                            },
                            child: Scrollbar(
                              controller: _scrollController,
                              thumbVisibility: _userScrolledUp,
                              child: ListView.builder(
                                key: const Key('home_live_terminal_list'),
                                controller: _scrollController,
                                physics: const AlwaysScrollableScrollPhysics(),
                                itemCount: _logLines.length + 1,
                                itemBuilder: (context, index) {
                                  if (index == _logLines.length) {
                                    return Padding(
                                      padding: const EdgeInsets.only(top: 4),
                                      child: Row(
                                        children: [
                                          Text(
                                            isWorking ? '> executing... ' : '> ready ',
                                            style: const TextStyle(
                                              fontFamily: 'monospace',
                                              fontSize: 11,
                                              color: Color(0xFF6E7681),
                                            ),
                                          ),
                                          const _BlinkingCursor(),
                                        ],
                                      ),
                                    );
                                  }
                                  final line = _logLines[index].text;
                                  final isUserInput = line.startsWith('> Prompt:') ||
                                      line.startsWith('> User') ||
                                      line.toLowerCase().startsWith('> user selected:') ||
                                      line.toLowerCase().startsWith('> user approved') ||
                                      line.toLowerCase().startsWith('> user denied') ||
                                      line.toLowerCase().startsWith('> user input:') ||
                                      line.startsWith('User:');
                                  final isAgentText = line.startsWith('Agent:') ||
                                      line.startsWith('●') ||
                                      line.startsWith('▸') ||
                                      line.toLowerCase().startsWith('executing prompt:');
                                  return InkWell(
                                    borderRadius: BorderRadius.circular(4),
                                    onLongPress: () {
                                      Clipboard.setData(ClipboardData(text: line));
                                      ScaffoldMessenger.of(context).showSnackBar(
                                        SnackBar(
                                          content: Text('Copied line to clipboard! 📋\n"$line"'),
                                          duration: const Duration(seconds: 2),
                                        ),
                                      );
                                    },
                                    child: Padding(
                                      padding: const EdgeInsets.only(bottom: 2),
                                      child: Text(
                                        line,
                                        style: TextStyle(
                                          fontFamily: 'monospace',
                                          fontSize: 11,
                                          fontWeight: (isUserInput || isAgentText) ? FontWeight.w700 : FontWeight.normal,
                                          color: _colorForLine(line),
                                          height: 1.35,
                                        ),
                                      ),
                                    ),
                                  );
                                },
                              ),
                            ),
                          ),
                          if (_userScrolledUp)
                            Positioned(
                              bottom: 4,
                              right: 4,
                              child: Material(
                                color: isDark ? const Color(0xFF21262D) : const Color(0xFF30363D),
                                borderRadius: BorderRadius.circular(14),
                                elevation: 4,
                                child: InkWell(
                                  borderRadius: BorderRadius.circular(14),
                                  onTap: () {
                                    setState(() {
                                      _userScrolledUp = false;
                                    });
                                    _scrollToBottom(force: true);
                                  },
                                  child: Container(
                                    padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 5),
                                    child: Row(
                                      mainAxisSize: MainAxisSize.min,
                                      children: [
                                        Icon(
                                          Icons.arrow_downward_rounded,
                                          size: 13,
                                          color: AppTheme.primaryCyan,
                                        ),
                                        const SizedBox(width: 4),
                                        Text(
                                          'Scroll to bottom',
                                          style: TextStyle(
                                            fontFamily: 'monospace',
                                            fontSize: 10,
                                            fontWeight: FontWeight.bold,
                                            color: AppTheme.primaryCyan,
                                          ),
                                        ),
                                      ],
                                    ),
                                  ),
                                ),
                              ),
                            ),
                        ],
                      )
                  : SingleChildScrollView(
                      physics: const AlwaysScrollableScrollPhysics(),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          _terminalLine(
                            '>',
                            'agentdesk-daemon v${widget.state.welcome?.daemonVersion ?? "0.1.0"} • ${widget.connection.status == ConnectionStatus.connected ? "relay connected" : "standby"}',
                            const Color(0xFF6E7681),
                          ),
                          const SizedBox(height: 3),
                          if (widget.state.welcome != null) ...[
                            _terminalLine(
                              '>',
                              'pipeline: ${widget.state.welcome!.pipelineMode.name} • transport: ${widget.state.welcome!.transport.name}',
                              const Color(0xFF58A6FF),
                            ),
                            const SizedBox(height: 3),
                          ],
                          if (event != null) ...[
                            _terminalLine(
                              isWorking ? '[RUN]' : (event.category == Category.request ? '[BLOCK]' : '[DONE]'),
                              'task: ${event.summary}',
                              isWorking ? const Color(0xFF79C0FF) : const Color(0xFF7EE787),
                              isBold: true,
                            ),
                            const SizedBox(height: 3),
                            if (event.message.isNotEmpty)
                              _terminalLine('[OUT]', 'detail: ${event.message}', const Color(0xFFE6EDF3)),
                            const SizedBox(height: 3),
                          ],
                          const Row(
                            children: [
                              Text(
                                '> agent waiting for prompt ',
                                style: TextStyle(
                                  fontFamily: 'monospace',
                                  fontSize: 11,
                                  color: Color(0xFF8B949E),
                                ),
                              ),
                              _BlinkingCursor(),
                            ],
                          ),
                        ],
                      ),
                    ),
                ),
              ),
            ),
          // Action Buttons Bar
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 7),
            decoration: BoxDecoration(
              color: isDark ? const Color(0xFF0B0E14) : const Color(0xFF161B22),
              borderRadius: const BorderRadius.vertical(bottom: Radius.circular(13)),
              border: Border(top: BorderSide(color: borderColor)),
            ),
            child: Row(
              children: [
                _terminalActionBtn(
                  icon: Icons.play_circle_outline,
                  label: 'Run Tests',
                  onTap: () {
                    widget.metrics.recordTap();
                    widget.onNavigateToTab?.call(
                      4,
                      prompt: 'Run the complete test suite and verify all unit tests pass.',
                    );
                  },
                ),
                const SizedBox(width: 8),
                _terminalActionBtn(
                  icon: Icons.add_circle_outline,
                  label: 'New Task',
                  onTap: () {
                    widget.metrics.recordTap();
                    widget.onNavigateToTab?.call(4);
                  },
                ),
                const SizedBox(width: 8),
                _terminalActionBtn(
                  icon: Icons.tune,
                  label: 'Controls',
                  onTap: () {
                    widget.metrics.recordTap();
                    AgentControlOverlay.show(context, connection: widget.connection);
                  },
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _terminalLine(String prefix, String content, Color color, {bool isBold = false}) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          '$prefix ',
          style: TextStyle(
            fontFamily: 'monospace',
            fontSize: 11,
            fontWeight: FontWeight.bold,
            color: color.withValues(alpha: 0.7),
          ),
        ),
        Expanded(
          child: Text(
            content,
            maxLines: 2,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              fontFamily: 'monospace',
              fontSize: 11,
              fontWeight: isBold ? FontWeight.bold : FontWeight.w500,
              color: color,
            ),
          ),
        ),
      ],
    );
  }

  Widget _terminalActionBtn({
    required IconData icon,
    required String label,
    required VoidCallback onTap,
  }) {
    return Expanded(
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(8),
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 8),
          decoration: BoxDecoration(
            color: const Color(0xFF161B22),
            borderRadius: BorderRadius.circular(8),
            border: Border.all(color: const Color(0xFF30363D)),
          ),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Icon(icon, size: 13, color: AppTheme.primaryEmerald),
              const SizedBox(width: 5),
              Flexible(
                child: Text(
                  label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    color: Color(0xFFC9D1D9),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _BlinkingCursor extends StatefulWidget {
  const _BlinkingCursor();

  @override
  State<_BlinkingCursor> createState() => _BlinkingCursorState();
}

class _BlinkingCursorState extends State<_BlinkingCursor>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller;

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 550),
    );
    if (!WidgetsBinding.instance.runtimeType.toString().contains('Test')) {
      _controller.repeat(reverse: true);
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return FadeTransition(
      opacity: _controller,
      child: const Text(
        '█',
        style: TextStyle(
          color: AppTheme.primaryEmerald,
          fontSize: 12,
          fontWeight: FontWeight.bold,
          fontFamily: 'monospace',
        ),
      ),
    );
  }
}
