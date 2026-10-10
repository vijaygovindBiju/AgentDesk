import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../services/connection_service.dart';
import 'agent_control_overlay.dart';
import 'app_theme.dart';

class NewWorkScreen extends StatefulWidget {
  final ConnectionService connection;
  final String? initialPrompt;
  final VoidCallback? onPromptConsumed;
  final VoidCallback? onSubmitted;

  const NewWorkScreen({
    super.key,
    required this.connection,
    this.initialPrompt,
    this.onPromptConsumed,
    this.onSubmitted,
  });

  @override
  State<NewWorkScreen> createState() => _NewWorkScreenState();
}

class _NewWorkScreenState extends State<NewWorkScreen> {
  final _promptController = TextEditingController();
  final _workspaceController = TextEditingController();
  bool _isSubmitting = false;
  String _activeModel = 'gemini-3.8-flash';
  String _activeEffort = 'high';
  String _selectedMode = 'default';
  List<String> _recentWorkspaces = [
    '/media/pirate/Shared/currently working/AgentDesk',
    '/home/pirate',
  ];

  final List<String> _slashCommands = [
    '/model',
    '/effort',
    '/usage',
    '/plan',
    '/goal',
    '/boost',
    '/learn',
  ];

  final List<Map<String, String>> _presets = [
    {
      'label': 'Fix Failing Tests',
      'prompt': 'Run the test suite, find failing tests, and fix them with minimal changes.',
    },
    {
      'label': 'Refactor Code',
      'prompt': 'Refactor the recent changes for clarity, performance, and idiomatic style.',
    },
    {
      'label': 'Run Unit Tests',
      'prompt': 'Run the complete test suite and verify all unit tests pass.',
    },
    {
      'label': 'Architecture Review',
      'prompt': 'Inspect the project architecture and summarize potential edge cases or vulnerabilities.',
    },
  ];

  @override
  void initState() {
    super.initState();
    _promptController.addListener(_onPromptChanged);
    _loadRecentWorkspaces();
    if (widget.initialPrompt != null && widget.initialPrompt!.isNotEmpty) {
      _promptController.text = widget.initialPrompt!;
      _promptController.selection = TextSelection.fromPosition(
        TextPosition(offset: _promptController.text.length),
      );
      widget.onPromptConsumed?.call();
    }
  }

  Future<void> _loadRecentWorkspaces() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      final list = prefs.getStringList('agentdesk_recent_workspaces');
      if (list != null && list.isNotEmpty) {
        if (!mounted) return;
        setState(() {
          _recentWorkspaces = list;
          if (_workspaceController.text.isEmpty) {
            _workspaceController.text = list.first;
          }
        });
      }
    } catch (_) {}
  }

  Future<void> _saveRecentWorkspace(String path) async {
    final clean = path.trim();
    if (clean.isEmpty) return;
    try {
      final prefs = await SharedPreferences.getInstance();
      final updated = [clean, ..._recentWorkspaces.where((p) => p != clean)].take(5).toList();
      await prefs.setStringList('agentdesk_recent_workspaces', updated);
      if (!mounted) return;
      setState(() {
        _recentWorkspaces = updated;
      });
    } catch (_) {}
  }

  void _showDirectoryPicker() {
    final isDark = Theme.of(context).brightness == Brightness.dark;
    final customDirController = TextEditingController(text: _workspaceController.text);

    showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: isDark ? AppTheme.surfaceCardElevated : Colors.white,
      shape: const RoundedRectangleBorder(
        borderRadius: BorderRadius.vertical(top: Radius.circular(20)),
      ),
      builder: (sheetContext) {
        return Padding(
          padding: EdgeInsets.fromLTRB(
            20,
            20,
            20,
            MediaQuery.of(sheetContext).viewInsets.bottom + 24,
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Row(
                    children: [
                      const Icon(Icons.folder_special, color: AppTheme.primaryCyan, size: 20),
                      const SizedBox(width: 8),
                      Text(
                        'Select Workspace Directory',
                        style: Theme.of(sheetContext).textTheme.titleMedium?.copyWith(
                              fontWeight: FontWeight.bold,
                              color: isDark ? AppTheme.textPrimary : null,
                            ),
                      ),
                    ],
                  ),
                  IconButton(
                    icon: const Icon(Icons.close, size: 18),
                    onPressed: () => Navigator.pop(sheetContext),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Text(
                'Choose a target directory on your laptop for the agent session:',
                style: TextStyle(
                  fontSize: 12,
                  color: isDark ? AppTheme.textSecondary : Colors.grey.shade600,
                ),
              ),
              const SizedBox(height: 16),
              TextField(
                controller: customDirController,
                autofocus: false,
                decoration: InputDecoration(
                  labelText: 'Custom Directory Path',
                  hintText: '/media/pirate/Shared/...',
                  prefixIcon: const Icon(Icons.edit_road, size: 18),
                  suffixIcon: IconButton(
                    icon: const Icon(Icons.check, color: AppTheme.primaryEmerald),
                    tooltip: 'Apply path',
                    onPressed: () {
                      final path = customDirController.text.trim();
                      if (path.isNotEmpty) {
                        setState(() => _workspaceController.text = path);
                        _saveRecentWorkspace(path);
                      }
                      Navigator.pop(sheetContext);
                    },
                  ),
                ),
              ),
              const SizedBox(height: 16),
              Text(
                'PRESETS & RECENT',
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.bold,
                  color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                  letterSpacing: 0.5,
                ),
              ),
              const SizedBox(height: 8),
              Flexible(
                child: ListView.separated(
                  shrinkWrap: true,
                  itemCount: _recentWorkspaces.length,
                  separatorBuilder: (_, _) => const Divider(height: 1),
                  itemBuilder: (context, idx) {
                    final p = _recentWorkspaces[idx];
                    final isCurrent = _workspaceController.text.trim() == p.trim();
                    return ListTile(
                      dense: true,
                      contentPadding: EdgeInsets.zero,
                      leading: Icon(
                        isCurrent ? Icons.check_circle : Icons.folder_outlined,
                        color: isCurrent ? AppTheme.primaryCyan : AppTheme.textSecondary,
                        size: 20,
                      ),
                      title: Text(
                        p,
                        style: TextStyle(
                          fontSize: 13,
                          fontFamily: 'monospace',
                          fontWeight: isCurrent ? FontWeight.bold : FontWeight.normal,
                          color: isCurrent
                              ? AppTheme.primaryCyan
                              : (isDark ? AppTheme.textPrimary : Colors.black87),
                        ),
                      ),
                      onTap: () {
                        setState(() => _workspaceController.text = p);
                        Navigator.pop(sheetContext);
                      },
                    );
                  },
                ),
              ),
              const SizedBox(height: 12),
              SizedBox(
                width: double.infinity,
                child: FilledButton(
                  style: FilledButton.styleFrom(
                    backgroundColor: AppTheme.primaryCyan,
                    foregroundColor: Colors.black,
                  ),
                  onPressed: () {
                    final path = customDirController.text.trim();
                    if (path.isNotEmpty) {
                      setState(() => _workspaceController.text = path);
                      _saveRecentWorkspace(path);
                    }
                    Navigator.pop(sheetContext);
                  },
                  child: const Text('Confirm Selection', style: TextStyle(fontWeight: FontWeight.bold)),
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  Widget _buildModeCard({
    required String mode,
    required String title,
    required String subtitle,
    required IconData icon,
    required Color color,
    required bool isSelected,
    required bool isDark,
  }) {
    return InkWell(
      onTap: () => setState(() => _selectedMode = mode),
      borderRadius: BorderRadius.circular(12),
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 200),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
        decoration: BoxDecoration(
          color: isSelected
              ? color.withValues(alpha: isDark ? 0.20 : 0.12)
              : (isDark ? AppTheme.surfaceCard : Colors.grey.shade50),
          borderRadius: BorderRadius.circular(12),
          border: Border.all(
            color: isSelected ? color : (isDark ? AppTheme.cardBorder : Colors.grey.shade300),
            width: isSelected ? 2.0 : 1.0,
          ),
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Icon(icon, size: 16, color: isSelected ? color : AppTheme.textSecondary),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    title,
                    style: TextStyle(
                      fontSize: 11,
                      fontFamily: title.startsWith('--') ? 'monospace' : null,
                      fontWeight: FontWeight.bold,
                      color: isSelected
                          ? (isDark ? Colors.white : Colors.black87)
                          : (isDark ? AppTheme.textSecondary : Colors.grey.shade700),
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                  ),
                ),
                if (isSelected)
                  Icon(Icons.check_circle, size: 14, color: color)
                else
                  Icon(Icons.radio_button_unchecked, size: 14, color: isDark ? AppTheme.textSecondary : Colors.grey.shade400),
              ],
            ),
            const SizedBox(height: 4),
            Text(
              subtitle,
              style: TextStyle(
                fontSize: 10,
                color: isDark ? AppTheme.textSecondary : Colors.grey.shade600,
              ),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ],
        ),
      ),
    );
  }

  void _onPromptChanged() {
    setState(() {});
  }

  @override
  void didUpdateWidget(NewWorkScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.initialPrompt != null &&
        widget.initialPrompt!.isNotEmpty &&
        widget.initialPrompt != oldWidget.initialPrompt) {
      _promptController.text = widget.initialPrompt!;
      _promptController.selection = TextSelection.fromPosition(
        TextPosition(offset: _promptController.text.length),
      );
      widget.onPromptConsumed?.call();
    }
  }

  @override
  void dispose() {
    _promptController.removeListener(_onPromptChanged);
    _promptController.dispose();
    _workspaceController.dispose();
    super.dispose();
  }

  void _onSlashCommandTap(String cmd) {
    if (cmd == '/model' || cmd == '/effort' || cmd == '/usage') {
      AgentControlOverlay.show(
        context,
        connection: widget.connection,
        currentModel: _activeModel,
        currentEffort: _activeEffort,
        onModelChanged: (m) => setState(() => _activeModel = m),
        onEffortChanged: (e) => setState(() => _activeEffort = e),
      );
    } else {
      final text = _promptController.text;
      final selection = _promptController.selection;
      final toInsert = '$cmd ';
      
      if (selection.start >= 0 && selection.end >= 0) {
        final newText = text.replaceRange(selection.start, selection.end, toInsert);
        final newOffset = selection.start + toInsert.length;
        _promptController.value = TextEditingValue(
          text: newText,
          selection: TextSelection.collapsed(offset: newOffset),
        );
      } else {
        final newText = text.isEmpty ? toInsert : '$text\n$toInsert';
        _promptController.value = TextEditingValue(
          text: newText,
          selection: TextSelection.collapsed(offset: newText.length),
        );
      }
    }
  }

  void _applyPreset(String prompt) {
    _promptController.text = prompt;
    _promptController.selection = TextSelection.fromPosition(
      TextPosition(offset: prompt.length),
    );
  }

  Future<void> _submitPrompt() async {
    final prompt = _promptController.text.trim();
    if (prompt.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Please enter a prompt')),
      );
      return;
    }

    setState(() {
      _isSubmitting = true;
    });

    try {
      final workspace = _workspaceController.text.trim().isNotEmpty
          ? _workspaceController.text.trim()
          : null;
      if (workspace != null) {
        _saveRecentWorkspace(workspace);
      }
      final result = await widget.connection.startTask(
        prompt: prompt,
        workspace: workspace,
        mode: _selectedMode,
      );

      if (!mounted) return;

      if (result.ok) {
        _promptController.clear();
        widget.onSubmitted?.call();
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Prompt sent to AgentDesk terminal! 🚀'),
            backgroundColor: AppTheme.primaryEmerald,
          ),
        );
      } else {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text('Failed to submit prompt: ${result.error?.wireName ?? "unknown error"}'),
            backgroundColor: AppTheme.errorRed,
          ),
        );
      }
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('Error: $e'),
          backgroundColor: AppTheme.errorRed,
        ),
      );
    } finally {
      if (mounted) {
        setState(() {
          _isSubmitting = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.connection,
      builder: (context, _) {
        final isConnected = widget.connection.status == ConnectionStatus.connected;
        final isDark = Theme.of(context).brightness == Brightness.dark;

        return Scaffold(
          appBar: AppBar(
            title: const Text('New Work'),
            actions: [
              IconButton(
                icon: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
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
                      Icon(Icons.tune, size: 14, color: AppTheme.primaryEmerald),
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
                tooltip: 'Agent Controls & Quotas',
                onPressed: () => AgentControlOverlay.show(
                  context,
                  connection: widget.connection,
                  currentModel: _activeModel,
                  currentEffort: _activeEffort,
                  onModelChanged: (m) => setState(() => _activeModel = m),
                  onEffortChanged: (e) => setState(() => _activeEffort = e),
                ),
              ),
              const SizedBox(width: 8),
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
                // Header Card
                Text(
                  'Send prompt to coding agent',
                  style: Theme.of(context).textTheme.headlineSmall?.copyWith(
                        fontWeight: FontWeight.bold,
                        color: isDark ? AppTheme.textPrimary : null,
                      ),
                ),
                const SizedBox(height: 6),
                Text(
                  'Send instructions directly to the active coding agent on your laptop.',
                  style: TextStyle(
                    fontSize: 13,
                    color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                  ),
                ),
                const SizedBox(height: 20),

                // Workspace Directory Header
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Row(
                      children: [
                        const Icon(Icons.folder_open, size: 14, color: AppTheme.primaryCyan),
                        const SizedBox(width: 6),
                        Text(
                          'WORKSPACE DIRECTORY',
                          style: TextStyle(
                            fontSize: 11,
                            fontWeight: FontWeight.bold,
                            color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                            letterSpacing: 0.5,
                          ),
                        ),
                      ],
                    ),
                    TextButton.icon(
                      onPressed: _showDirectoryPicker,
                      style: TextButton.styleFrom(
                        padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                        minimumSize: Size.zero,
                        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                      ),
                      icon: const Icon(Icons.list_alt, size: 14, color: AppTheme.primaryCyan),
                      label: const Text(
                        'Select Dir',
                        style: TextStyle(fontSize: 12, color: AppTheme.primaryCyan, fontWeight: FontWeight.bold),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),

                // Quick Directory Preset Chips
                if (_recentWorkspaces.isNotEmpty) ...[
                  SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      children: _recentWorkspaces.map((wsPath) {
                        final isSelected = _workspaceController.text.trim() == wsPath.trim();
                        final parts = wsPath.split('/').where((s) => s.isNotEmpty).toList();
                        final shortName = parts.isNotEmpty ? parts.last : wsPath;
                        return Padding(
                          padding: const EdgeInsets.only(right: 8),
                          child: ActionChip(
                            avatar: Icon(
                              isSelected ? Icons.check_circle : Icons.folder_outlined,
                              size: 14,
                              color: isSelected ? Colors.black : AppTheme.primaryCyan,
                            ),
                            label: Text(
                              shortName,
                              style: TextStyle(
                                fontSize: 12,
                                fontWeight: isSelected ? FontWeight.bold : FontWeight.normal,
                                color: isSelected ? Colors.black : (isDark ? AppTheme.textPrimary : Colors.black87),
                              ),
                            ),
                            tooltip: wsPath,
                            backgroundColor: isSelected
                                ? AppTheme.primaryCyan
                                : (isDark ? AppTheme.surfaceCardElevated : Colors.grey.shade100),
                            side: BorderSide(
                              color: isSelected ? AppTheme.primaryCyan : (isDark ? AppTheme.cardBorder : Colors.grey.shade300),
                            ),
                            onPressed: () {
                              setState(() {
                                _workspaceController.text = wsPath;
                              });
                            },
                          ),
                        );
                      }).toList(),
                    ),
                  ),
                  const SizedBox(height: 8),
                ],

                // Workspace Input
                TextField(
                  controller: _workspaceController,
                  decoration: InputDecoration(
                    labelText: 'Workspace Path (Optional)',
                    hintText: '/media/pirate/Shared/...',
                    prefixIcon: const Icon(Icons.folder_outlined, size: 18),
                    suffixIcon: _workspaceController.text.isNotEmpty
                        ? IconButton(
                            icon: const Icon(Icons.clear, size: 16),
                            onPressed: () => setState(() => _workspaceController.clear()),
                          )
                        : null,
                  ),
                ),
                const SizedBox(height: 16),

                // Execution Mode Section
                Row(
                  children: [
                    const Icon(Icons.shield_outlined, size: 14, color: AppTheme.warningAmber),
                    const SizedBox(width: 6),
                    Text(
                      'EXECUTION MODE',
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.bold,
                        color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                        letterSpacing: 0.5,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Row(
                  children: [
                    Expanded(
                      child: _buildModeCard(
                        mode: 'default',
                        title: 'Default',
                        subtitle: 'Interactive approvals',
                        icon: Icons.verified_user_outlined,
                        color: AppTheme.primaryEmerald,
                        isSelected: _selectedMode == 'default',
                        isDark: isDark,
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: _buildModeCard(
                        mode: 'dangerously-skip-permissions',
                        title: '--dangerously-skip-permissions',
                        subtitle: 'Auto-approve tools',
                        icon: Icons.bolt,
                        color: AppTheme.warningAmber,
                        isSelected: _selectedMode == 'dangerously-skip-permissions',
                        isDark: isDark,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                  decoration: BoxDecoration(
                    color: _selectedMode == 'dangerously-skip-permissions'
                        ? AppTheme.warningAmber.withValues(alpha: 0.12)
                        : AppTheme.primaryEmerald.withValues(alpha: 0.10),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(
                      color: _selectedMode == 'dangerously-skip-permissions'
                          ? AppTheme.warningAmber.withValues(alpha: 0.3)
                          : AppTheme.primaryEmerald.withValues(alpha: 0.25),
                    ),
                  ),
                  child: Row(
                    children: [
                      Icon(
                        _selectedMode == 'dangerously-skip-permissions'
                            ? Icons.warning_amber_rounded
                            : Icons.info_outline,
                        size: 15,
                        color: _selectedMode == 'dangerously-skip-permissions'
                            ? AppTheme.warningAmber
                            : AppTheme.primaryEmerald,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          _selectedMode == 'dangerously-skip-permissions'
                              ? '⚡ Auto-approve: Runs with --dangerously-skip-permissions. All tools and bash commands execute without prompting.'
                              : '🛡️ Safe Default: The agent asks for approval on your phone/laptop before executing bash commands or modifying files.',
                          style: TextStyle(
                            fontSize: 11,
                            color: _selectedMode == 'dangerously-skip-permissions'
                                ? (isDark ? Colors.amber.shade200 : Colors.amber.shade900)
                                : (isDark ? Colors.green.shade200 : Colors.green.shade900),
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 20),

                // Quick Slash Commands Carousel
                Row(
                  children: [
                    const Icon(Icons.flash_on, size: 14, color: AppTheme.primaryEmerald),
                    const SizedBox(width: 6),
                    Text(
                      'QUICK COMMANDS',
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.bold,
                        color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                        letterSpacing: 0.5,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                SingleChildScrollView(
                  scrollDirection: Axis.horizontal,
                  physics: const BouncingScrollPhysics(),
                  padding: const EdgeInsets.symmetric(horizontal: 2),
                  child: Row(
                    children: _slashCommands.map((cmd) {
                      return Padding(
                        padding: const EdgeInsets.only(right: 8),
                        child: ActionChip(
                          label: Text(
                            cmd,
                            style: const TextStyle(
                              fontFamily: 'monospace',
                              fontSize: 12,
                              fontWeight: FontWeight.bold,
                            ),
                          ),
                          backgroundColor: isDark
                              ? AppTheme.surfaceCardElevated
                              : Colors.grey.shade100,
                          side: BorderSide(
                            color: isDark ? AppTheme.cardBorder : Colors.grey.shade300,
                          ),
                          onPressed: () => _onSlashCommandTap(cmd),
                        ),
                      );
                    }).toList(),
                  ),
                ),
                const SizedBox(height: 16),

                // Prompt Input Box
                TextField(
                  controller: _promptController,
                  minLines: 5,
                  maxLines: 8,
                  decoration: InputDecoration(
                    labelText: 'Prompt / Instruction',
                    hintText: 'Describe what you want the agent to do...',
                    alignLabelWithHint: true,
                  ),
                ),
                const SizedBox(height: 8),

                // Active Model & Quota Pill at bottom of input
                InkWell(
                  onTap: () => AgentControlOverlay.show(
                    context,
                    connection: widget.connection,
                    currentModel: _activeModel,
                    currentEffort: _activeEffort,
                    onModelChanged: (m) => setState(() => _activeModel = m),
                    onEffortChanged: (e) => setState(() => _activeEffort = e),
                  ),
                  borderRadius: BorderRadius.circular(10),
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                    decoration: BoxDecoration(
                      color: isDark ? AppTheme.surfaceCard : Colors.grey.shade100,
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(
                        color: isDark ? AppTheme.cardBorder : Colors.grey.shade300,
                      ),
                    ),
                    child: Row(
                      children: [
                        const Icon(Icons.psychology, size: 16, color: AppTheme.effortPurple),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Builder(
                            builder: (context) {
                              final modelInfo = AgentControlOverlay.getModelInfo(_activeModel);
                              final displayText = modelInfo.supportsEffort
                                  ? '${modelInfo.name} • Effort: ${_activeEffort.toUpperCase()}'
                                  : modelInfo.name;
                              return Text(
                                displayText,
                                style: TextStyle(
                                  fontSize: 11,
                                  fontFamily: 'monospace',
                                  color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                                ),
                                overflow: TextOverflow.ellipsis,
                              );
                            },
                          ),
                        ),
                        const Icon(Icons.arrow_drop_down, size: 18, color: AppTheme.textSecondary),
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 20),

                // Task Presets
                Row(
                  children: [
                    const Icon(Icons.auto_awesome, size: 14, color: AppTheme.warningAmber),
                    const SizedBox(width: 6),
                    Text(
                      'TASK PRESETS',
                      style: TextStyle(
                        fontSize: 11,
                        fontWeight: FontWeight.bold,
                        color: isDark ? AppTheme.textSecondary : Colors.grey.shade700,
                        letterSpacing: 0.5,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: _presets.map((preset) {
                    final isSelected = _promptController.text.trim() == preset['prompt']!.trim();
                    return FilterChip(
                      selected: isSelected,
                      showCheckmark: isSelected,
                      checkmarkColor: Colors.black,
                      label: Text(
                        preset['label']!,
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: isSelected ? FontWeight.bold : FontWeight.w500,
                          color: isSelected
                              ? Colors.black
                              : (isDark ? AppTheme.textPrimary : Colors.black87),
                        ),
                      ),
                      selectedColor: AppTheme.primaryEmerald,
                      backgroundColor: isDark ? AppTheme.surfaceCard : Colors.white,
                      side: BorderSide(
                        color: isSelected
                            ? AppTheme.primaryEmerald
                            : (isDark ? AppTheme.cardBorder : Colors.grey.shade300),
                        width: isSelected ? 1.5 : 1.0,
                      ),
                      onSelected: (_) => _applyPreset(preset['prompt']!),
                    );
                  }).toList(),
                ),
                const SizedBox(height: 24),

                // Launch Button
                FilledButton.icon(
                  onPressed: (isConnected && !_isSubmitting) ? _submitPrompt : null,
                  style: FilledButton.styleFrom(
                    backgroundColor: AppTheme.primaryEmerald,
                    foregroundColor: Colors.black,
                    padding: const EdgeInsets.symmetric(vertical: 16),
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(14),
                    ),
                  ),
                  icon: _isSubmitting
                      ? const SizedBox(
                          width: 18,
                          height: 18,
                          child: CircularProgressIndicator(strokeWidth: 2, color: Colors.black),
                        )
                      : const Icon(Icons.rocket_launch, size: 20),
                  label: Text(
                    _isSubmitting ? 'Launching on Laptop...' : 'Launch Task on Laptop',
                    style: const TextStyle(fontWeight: FontWeight.bold, fontSize: 15),
                  ),
                ),

                if (!isConnected) ...[
                  const SizedBox(height: 14),
                  Container(
                    padding: const EdgeInsets.all(12),
                    decoration: BoxDecoration(
                      color: AppTheme.errorRed.withValues(alpha: 0.1),
                      borderRadius: BorderRadius.circular(10),
                      border: Border.all(color: AppTheme.errorRed.withValues(alpha: 0.3)),
                    ),
                    child: const Row(
                      children: [
                        Icon(Icons.wifi_off, size: 16, color: AppTheme.errorRed),
                        SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            'Phone is not connected to AgentDesk. Please pair or connect first.',
                            style: TextStyle(color: AppTheme.errorRed, fontSize: 12),
                          ),
                        ),
                      ],
                    ),
                  ),
                ],
              ],
            ),
          ),
        );
      },
    );
  }
}
