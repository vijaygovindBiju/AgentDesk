import 'package:flutter/material.dart';
import '../services/connection_service.dart';
import 'app_theme.dart';

class AgentModelInfo {
  final String id;
  final String name;
  final String badge;
  final String desc;
  final List<String> supportedEfforts;

  const AgentModelInfo({
    required this.id,
    required this.name,
    required this.badge,
    required this.desc,
    this.supportedEfforts = const [],
  });

  bool get supportsEffort => supportedEfforts.isNotEmpty;
}

class AgentControlOverlay extends StatefulWidget {
  final ConnectionService connection;
  final String currentModel;
  final String currentEffort;
  final ValueChanged<String>? onModelChanged;
  final ValueChanged<String>? onEffortChanged;

  static const List<AgentModelInfo> availableModels = [
    AgentModelInfo(
      id: 'gemini-3.8-flash',
      name: 'Gemini 3.8 Flash',
      badge: 'Recommended',
      desc: 'Fastest response & multimodal agent execution (Default)',
      supportedEfforts: ['low', 'medium', 'high'],
    ),
    AgentModelInfo(
      id: 'gemini-3.7-flash',
      name: 'Gemini 3.7 Flash',
      badge: 'Fast',
      desc: 'Reliable fast agent for everyday workflows',
      supportedEfforts: ['low', 'medium', 'high'],
    ),
    AgentModelInfo(
      id: 'gemini-3.1-pro',
      name: 'Gemini 3.1 Pro',
      badge: 'Pro Reasoning',
      desc: 'Deep multi-step reasoning & architecture design',
      supportedEfforts: ['low', 'high'],
    ),
    AgentModelInfo(
      id: 'claude-sonnet-4-6',
      name: 'Claude Sonnet 4.6',
      badge: 'Anthropic',
      desc: 'Anthropic powerhouse for complex codebases',
      supportedEfforts: [],
    ),
    AgentModelInfo(
      id: 'claude-opus-4-6-thinking',
      name: 'Claude Opus 4.6',
      badge: 'Max Reasoning',
      desc: 'Full chain-of-thought exploration & verification',
      supportedEfforts: [],
    ),
    AgentModelInfo(
      id: 'gpt-oss-120b-medium',
      name: 'GPT-OSS 120B',
      badge: 'Open Weights',
      desc: 'Local & open-source compute model',
      supportedEfforts: [],
    ),
  ];

  static String normalizeModelId(String modelId) {
    if (modelId.startsWith('gemini-3.8-flash')) return 'gemini-3.8-flash';
    if (modelId.startsWith('gemini-3.7-flash')) return 'gemini-3.7-flash';
    if (modelId.startsWith('gemini-3.6-flash')) return 'gemini-3.6-flash';
    if (modelId.startsWith('gemini-3.1-pro')) return 'gemini-3.1-pro';
    if (modelId.startsWith('gpt-oss-120b')) return 'gpt-oss-120b-medium';
    return modelId;
  }

  static AgentModelInfo getModelInfo(String modelId) {
    final norm = normalizeModelId(modelId);
    return availableModels.firstWhere(
      (m) => m.id == norm || m.id == modelId,
      orElse: () => AgentModelInfo(
        id: modelId,
        name: modelId,
        badge: 'Custom',
        desc: 'Active model configuration',
        supportedEfforts: const [],
      ),
    );
  }

  const AgentControlOverlay({
    super.key,
    required this.connection,
    this.currentModel = 'gemini-3.8-flash',
    this.currentEffort = 'high',
    this.onModelChanged,
    this.onEffortChanged,
  });

  static Future<void> show(
    BuildContext context, {
    required ConnectionService connection,
    String currentModel = 'gemini-3.8-flash',
    String currentEffort = 'high',
    ValueChanged<String>? onModelChanged,
    ValueChanged<String>? onEffortChanged,
  }) {
    return showModalBottomSheet(
      context: context,
      isScrollControlled: true,
      backgroundColor: Colors.transparent,
      builder: (context) => AgentControlOverlay(
        connection: connection,
        currentModel: currentModel,
        currentEffort: currentEffort,
        onModelChanged: onModelChanged,
        onEffortChanged: onEffortChanged,
      ),
    );
  }

  @override
  State<AgentControlOverlay> createState() => _AgentControlOverlayState();
}

class _AgentControlOverlayState extends State<AgentControlOverlay> {
  late String _selectedModel;
  late String _selectedEffort;
  bool _isApplying = false;

  @override
  void initState() {
    super.initState();
    _selectedModel = AgentControlOverlay.normalizeModelId(widget.currentModel);
    _selectedEffort = widget.currentEffort;

    final info = AgentControlOverlay.getModelInfo(_selectedModel);
    if (info.supportsEffort && !info.supportedEfforts.contains(_selectedEffort)) {
      _selectedEffort = info.supportedEfforts.contains('high')
          ? 'high'
          : info.supportedEfforts.first;
    }
  }

  String _effortDescription(String effort) {
    switch (effort) {
      case 'low':
        return 'Minimal reasoning tokens; quickest response for simple edits.';
      case 'medium':
        return 'Balanced verification and speed for routine tasks.';
      case 'high':
        return 'Solves complex coding tasks, checks edge cases, and verifies with terminal tests.';
      case 'xhigh':
        return 'Extended thinking for intricate algorithms and multi-file refactors.';
      case 'max':
        return 'Exhaustive deliberation with full self-critique loops.';
      default:
        return 'Standard effort level.';
    }
  }

  Future<void> _applySettings() async {
    setState(() => _isApplying = true);
    final modelInfo = AgentControlOverlay.getModelInfo(_selectedModel);
    try {
      widget.onModelChanged?.call(_selectedModel);
      widget.onEffortChanged?.call(_selectedEffort);

      if (widget.connection.status == ConnectionStatus.connected) {
        if (modelInfo.supportsEffort) {
          // Send /model and /effort commands for models supporting reasoning effort
          await widget.connection.startTask(
            prompt: '/model $_selectedModel-$_selectedEffort\n/effort $_selectedEffort',
          );
        } else {
          // Model does not have reasoning effort
          await widget.connection.startTask(
            prompt: '/model $_selectedModel',
          );
        }
      }

      if (mounted) {
        Navigator.of(context).pop();
        final snackMsg = modelInfo.supportsEffort
            ? 'Model set to ${modelInfo.name} (Effort: ${_selectedEffort.toUpperCase()}) ⚡'
            : 'Model set to ${modelInfo.name} ⚡';
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: Text(snackMsg),
            backgroundColor: AppTheme.primaryEmerald,
          ),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Error: $e'), backgroundColor: Colors.red),
        );
      }
    } finally {
      if (mounted) setState(() => _isApplying = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final bottomInset = MediaQuery.of(context).viewInsets.bottom;
    final modelInfo = AgentControlOverlay.getModelInfo(_selectedModel);

    return Container(
      constraints: BoxConstraints(
        maxHeight: MediaQuery.of(context).size.height * 0.88,
      ),
      padding: EdgeInsets.fromLTRB(20, 12, 20, bottomInset > 0 ? bottomInset + 16 : 24),
      decoration: const BoxDecoration(
        color: AppTheme.darkBg,
        borderRadius: BorderRadius.vertical(top: Radius.circular(24)),
        border: Border(
          top: BorderSide(color: AppTheme.cardBorder, width: 1.5),
          left: BorderSide(color: AppTheme.cardBorder, width: 1),
          right: BorderSide(color: AppTheme.cardBorder, width: 1),
        ),
      ),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // Drag handle
          Center(
            child: Container(
              width: 44,
              height: 4,
              decoration: BoxDecoration(
                color: AppTheme.textTertiary,
                borderRadius: BorderRadius.circular(2),
              ),
            ),
          ),
          const SizedBox(height: 14),

          // Header
          Row(
            children: [
              Container(
                padding: const EdgeInsets.all(8),
                decoration: BoxDecoration(
                  color: AppTheme.primaryEmerald.withValues(alpha: 0.15),
                  borderRadius: BorderRadius.circular(10),
                ),
                child: const Icon(
                  Icons.tune,
                  color: AppTheme.primaryEmerald,
                  size: 20,
                ),
              ),
              const SizedBox(width: 12),
              const Expanded(
                child: Text(
                  'Agent Controls',
                  style: TextStyle(
                    fontSize: 18,
                    fontWeight: FontWeight.bold,
                    color: AppTheme.textPrimary,
                  ),
                ),
              ),
              IconButton(
                icon: const Icon(Icons.close, color: AppTheme.textSecondary, size: 20),
                onPressed: () => Navigator.of(context).pop(),
                visualDensity: VisualDensity.compact,
              ),
            ],
          ),
          const SizedBox(height: 16),

          // Content scroll area
          Flexible(
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  // SECTION 1: DAEMON STATUS
                  _buildSectionHeader('DAEMON CONNECTION'),
                  const SizedBox(height: 8),
                  _buildSessionInfoCard(),
                  const SizedBox(height: 20),

                  // SECTION 2: MODEL SELECTION
                  _buildSectionHeader('ACTIVE MODEL (/model)'),
                  const SizedBox(height: 8),
                  ...AgentControlOverlay.availableModels.map((m) => _buildModelTile(m)),
                  const SizedBox(height: 20),

                  // SECTION 3: REASONING EFFORT (ONLY SHOWN IF MODEL SUPPORTS EFFORT)
                  if (modelInfo.supportsEffort) ...[
                    _buildSectionHeader('REASONING EFFORT (${modelInfo.name.toUpperCase()})'),
                    const SizedBox(height: 8),
                    _buildEffortSelector(modelInfo.supportedEfforts),
                    const SizedBox(height: 6),
                    Text(
                      _effortDescription(_selectedEffort),
                      style: const TextStyle(
                        fontSize: 12,
                        color: AppTheme.textSecondary,
                        fontStyle: FontStyle.italic,
                      ),
                    ),
                    const SizedBox(height: 20),
                  ],
                ],
              ),
            ),
          ),
          const SizedBox(height: 16),

          // Footer action buttons
          Row(
            children: [
              Expanded(
                child: OutlinedButton(
                  onPressed: () => Navigator.of(context).pop(),
                  style: OutlinedButton.styleFrom(
                    side: const BorderSide(color: AppTheme.cardBorder),
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(12),
                    ),
                    padding: const EdgeInsets.symmetric(vertical: 14),
                  ),
                  child: const Text(
                    'Cancel',
                    style: TextStyle(color: AppTheme.textSecondary),
                  ),
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                flex: 2,
                child: ElevatedButton(
                  onPressed: _isApplying ? null : _applySettings,
                  style: ElevatedButton.styleFrom(
                    backgroundColor: AppTheme.primaryEmerald,
                    foregroundColor: Colors.black,
                    shape: RoundedRectangleBorder(
                      borderRadius: BorderRadius.circular(12),
                    ),
                    padding: const EdgeInsets.symmetric(vertical: 14),
                    elevation: 0,
                  ),
                  child: _isApplying
                      ? const SizedBox(
                          height: 18,
                          width: 18,
                          child: CircularProgressIndicator(
                            strokeWidth: 2,
                            valueColor: AlwaysStoppedAnimation<Color>(Colors.black),
                          ),
                        )
                      : const Text(
                          'Apply & Save Settings',
                          style: TextStyle(fontWeight: FontWeight.bold, fontSize: 14),
                        ),
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildSectionHeader(String title) {
    return Text(
      title,
      style: const TextStyle(
        fontSize: 11,
        fontWeight: FontWeight.bold,
        color: AppTheme.textSecondary,
        letterSpacing: 0.8,
      ),
    );
  }

  Widget _buildSessionInfoCard() {
    final isConnected = widget.connection.status == ConnectionStatus.connected;
    final daemonVer = widget.connection.state.welcome?.daemonVersion ?? '0.1.0';
    final pipeline = widget.connection.state.welcome?.pipelineMode.name.toUpperCase() ?? 'AGENTDESK';
    final transport = widget.connection.state.welcome?.transport.name.toUpperCase() ?? 'RELAY';

    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      decoration: BoxDecoration(
        color: AppTheme.surfaceCard,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.cardBorder),
      ),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          Row(
            children: [
              Container(
                width: 8,
                height: 8,
                decoration: BoxDecoration(
                  color: isConnected ? AppTheme.primaryEmerald : AppTheme.errorRed,
                  shape: BoxShape.circle,
                ),
              ),
              const SizedBox(width: 8),
              Text(
                isConnected ? 'CONNECTED' : 'STANDBY',
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.bold,
                  letterSpacing: 0.5,
                  color: isConnected ? AppTheme.primaryEmerald : AppTheme.errorRed,
                ),
              ),
            ],
          ),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
            decoration: BoxDecoration(
              color: AppTheme.cardBorder.withValues(alpha: 0.5),
              borderRadius: BorderRadius.circular(6),
            ),
            child: Text(
              'v$daemonVer • $transport • $pipeline',
              style: const TextStyle(
                fontSize: 10,
                fontFamily: 'monospace',
                fontWeight: FontWeight.w600,
                color: AppTheme.textSecondary,
              ),
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildEffortSelector(List<String> availableEfforts) {
    return Container(
      padding: const EdgeInsets.all(4),
      decoration: BoxDecoration(
        color: AppTheme.surfaceCard,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(color: AppTheme.cardBorder),
      ),
      child: Row(
        children: availableEfforts.map((lvl) {
          final isSelected = _selectedEffort == lvl;
          return Expanded(
            child: InkWell(
              onTap: () => setState(() => _selectedEffort = lvl),
              borderRadius: BorderRadius.circular(8),
              child: AnimatedContainer(
                duration: const Duration(milliseconds: 150),
                padding: const EdgeInsets.symmetric(vertical: 8),
                decoration: BoxDecoration(
                  color: isSelected ? AppTheme.effortPurple : Colors.transparent,
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Center(
                  child: Text(
                    lvl.toUpperCase(),
                    style: TextStyle(
                      fontSize: 11,
                      fontWeight: isSelected ? FontWeight.bold : FontWeight.w500,
                      color: isSelected ? Colors.white : AppTheme.textSecondary,
                    ),
                  ),
                ),
              ),
            ),
          );
        }).toList(),
      ),
    );
  }

  Widget _buildModelTile(AgentModelInfo m) {
    final isSelected = _selectedModel == m.id;
    return Container(
      margin: const EdgeInsets.only(bottom: 8),
      decoration: BoxDecoration(
        color: isSelected ? AppTheme.surfaceCardElevated : AppTheme.surfaceCard,
        borderRadius: BorderRadius.circular(12),
        border: Border.all(
          color: isSelected ? AppTheme.primaryEmerald : AppTheme.cardBorder,
          width: isSelected ? 1.5 : 1.0,
        ),
      ),
      child: InkWell(
        onTap: () {
          setState(() {
            _selectedModel = m.id;
            if (m.supportsEffort && !m.supportedEfforts.contains(_selectedEffort)) {
              _selectedEffort = m.supportedEfforts.contains('high')
                  ? 'high'
                  : m.supportedEfforts.first;
            }
          });
        },
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
          child: Row(
            children: [
              Container(
                width: 20,
                height: 20,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  border: Border.all(
                    color: isSelected ? AppTheme.primaryEmerald : AppTheme.textTertiary,
                    width: 2,
                  ),
                ),
                child: isSelected
                    ? Center(
                        child: Container(
                          width: 10,
                          height: 10,
                          decoration: const BoxDecoration(
                            shape: BoxShape.circle,
                            color: AppTheme.primaryEmerald,
                          ),
                        ),
                      )
                    : null,
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Flexible(
                          child: Text(
                            m.name,
                            style: const TextStyle(
                              fontSize: 13,
                              fontWeight: FontWeight.bold,
                              fontFamily: 'monospace',
                              color: AppTheme.textPrimary,
                            ),
                          ),
                        ),
                        const SizedBox(width: 8),
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 6,
                            vertical: 2,
                          ),
                          decoration: BoxDecoration(
                            color: isSelected
                                ? AppTheme.primaryEmerald.withValues(alpha: 0.2)
                                : AppTheme.cardBorder,
                            borderRadius: BorderRadius.circular(6),
                          ),
                          child: Text(
                            m.badge,
                            style: TextStyle(
                              fontSize: 9,
                              fontWeight: FontWeight.bold,
                              color: isSelected
                                  ? AppTheme.primaryEmerald
                                  : AppTheme.textSecondary,
                            ),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 3),
                    Text(
                      m.desc,
                      style: const TextStyle(
                        fontSize: 11,
                        color: AppTheme.textSecondary,
                      ),
                    ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
