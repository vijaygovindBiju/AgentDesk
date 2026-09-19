import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';

class DebugScreen extends StatefulWidget {
  final ConnectionService connection;
  final AgentDeskState state;
  final ClientMetrics metrics;

  const DebugScreen({
    super.key,
    required this.connection,
    required this.state,
    required this.metrics,
  });

  @override
  State<DebugScreen> createState() => _DebugScreenState();
}

class _DebugScreenState extends State<DebugScreen> {
  Map<String, int>? _daemonMetrics;
  bool _isLoadingMetrics = false;
  String? _metricsError;

  @override
  void initState() {
    super.initState();
    if (widget.connection.status == ConnectionStatus.connected) {
      _fetchDaemonMetrics();
    }
  }

  Future<void> _fetchDaemonMetrics() async {
    setState(() {
      _isLoadingMetrics = true;
      _metricsError = null;
    });

    try {
      final metrics = await widget.connection.getMetrics();
      if (!mounted) return;
      setState(() {
        _daemonMetrics = metrics;
        _isLoadingMetrics = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _isLoadingMetrics = false;
        _metricsError = e.toString();
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final welcome = widget.state.welcome;
    final isInsecureDev = welcome?.transport == TransportMode.insecureDev;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Debug & Metrics'),
        actions: [
          IconButton(
            icon: const Icon(Icons.refresh),
            tooltip: 'Refresh metrics',
            onPressed: widget.connection.status == ConnectionStatus.connected
                ? _fetchDaemonMetrics
                : null,
          ),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          if (isInsecureDev)
            Container(
              key: const Key('insecure_dev_banner'),
              margin: const EdgeInsets.only(bottom: 16),
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: Colors.red.shade100,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: Colors.red.shade700, width: 2),
              ),
              child: Row(
                children: [
                  Icon(Icons.warning, color: Colors.red.shade900),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Text(
                      'INSECURE DEVELOPMENT MODE\n'
                      'Plain ws:// loopback transport. Not encrypted with TLS.',
                      style: TextStyle(
                        color: Colors.red.shade900,
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                ],
              ),
            ),

          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Connection Status',
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const Divider(),
                  _row('Status', widget.connection.status.name),
                  _row('Server URL', widget.connection.url),
                  _row('Device ID', widget.connection.deviceId),
                  if (widget.connection.lastError != null)
                    _row('Last Error', widget.connection.lastError!,
                        textColor: Colors.red),
                  if (welcome != null) ...[
                    _row('Daemon Version', welcome.daemonVersion),
                    _row('Schema Version', '${welcome.schemaVersion}'),
                    _row('Pipeline Mode', welcome.pipelineMode.wireName),
                    _row('Transport', welcome.transport.wireName),
                    _row('Server Time', welcome.serverTime.toIso8601String()),
                  ],
                ],
              ),
            ),
          ),

          const SizedBox(height: 16),

          // Client Counters
          ListenableBuilder(
            listenable: widget.metrics,
            builder: (context, _) {
              return Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        children: [
                          Text(
                            'Client Metrics',
                            style: Theme.of(context).textTheme.titleMedium,
                          ),
                          TextButton(
                            onPressed: widget.metrics.reset,
                            child: const Text('Reset'),
                          ),
                        ],
                      ),
                      const Divider(),
                      _row('Summaries Rendered',
                          '${widget.metrics.summariesRendered}'),
                      _row('Taps', '${widget.metrics.taps}'),
                      _row('Log Pages Requested',
                          '${widget.metrics.logPagesRequested}'),
                    ],
                  ),
                ),
              );
            },
          ),

          const SizedBox(height: 16),

          // Laptop Daemon Metrics
          Card(
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    mainAxisAlignment: MainAxisAlignment.spaceBetween,
                    children: [
                      Text(
                        'Daemon Metrics',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      if (_isLoadingMetrics)
                        const SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      else
                        TextButton(
                          onPressed: widget.connection.status ==
                                  ConnectionStatus.connected
                              ? _fetchDaemonMetrics
                              : null,
                          child: const Text('Fetch'),
                        ),
                    ],
                  ),
                  const Divider(),
                  if (_metricsError != null)
                    Text(
                      'Failed to load: $_metricsError',
                      style: const TextStyle(color: Colors.red),
                    )
                  else if (_daemonMetrics == null)
                    const Text('Not fetched yet')
                  else
                    ..._daemonMetrics!.entries.map(
                      (e) => _row(e.key, '${e.value}'),
                    ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _row(String label, String value, {Color? textColor}) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4.0),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            flex: 2,
            child: Text(
              label,
              style: const TextStyle(color: Colors.grey, fontSize: 13),
            ),
          ),
          Expanded(
            flex: 3,
            child: Text(
              value,
              style: TextStyle(
                fontWeight: FontWeight.w500,
                color: textColor,
                fontSize: 13,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
