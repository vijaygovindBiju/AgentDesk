import 'package:flutter/material.dart';
import '../models/models.dart';
import '../services/client_metrics.dart';
import '../services/connection_service.dart';
import '../state/agentdesk_state.dart';
import 'log_viewer_screen.dart';

class EventScreen extends StatefulWidget {
  final String eventId;
  final AgentDeskState state;
  final ConnectionService connection;
  final ClientMetrics metrics;

  const EventScreen({
    super.key,
    required this.eventId,
    required this.state,
    required this.connection,
    required this.metrics,
  });

  @override
  State<EventScreen> createState() => _EventScreenState();
}

class _EventScreenState extends State<EventScreen> {
  static const _writeInOption = '__agentdesk_write_in__';
  bool _isLoading = false;
  String? _inlineError;
  final TextEditingController _textController = TextEditingController();
  final Set<String> _selectedOptions = <String>{};

  @override
  void dispose() {
    _textController.dispose();
    super.dispose();
  }

  @override
  void initState() {
    super.initState();
    _fetchDetails();
  }

  Future<void> _fetchDetails() async {
    if (widget.connection.status != ConnectionStatus.connected) return;
    try {
      final push = await widget.connection.getEventDetails(widget.eventId);
      widget.state.applyEventDetails(push);
    } catch (_) {
      // Ignore network errors on fetch details, fallback to cached event
    }
  }

  Future<void> _dismiss() async {
    setState(() {
      _isLoading = true;
      _inlineError = null;
    });
    widget.metrics.recordTap();

    try {
      final res = await widget.connection.dismiss(widget.eventId);
      if (!res.ok) {
        setState(() {
          _inlineError = 'Dismiss failed: ${res.error?.wireName ?? "unknown"}';
        });
      } else {
        if (mounted) {
          ScaffoldMessenger.of(
            context,
          ).showSnackBar(const SnackBar(content: Text('Event dismissed')));
          Navigator.of(context).pop();
        }
      }
    } catch (e) {
      setState(() {
        _inlineError = e.toString();
      });
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  Future<void> _respond(Decision decision) async {
    await _respondWithInput(decision);
  }

  Future<void> _respondWithInput(
    Decision decision, {
    List<String>? selectedOptions,
    String? textInput,
  }) async {
    setState(() {
      _isLoading = true;
      _inlineError = null;
    });
    widget.metrics.recordTap();

    try {
      final res = await widget.connection.respondRequest(
        widget.eventId,
        decision,
        selectedOptions: selectedOptions,
        textInput: textInput,
      );
      if (!res.ok) {
        setState(() {
          _inlineError =
              'Action failed: ${res.error?.wireName ?? "unknown error"}';
        });
      } else {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: Text(
                decision == Decision.approve
                    ? 'Response sent'
                    : 'Request denied',
              ),
            ),
          );
          Navigator.of(context).pop();
        }
      }
    } catch (e) {
      setState(() {
        _inlineError = e.toString();
      });
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  Future<void> _submitQuestion(Event event) async {
    final customText = _textController.text;
    final selected = _selectedOptions
        .where((option) => option != _writeInOption)
        .toList(growable: false);

    // A write-in is represented by text_input. The server and core preserve
    // the existing structured protocol and route it as a TextInput response.
    if (_selectedOptions.contains(_writeInOption) &&
        customText.trim().isEmpty) {
      setState(() => _inlineError = 'Enter a custom response.');
      return;
    }

    if (customText.isNotEmpty) {
      await _respondWithInput(
        Decision.approve,
        selectedOptions: selected.isEmpty ? null : selected,
        textInput: customText,
      );
      return;
    }

    await _respondWithInput(
      Decision.approve,
      selectedOptions: selected.isEmpty ? null : selected,
    );
  }

  void _openLogs() {
    widget.metrics.recordTap();
    Navigator.of(context).push(
      MaterialPageRoute(
        builder: (_) => LogViewerScreen(
          eventId: widget.eventId,
          connection: widget.connection,
          metrics: widget.metrics,
        ),
      ),
    );
  }

  Color _categoryColor(Category category) {
    switch (category) {
      case Category.request:
        return Colors.amber.shade800;
      case Category.error:
        return Colors.red.shade700;
      case Category.completed:
        return Colors.green.shade700;
      case Category.working:
        return Colors.blue.shade700;
    }
  }

  String _severityLabel(Severity severity) {
    switch (severity) {
      case Severity.routine:
        return 'Routine (0)';
      case Severity.notable:
        return 'Notable (1)';
      case Severity.important:
        return 'Important (2)';
      case Severity.critical:
        return 'Critical (3)';
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.state,
      builder: (context, _) {
        final event = widget.state.eventFor(widget.eventId);
        final entry = widget.state.entryFor(widget.eventId);

        if (event == null) {
          return Scaffold(
            appBar: AppBar(title: const Text('Event Details')),
            body: const Center(child: Text('Event not found')),
          );
        }

        final color = _categoryColor(event.category);
        final isRequest = event.category == Category.request;
        final resolution = entry?.resolution;
        final isResolved =
            resolution != null && resolution != Resolution.unresolved;

        return Scaffold(
          appBar: AppBar(
            title: Text('${event.category.wireName.toUpperCase()} — Details'),
            actions: [
              IconButton(
                key: const Key('view_logs_appbar_button'),
                icon: const Icon(Icons.article_outlined),
                tooltip: 'View logs',
                onPressed: _openLogs,
              ),
            ],
          ),
          body: ListView(
            padding: const EdgeInsets.all(16),
            children: [
              if (_isLoading) const LinearProgressIndicator(),

              if (_inlineError != null)
                Container(
                  key: const Key('command_error_banner'),
                  margin: const EdgeInsets.only(bottom: 12),
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: Colors.red.shade100,
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: Colors.red.shade700),
                  ),
                  child: Row(
                    children: [
                      Icon(Icons.error_outline, color: Colors.red.shade900),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          _inlineError!,
                          style: TextStyle(
                            color: Colors.red.shade900,
                            fontWeight: FontWeight.bold,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),

              // Summary header card
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Container(
                            width: 12,
                            height: 12,
                            decoration: BoxDecoration(
                              color: color,
                              shape: BoxShape.circle,
                            ),
                          ),
                          const SizedBox(width: 8),
                          Text(
                            '${event.agentName} • ${event.project}',
                            style: TextStyle(
                              color: Colors.grey.shade700,
                              fontWeight: FontWeight.w500,
                            ),
                          ),
                          const Spacer(),
                          Text(
                            _severityLabel(event.severity),
                            style: TextStyle(
                              fontSize: 12,
                              color: color,
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 12),
                      Text(
                        event.summary,
                        style: Theme.of(context).textTheme.titleLarge?.copyWith(
                          fontWeight: FontWeight.bold,
                        ),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        event.message,
                        style: Theme.of(context).textTheme.bodyLarge,
                      ),
                      if (event.taskId != null) ...[
                        const SizedBox(height: 8),
                        Text(
                          'Task: ${event.taskId} (${event.operation.wireName})',
                          style: const TextStyle(
                            fontFamily: 'monospace',
                            fontSize: 12,
                            color: Colors.grey,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ),

              const SizedBox(height: 16),

              // Request prompt and actions (if request)
              if (isRequest)
                Card(
                  key: const Key('request_action_card'),
                  color: Colors.amber.shade50,
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Icon(
                              Icons.help_outline,
                              color: Colors.amber.shade900,
                            ),
                            const SizedBox(width: 8),
                            Text(
                              event.request?.questionType == null
                                  ? 'Approval Request'
                                  : 'Input Required',
                              style: TextStyle(
                                fontWeight: FontWeight.bold,
                                color: Colors.amber.shade900,
                              ),
                            ),
                            const Spacer(),
                            if (entry?.resolution != null)
                              Chip(
                                label: Text(entry!.resolution!.wireName),
                                backgroundColor: Colors.white,
                              ),
                          ],
                        ),
                        if (event.request != null) ...[
                          const SizedBox(height: 8),
                          Text(
                            event.request!.prompt,
                            style: const TextStyle(fontWeight: FontWeight.w600),
                          ),
                        ],
                        const SizedBox(height: 16),
                        if (!isResolved && event.request != null)
                          _buildRequestControls(event)
                        else
                          Text(
                            'Request already resolved: ${resolution!.wireName}',
                            style: const TextStyle(fontStyle: FontStyle.italic),
                          ),
                      ],
                    ),
                  ),
                ),

              const SizedBox(height: 16),

              // Level 2 Details
              Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Details (Level 2)',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const Divider(),
                      if (event.details.isEmpty)
                        const Text(
                          'No extra details provided',
                          style: TextStyle(
                            color: Colors.grey,
                            fontStyle: FontStyle.italic,
                          ),
                        )
                      else
                        ...event.details.entries.map(
                          (e) => Padding(
                            padding: const EdgeInsets.symmetric(vertical: 4.0),
                            child: Row(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Expanded(
                                  flex: 2,
                                  child: Text(
                                    e.key,
                                    style: const TextStyle(
                                      color: Colors.grey,
                                      fontSize: 13,
                                    ),
                                  ),
                                ),
                                Expanded(
                                  flex: 3,
                                  child: Text(
                                    '${e.value}',
                                    style: const TextStyle(
                                      fontWeight: FontWeight.w500,
                                      fontSize: 13,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                    ],
                  ),
                ),
              ),

              const SizedBox(height: 24),

              // Action Buttons
              Row(
                children: [
                  Expanded(
                    child: OutlinedButton.icon(
                      key: const Key('view_logs_button'),
                      onPressed: _openLogs,
                      icon: const Icon(Icons.list_alt),
                      label: const Text('View Logs'),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: OutlinedButton.icon(
                      key: const Key('dismiss_button'),
                      onPressed: _isLoading ? null : _dismiss,
                      icon: const Icon(Icons.archive_outlined),
                      label: const Text('Dismiss'),
                    ),
                  ),
                ],
              ),
            ],
          ),
        );
      },
    );
  }

  Widget _buildRequestControls(Event event) {
    final request = event.request!;
    final isApprovalOptions =
        request.options.length == 2 &&
        request.options
            .map((option) => option.toLowerCase())
            .toSet()
            .containsAll({'approve', 'deny'});
    final questionType =
        request.questionType ??
        (request.options.isNotEmpty && !isApprovalOptions
            ? QuestionType.singleChoice
            : null);
    final allowsWriteIn = event.details['allows_write_in'] == true;

    if (questionType == null) {
      return Row(
        children: [
          Expanded(
            child: FilledButton.icon(
              key: const Key('approve_button'),
              onPressed: _isLoading ? null : () => _respond(Decision.approve),
              icon: const Icon(Icons.check),
              label: const Text('Approve'),
              style: FilledButton.styleFrom(
                backgroundColor: Colors.green.shade700,
              ),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: FilledButton.icon(
              key: const Key('deny_button'),
              onPressed: _isLoading ? null : () => _respond(Decision.deny),
              icon: const Icon(Icons.close),
              label: const Text('Deny'),
              style: FilledButton.styleFrom(
                backgroundColor: Colors.red.shade700,
              ),
            ),
          ),
        ],
      );
    }

    final controls = <Widget>[];
    if (questionType == QuestionType.singleChoice) {
      final options = [...request.options, if (allowsWriteIn) _writeInOption];
      controls.add(
        RadioGroup<String>(
          groupValue: _selectedOptions.isEmpty ? null : _selectedOptions.first,
          onChanged: _isLoading
              ? (_) {}
              : (value) {
                  if (value == null) return;
                  setState(() {
                    _selectedOptions
                      ..clear()
                      ..add(value);
                  });
                },
          child: Column(
            children: options
                .map(
                  (option) => RadioListTile<String>(
                    key: Key('question_option_$option'),
                    title: Text(
                      option == _writeInOption
                          ? 'Other / Write your own'
                          : option,
                    ),
                    value: option,
                  ),
                )
                .toList(),
          ),
        ),
      );
    } else if (questionType == QuestionType.multipleChoice) {
      final options = [...request.options, if (allowsWriteIn) _writeInOption];
      controls.addAll(
        options.map((option) {
          return CheckboxListTile(
            key: Key('question_option_$option'),
            title: Text(
              option == _writeInOption ? 'Other / Write your own' : option,
            ),
            value: _selectedOptions.contains(option),
            onChanged: _isLoading
                ? null
                : (value) {
                    setState(() {
                      if (value == true) {
                        _selectedOptions.add(option);
                      } else {
                        _selectedOptions.remove(option);
                      }
                    });
                  },
          );
        }),
      );
    }

    if (questionType == QuestionType.freeText ||
        _selectedOptions.contains(_writeInOption)) {
      controls.add(
        TextField(
          key: const Key('question_text_input'),
          controller: _textController,
          enabled: !_isLoading,
          maxLines: 4,
          decoration: InputDecoration(
            labelText: 'Your response',
            hintText: allowsWriteIn ? 'Or enter a custom response' : null,
            border: const OutlineInputBorder(),
          ),
        ),
      );
    }

    controls.add(const SizedBox(height: 12));
    controls.add(
      Row(
        children: [
          Expanded(
            child: FilledButton.icon(
              key: const Key('send_question_button'),
              onPressed: _isLoading ? null : () => _submitQuestion(event),
              icon: const Icon(Icons.send),
              label: const Text('Send response'),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: OutlinedButton.icon(
              key: const Key('deny_question_button'),
              onPressed: _isLoading ? null : () => _respond(Decision.deny),
              icon: const Icon(Icons.close),
              label: const Text('Cancel'),
            ),
          ),
        ],
      ),
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: controls,
    );
  }
}
