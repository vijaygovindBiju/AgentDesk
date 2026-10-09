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
    final request = event.request!;
    final writeInOption = request.options.firstWhere(
      (option) => option.trim().toLowerCase() == 'other',
      orElse: () => _writeInOption,
    );
    final selected = _selectedOptions
        .where((option) => option != _writeInOption)
        .toList(growable: false);

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

    // A write-in is represented by text_input. The server and core preserve
    // the existing structured protocol and route it as a TextInput response.
    if (_selectedOptions.contains(writeInOption) && customText.trim().isEmpty) {
      setState(() => _inlineError = 'Enter a custom response.');
      return;
    }

    if (questionType == QuestionType.freeText && customText.trim().isEmpty) {
      setState(() => _inlineError = 'Please enter a response.');
      return;
    }

    if ((questionType == QuestionType.singleChoice ||
            questionType == QuestionType.multipleChoice ||
            questionType == QuestionType.yesNo) &&
        _selectedOptions.isEmpty &&
        customText.trim().isEmpty) {
      setState(() => _inlineError = 'Please select an option.');
      return;
    }

    if (customText.trim().isNotEmpty) {
      await _respondWithInput(
        Decision.approve,
        selectedOptions: selected.isEmpty ? null : selected,
        textInput: customText.trim(),
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
        final isRequest = event.category == Category.request || event.request != null;
        final resolution = entry?.resolution;
        final isPendingDelivery = resolution == Resolution.pendingDelivery;
        final isResolved = resolution != null &&
            resolution != Resolution.unresolved &&
            !isPendingDelivery;

        final isDark = Theme.of(context).brightness == Brightness.dark;
        final requestCardColor = isDark ? const Color(0xFF2A2113) : Colors.amber.shade50;
        final requestBorderColor = isDark ? Colors.amber.shade700.withValues(alpha: 0.4) : Colors.amber.shade200;
        final requestHeaderColor = isDark ? Colors.amber.shade300 : Colors.amber.shade900;
        final requestPromptColor = isDark ? Colors.amber.shade50 : Colors.brown.shade900;

        final displayCategory = isRequest ? 'REQUEST' : event.category.wireName.toUpperCase();

        return Scaffold(
          appBar: AppBar(
            title: Text('$displayCategory — Details'),
            actions: [
              IconButton(
                key: const Key('view_logs_appbar_button'),
                icon: const Icon(Icons.article_outlined),
                tooltip: 'View logs',
                onPressed: _openLogs,
              ),
            ],
          ),
          body: GestureDetector(
            onTap: () => FocusScope.of(context).unfocus(),
            behavior: HitTestBehavior.opaque,
            child: ListView(
              keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
              padding: EdgeInsets.fromLTRB(
                16,
                16,
                16,
                MediaQuery.of(context).viewInsets.bottom > 0 ? 120 : 32,
              ),
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
                        event.category == Category.completed
                            ? _extractHeadline(event)
                            : event.message,
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
                  color: requestCardColor,
                  shape: RoundedRectangleBorder(
                    borderRadius: BorderRadius.circular(12),
                    side: BorderSide(color: requestBorderColor),
                  ),
                  child: Padding(
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Icon(
                              Icons.help_outline,
                              color: requestHeaderColor,
                            ),
                            const SizedBox(width: 8),
                            Text(
                              event.request?.questionType == null
                                  ? 'Approval Request'
                                  : 'Input Required',
                              style: TextStyle(
                                fontWeight: FontWeight.bold,
                                color: requestHeaderColor,
                              ),
                            ),
                            const Spacer(),
                            if (entry?.resolution != null)
                              Chip(
                                label: Text(
                                  entry!.resolution!.wireName,
                                  style: TextStyle(
                                    color: isDark ? Colors.amber.shade100 : Colors.brown.shade900,
                                    fontWeight: FontWeight.w600,
                                    fontSize: 12,
                                  ),
                                ),
                                backgroundColor: isDark ? const Color(0xFF423420) : Colors.white,
                              ),
                          ],
                        ),
                        if (event.request != null) ...[
                          const SizedBox(height: 8),
                          Text(
                            event.request!.prompt,
                            style: TextStyle(
                              fontWeight: FontWeight.w600,
                              fontSize: 15,
                              color: requestPromptColor,
                            ),
                          ),
                        ],
                        const SizedBox(height: 16),
                        if (isPendingDelivery)
                          Padding(
                            padding: const EdgeInsets.symmetric(vertical: 8.0),
                            child: Row(
                              children: [
                                const SizedBox(
                                  width: 16,
                                  height: 16,
                                  child: CircularProgressIndicator(strokeWidth: 2),
                                ),
                                const SizedBox(width: 10),
                                Text(
                                  'Delivering response to agent...',
                                  style: TextStyle(
                                    fontStyle: FontStyle.italic,
                                    color: isDark ? Colors.grey.shade400 : Colors.grey.shade700,
                                  ),
                                ),
                              ],
                            ),
                          )
                        else if (!isResolved && event.request != null)
                          _buildRequestControls(event, isDark: isDark)
                        else
                          Text(
                            'Request already resolved: ${resolution?.wireName ?? "resolved"}',
                            style: TextStyle(
                              fontStyle: FontStyle.italic,
                              color: isDark ? Colors.grey.shade400 : Colors.grey.shade700,
                            ),
                          ),
                      ],
                    ),
                  ),
                ),

              const SizedBox(height: 16),

              // If Completed event: Show the Full Detailed Execution View!
              if (event.category == Category.completed) ...[
                _buildCompletedExecutionView(event, context),
                const SizedBox(height: 16),
              ],

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
                      if (event.details.isEmpty ||
                          event.details.entries.where((e) =>
                              e.key != 'execution_steps' &&
                              (event.category != Category.completed ||
                                  (e.key != 'description' &&
                                      e.key != 'summary'))).isEmpty)
                        const Text(
                          'No extra details provided',
                          style: TextStyle(
                            color: Colors.grey,
                            fontStyle: FontStyle.italic,
                          ),
                        )
                      else
                        ...event.details.entries
                            .where((e) =>
                                e.key != 'execution_steps' &&
                                (event.category != Category.completed ||
                                    (e.key != 'description' &&
                                        e.key != 'summary')))
                            .map(
                              (e) => Padding(
                                padding:
                                    const EdgeInsets.symmetric(vertical: 4.0),
                                child: Row(
                                  crossAxisAlignment:
                                      CrossAxisAlignment.start,
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
        ),
      );
    },
    );
  }

  Widget _buildRequestControls(Event event, {required bool isDark}) {
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
    final allowsWriteIn =
        questionType != null && event.details['allows_write_in'] != false;
    final writeInOption = request.options.firstWhere(
      (option) => option.trim().toLowerCase() == 'other',
      orElse: () => _writeInOption,
    );

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
    if (questionType == QuestionType.singleChoice ||
        questionType == QuestionType.yesNo) {
      final options = [
        ...request.options,
        if (allowsWriteIn && writeInOption == _writeInOption) writeInOption,
      ];
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
                      style: TextStyle(
                        color: isDark ? Colors.grey.shade100 : Colors.black87,
                      ),
                    ),
                    value: option,
                  ),
                )
                .toList(),
          ),
        ),
      );
    } else if (questionType == QuestionType.multipleChoice) {
      final options = [
        ...request.options,
        if (allowsWriteIn && writeInOption == _writeInOption) writeInOption,
      ];
      controls.addAll(
        options.map((option) {
          return CheckboxListTile(
            key: Key('question_option_$option'),
            title: Text(
              option == _writeInOption ? 'Other / Write your own' : option,
              style: TextStyle(
                color: isDark ? Colors.grey.shade100 : Colors.black87,
              ),
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
        (allowsWriteIn && _selectedOptions.contains(writeInOption))) {
      controls.add(
        TextField(
          key: const Key('question_text_input'),
          controller: _textController,
          enabled: !_isLoading,
          maxLines: 4,
          style: TextStyle(
            color: isDark ? Colors.white : Colors.black87,
          ),
          decoration: InputDecoration(
            labelText: 'Your response',
            hintText: allowsWriteIn ? 'Or enter a custom response' : null,
            filled: true,
            fillColor: isDark ? const Color(0xFF1E1E1E) : Colors.white,
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
              onPressed: _isLoading
                  ? null
                  : (questionType == QuestionType.freeText
                      ? () => Navigator.of(context).pop()
                      : () => _respond(Decision.deny)),
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

  String _extractHeadline(Event event) {
    final finalRes = event.details['final_result']?.toString() ??
        _extractFinalResultFallback(event.message);
    for (final line in finalRes.split('\n')) {
      final t = line.trim();
      if (t.isNotEmpty &&
          !t.startsWith('●') &&
          !t.startsWith('▸') &&
          !t.startsWith('Thought for')) {
        return t;
      }
    }
    return event.summary;
  }

  String _extractFinalResultFallback(String message) {
    final lines = message.split('\n');
    final resultLines = <String>[];
    for (int i = lines.length - 1; i >= 0; i--) {
      final t = lines[i].trim();
      if (t.isEmpty) {
        if (resultLines.isNotEmpty && resultLines.last.isNotEmpty) {
          resultLines.add('');
        }
        continue;
      }
      if (t.startsWith('●') ||
          t.startsWith('▸') ||
          t.startsWith('Thought for') ||
          t.startsWith('>') ||
          t.startsWith('───') ||
          t.contains('(ctrl+o to expand)')) {
        if (resultLines.isNotEmpty) {
          break;
        }
        continue;
      }
      resultLines.add(t);
    }
    if (resultLines.isNotEmpty) {
      final reversed = resultLines.reversed.toList();
      while (reversed.isNotEmpty && reversed.first.isEmpty) {
        reversed.removeAt(0);
      }
      while (reversed.isNotEmpty && reversed.last.isEmpty) {
        reversed.removeLast();
      }
      final joined = reversed.join('\n').trim();
      if (joined.toLowerCase().startsWith('final result:')) {
        final after = joined.substring('final result:'.length).trim();
        if (after.isNotEmpty) return after;
      }
      if (joined.isNotEmpty) {
        return joined;
      }
    }
    return message;
  }

  Widget _buildCompletedExecutionView(Event event, BuildContext context) {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final borderColor = isDark ? Colors.grey.shade800 : Colors.grey.shade300;

    final stepsData = event.details['execution_steps'];
    final List<Widget> stepWidgets = [];

    if (stepsData is List && stepsData.isNotEmpty) {
      for (final step in stepsData) {
        if (step is! Map) continue;
        final kind = step['kind']?.toString() ?? 'message';
        final tool = step['tool']?.toString();
        final command = step['command']?.toString();
        final duration = step['duration']?.toString();
        final tokens = step['tokens']?.toString();
        final text = step['text']?.toString();

        if (kind == 'thought') {
          stepWidgets.add(
            Container(
              margin: const EdgeInsets.symmetric(vertical: 6),
              padding: const EdgeInsets.all(12),
              decoration: BoxDecoration(
                color: isDark ? const Color(0xFF232833) : Colors.grey.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                    color: isDark ? Colors.grey.shade800 : Colors.grey.shade200),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      const Icon(Icons.psychology_outlined,
                          size: 16, color: Colors.blueAccent),
                      const SizedBox(width: 6),
                      Text(
                        'Thought${duration != null ? " for $duration" : ""}',
                        style: const TextStyle(
                            fontWeight: FontWeight.bold,
                            fontSize: 13,
                            color: Colors.blueAccent),
                      ),
                      if (tokens != null) ...[
                        const SizedBox(width: 8),
                        Container(
                          padding: const EdgeInsets.symmetric(
                              horizontal: 6, vertical: 2),
                          decoration: BoxDecoration(
                            color: isDark
                                ? Colors.blue.shade900.withValues(alpha: 0.3)
                                : Colors.blue.shade50,
                            borderRadius: BorderRadius.circular(4),
                          ),
                          child: Text(
                            tokens,
                            style: TextStyle(
                                fontSize: 11,
                                color: isDark
                                    ? Colors.blue.shade200
                                    : Colors.blue.shade800),
                          ),
                        ),
                      ],
                    ],
                  ),
                  if (text != null && text.isNotEmpty) ...[
                    const SizedBox(height: 6),
                    Text(
                      text,
                      style: TextStyle(
                          fontSize: 13,
                          color: isDark
                              ? Colors.grey.shade300
                              : Colors.grey.shade800),
                    ),
                  ],
                ],
              ),
            ),
          );
        } else if (kind == 'tool_call') {
          final isBash = tool?.toLowerCase() == 'bash';
          final isEdit =
              tool?.toLowerCase() == 'edit' || tool?.toLowerCase() == 'write';
          final icon = isBash
              ? Icons.terminal
              : (isEdit ? Icons.edit_note : Icons.build_outlined);
          final iconColor = isBash
              ? Colors.amber.shade700
              : (isEdit ? Colors.teal : Colors.orange);

          stepWidgets.add(
            Container(
              margin: const EdgeInsets.symmetric(vertical: 4),
              padding: const EdgeInsets.all(10),
              decoration: BoxDecoration(
                color: isDark ? const Color(0xFF1B2028) : Colors.grey.shade50,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(
                    color: isDark ? Colors.grey.shade800 : Colors.grey.shade200),
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Icon(icon, size: 16, color: iconColor),
                      const SizedBox(width: 6),
                      Text(
                        '● ${tool ?? "Tool"}',
                        style: TextStyle(
                            fontWeight: FontWeight.bold,
                            fontSize: 13,
                            color: iconColor),
                      ),
                    ],
                  ),
                  if (command != null && command.isNotEmpty) ...[
                    const SizedBox(height: 4),
                    Container(
                      width: double.infinity,
                      padding:
                          const EdgeInsets.symmetric(horizontal: 8, vertical: 6),
                      decoration: BoxDecoration(
                        color: isDark
                            ? const Color(0xFF14171D)
                            : Colors.grey.shade100,
                        borderRadius: BorderRadius.circular(4),
                      ),
                      child: Text(
                        command,
                        style: const TextStyle(
                            fontFamily: 'monospace', fontSize: 12),
                      ),
                    ),
                  ],
                  if (text != null && text.isNotEmpty) ...[
                    const SizedBox(height: 4),
                    Text(
                      text,
                      style: TextStyle(
                          fontSize: 12,
                          color: isDark
                              ? Colors.grey.shade400
                              : Colors.grey.shade700),
                    ),
                  ],
                ],
              ),
            ),
          );
        } else if (kind == 'result') {
          // Rendered separately in prominent result banner
        } else {
          if (text != null && text.isNotEmpty) {
            stepWidgets.add(
              Padding(
                padding:
                    const EdgeInsets.symmetric(vertical: 4, horizontal: 4),
                child: Text(
                  text,
                  style: TextStyle(
                      fontSize: 13,
                      color: isDark
                          ? Colors.grey.shade300
                          : Colors.grey.shade800),
                ),
              ),
            );
          }
        }
      }
    } else {
      // Fallback: parse lines from event.message
      final lines = event.message.split('\n');
      for (final line in lines) {
        final t = line.trim();
        if (t.isEmpty) continue;
        if (t.startsWith('▸ Thought') || t.startsWith('Thought for')) {
          stepWidgets.add(
            Padding(
              padding: const EdgeInsets.only(top: 8, bottom: 2),
              child: Row(
                children: [
                  const Icon(Icons.psychology_outlined,
                      size: 16, color: Colors.blueAccent),
                  const SizedBox(width: 6),
                  Expanded(
                    child: Text(
                      t,
                      style: const TextStyle(
                          fontWeight: FontWeight.bold,
                          fontSize: 13,
                          color: Colors.blueAccent),
                    ),
                  ),
                ],
              ),
            ),
          );
        } else if (t.startsWith('● ')) {
          stepWidgets.add(
            Padding(
              padding: const EdgeInsets.only(top: 6, bottom: 2),
              child: Text(
                t,
                style: const TextStyle(
                    fontWeight: FontWeight.bold,
                    fontSize: 13,
                    color: Colors.orange),
              ),
            ),
          );
        } else {
          stepWidgets.add(
            Padding(
              padding:
                  const EdgeInsets.symmetric(vertical: 2, horizontal: 4),
              child: Text(
                t,
                style: TextStyle(
                  fontSize: 12,
                  fontFamily: t.startsWith('/') ||
                          t.startsWith('git') ||
                          t.startsWith('rm')
                      ? 'monospace'
                      : null,
                  color: isDark ? Colors.grey.shade300 : Colors.grey.shade800,
                ),
              ),
            ),
          );
        }
      }
    }

    final finalResult = event.details['final_result']?.toString() ??
        _extractFinalResultFallback(event.message);

    return Card(
      key: const Key('completed_execution_view'),
      shape: RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(12),
        side: BorderSide(color: borderColor),
      ),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (finalResult.trim().isNotEmpty) ...[
              Row(
                children: [
                  const Icon(Icons.check_circle_outline, color: Colors.green),
                  const SizedBox(width: 8),
                  Text(
                    'Final Result',
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
                          fontWeight: FontWeight.bold,
                          color: Colors.green,
                        ),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              Container(
                width: double.infinity,
                padding: const EdgeInsets.all(14),
                decoration: BoxDecoration(
                  color: isDark
                      ? Colors.green.shade900.withValues(alpha: 0.2)
                      : Colors.green.shade50,
                  borderRadius: BorderRadius.circular(8),
                  border: Border.all(
                    color: isDark
                        ? Colors.green.shade800
                        : Colors.green.shade200,
                  ),
                ),
                child: SelectableText(
                  finalResult.trim(),
                  style: TextStyle(
                    fontSize: 14,
                    height: 1.45,
                    fontWeight: FontWeight.w500,
                    color: isDark
                        ? Colors.green.shade100
                        : Colors.green.shade900,
                  ),
                ),
              ),
              const SizedBox(height: 16),
            ],
            Row(
              children: [
                const Icon(Icons.playlist_add_check, color: Colors.blueGrey),
                const SizedBox(width: 8),
                Text(
                  'Execution Transcript',
                  style: Theme.of(context)
                      .textTheme
                      .titleSmall
                      ?.copyWith(fontWeight: FontWeight.bold),
                ),
                const Spacer(),
                if (event.details['tool_count'] != null)
                  Chip(
                    label: Text(
                      '${event.details["tool_count"]} tool calls',
                      style: const TextStyle(
                          fontSize: 11, fontWeight: FontWeight.bold),
                    ),
                    padding: EdgeInsets.zero,
                    visualDensity: VisualDensity.compact,
                  ),
              ],
            ),
            const Divider(),
            if (stepWidgets.isNotEmpty) ...stepWidgets,
          ],
        ),
      ),
    );
  }
}
