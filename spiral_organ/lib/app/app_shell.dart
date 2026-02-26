import 'dart:async';
import 'dart:convert';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import '../bloc/bloc.dart';
import '../core/network/kernel_client.dart';
import '../features/notifications/notification_cubit.dart';
import '../features/notifications/notification_state.dart';
import '../features/session/session_cubit.dart';
import '../features/session/session_state.dart';
import '../widgets/json_viewer.dart';
import '../widgets/panel_card.dart';

enum _WorkspaceTabKind { globalControl, kanban, chat, process, skills }

enum _StickyStatus { running, done, failed, cancelled }

class _WorkspaceTabItem {
  const _WorkspaceTabItem({
    required this.id,
    required this.title,
    required this.kind,
    required this.closable,
  });

  final String id;
  final String title;
  final _WorkspaceTabKind kind;
  final bool closable;
}

class _ChatMessage {
  const _ChatMessage({
    required this.id,
    required this.fromUser,
    required this.text,
    required this.createdAt,
  });

  final String id;
  final bool fromUser;
  final String text;
  final DateTime createdAt;
}

class _TaskDraft {
  const _TaskDraft({required this.title, required this.category});

  final String title;
  final String category;
}

class _KanbanSticky {
  const _KanbanSticky({
    required this.id,
    required this.chatTabId,
    required this.suggestedTaskId,
    required this.title,
    required this.prompt,
    required this.category,
    required this.status,
    required this.createdAt,
    required this.updatedAt,
    this.taskId = '',
    this.engineStatus = 'running',
    this.note = '',
    this.error,
    this.busy = false,
    this.runCount = 0,
    this.seenEventIds = const <String>{},
  });

  final String id;
  final String chatTabId;
  final String suggestedTaskId;
  final String title;
  final String prompt;
  final String category;
  final _StickyStatus status;
  final DateTime createdAt;
  final DateTime updatedAt;
  final String taskId;
  final String engineStatus;
  final String note;
  final String? error;
  final bool busy;
  final int runCount;
  final Set<String> seenEventIds;

  _KanbanSticky copyWith({
    String? taskId,
    String? engineStatus,
    String? note,
    String? error,
    bool clearError = false,
    bool? busy,
    _StickyStatus? status,
    DateTime? updatedAt,
    int? runCount,
    Set<String>? seenEventIds,
  }) {
    return _KanbanSticky(
      id: id,
      chatTabId: chatTabId,
      suggestedTaskId: suggestedTaskId,
      title: title,
      prompt: prompt,
      category: category,
      status: status ?? this.status,
      createdAt: createdAt,
      updatedAt: updatedAt ?? this.updatedAt,
      taskId: taskId ?? this.taskId,
      engineStatus: engineStatus ?? this.engineStatus,
      note: note ?? this.note,
      error: clearError ? null : (error ?? this.error),
      busy: busy ?? this.busy,
      runCount: runCount ?? this.runCount,
      seenEventIds: seenEventIds ?? this.seenEventIds,
    );
  }
}

class _DispatcherChoice {
  const _DispatcherChoice({
    required this.kind,
    required this.ref,
    required this.label,
    this.subtitle,
  });

  final String kind;
  final String ref;
  final String label;
  final String? subtitle;

  String get value => '$kind::$ref';
}

class _WorkspaceState {
  const _WorkspaceState({
    required this.id,
    required this.name,
    required this.projectId,
    required this.target,
    required this.dispatcherKind,
    required this.dispatcherRef,
    required this.sessionId,
    required this.sessionStatus,
    required this.tabs,
    required this.selectedTabIndex,
    required this.chatMessages,
    required this.stickies,
  });

  final String id;
  final String name;
  final String projectId;
  final String target;
  final String dispatcherKind;
  final String dispatcherRef;
  final String sessionId;
  final String sessionStatus;
  final List<_WorkspaceTabItem> tabs;
  final int selectedTabIndex;
  final Map<String, List<_ChatMessage>> chatMessages;
  final List<_KanbanSticky> stickies;

  _WorkspaceState copyWith({
    String? name,
    String? projectId,
    String? target,
    String? dispatcherKind,
    String? dispatcherRef,
    String? sessionId,
    String? sessionStatus,
    List<_WorkspaceTabItem>? tabs,
    int? selectedTabIndex,
    Map<String, List<_ChatMessage>>? chatMessages,
    List<_KanbanSticky>? stickies,
  }) {
    return _WorkspaceState(
      id: id,
      name: name ?? this.name,
      projectId: projectId ?? this.projectId,
      target: target ?? this.target,
      dispatcherKind: dispatcherKind ?? this.dispatcherKind,
      dispatcherRef: dispatcherRef ?? this.dispatcherRef,
      sessionId: sessionId ?? this.sessionId,
      sessionStatus: sessionStatus ?? this.sessionStatus,
      tabs: tabs ?? this.tabs,
      selectedTabIndex: selectedTabIndex ?? this.selectedTabIndex,
      chatMessages: chatMessages ?? this.chatMessages,
      stickies: stickies ?? this.stickies,
    );
  }
}

class AppShell extends StatefulWidget {
  const AppShell({super.key});

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  static const _pollInterval = Duration(seconds: 3);
  static const _stickyWatchInterval = Duration(seconds: 2);
  static const _stickyWatchAttemptsPadding = 30;
  static const _uiLogPreviewMaxChars = 320;
  static const _workspaceMappingRoot = '.runtime/workspaces';

  Timer? _pollTimer;
  Timer? _eventRetryTimer;
  bool _pollInFlight = false;
  bool _kernelEventsConnected = false;
  StreamSubscription<Map<String, dynamic>>? _kernelEventSubscription;
  String? _kernelEventWorkspaceId;
  String? _kernelEventSessionId;
  final Set<String> _stickyWatchers = <String>{};

  int _workspaceSeed = 2;
  int _chatSeed = 2;
  int _stickySeed = 1;
  int _messageSeed = 1;

  List<_WorkspaceState> _workspaces = const <_WorkspaceState>[];
  String? _activeWorkspaceId;

  late final TextEditingController _workspaceNameController;
  late final TextEditingController _projectIdController;
  late final TextEditingController _targetController;
  late final TextEditingController _sessionIdController;
  late final TextEditingController _baseUrlController;
  late final TextEditingController _tokenController;
  late final TextEditingController _chatInputController;
  late final TextEditingController _noResponseTimeoutController;

  bool _connectionAutoRefresh = false;
  int _noResponseTimeoutMinutes = 5;

  List<RuntimeProviderConfig> _runtimeProviders =
      const <RuntimeProviderConfig>[];
  List<LocalClientInfo> _localClients = const <LocalClientInfo>[];
  List<SshTargetConfig> _sshTargets = const <SshTargetConfig>[];
  List<KernelSkill> _skills = const <KernelSkill>[];
  List<Map<String, dynamic>> _skillStoreItems = const <Map<String, dynamic>>[];
  List<SandboxApproval> _sandboxApprovals = const <SandboxApproval>[];
  List<LocalClientProcessSnapshot> _clientProcesses =
      const <LocalClientProcessSnapshot>[];
  int _clientProcessesLastUpdatedMs = 0;
  bool _clientProcessesBusy = false;
  String? _clientProcessesError;
  bool _runtimeBusy = false;
  String? _runtimeError;
  bool _skillsBusy = false;
  String? _skillsError;
  bool _sandboxApprovalsBusy = false;
  bool _sandboxActionBusy = false;
  String? _sandboxError;

  @override
  void initState() {
    super.initState();

    final settings = context.read<SettingCubit>().state;
    final session = context.read<SessionCubit>().state;

    final main = _createWorkspace(
      id: 'ws-1',
      name: 'Main',
      projectId: settings.projectId,
      target: settings.target,
      sessionId: session.sessionId,
      sessionStatus: session.status,
    );

    _workspaces = <_WorkspaceState>[main];
    _activeWorkspaceId = main.id;

    _workspaceNameController = TextEditingController(text: main.name);
    _projectIdController = TextEditingController(text: main.projectId);
    _targetController = TextEditingController(text: main.target);
    _sessionIdController = TextEditingController(text: main.sessionId);
    _baseUrlController = TextEditingController(text: settings.baseUrl);
    _tokenController = TextEditingController(text: settings.token);
    _chatInputController = TextEditingController();
    _noResponseTimeoutController = TextEditingController(
      text: settings.noResponseTimeoutMinutes.toString(),
    );

    _connectionAutoRefresh = settings.autoRefresh;
    _noResponseTimeoutMinutes = settings.noResponseTimeoutMinutes;

    _syncPolling(_connectionAutoRefresh);
    _syncEventSubscription(force: true);
    unawaited(
      _ensureWorkspaceFolder(workspaceId: main.id, target: main.target),
    );
    unawaited(_loadRuntimeConfiguration(workspaceId: main.id));
    unawaited(_refreshSkills(silent: true));
    unawaited(_refreshSandboxApprovals(silent: true));
    unawaited(_refreshClientProcesses(silent: true));
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    _eventRetryTimer?.cancel();
    _kernelEventSubscription?.cancel();
    _stickyWatchers.clear();

    _workspaceNameController.dispose();
    _projectIdController.dispose();
    _targetController.dispose();
    _sessionIdController.dispose();
    _baseUrlController.dispose();
    _tokenController.dispose();
    _chatInputController.dispose();
    _noResponseTimeoutController.dispose();

    super.dispose();
  }

  _WorkspaceState _createWorkspace({
    required String id,
    required String name,
    required String projectId,
    String? target,
    String sessionId = '',
    String sessionStatus = 'idle',
  }) {
    final mappedTarget = _workspaceMappedFolderPath(id);
    final resolvedTarget = (target?.trim().isNotEmpty ?? false)
        ? target!.trim()
        : (mappedTarget.isEmpty ? _workspaceMappingRoot : mappedTarget);

    return _WorkspaceState(
      id: id,
      name: name,
      projectId: projectId,
      target: resolvedTarget,
      dispatcherKind: 'provider',
      dispatcherRef: 'default',
      sessionId: sessionId,
      sessionStatus: sessionStatus,
      tabs: const <_WorkspaceTabItem>[
        _WorkspaceTabItem(
          id: 'global-control',
          title: 'Global Control',
          kind: _WorkspaceTabKind.globalControl,
          closable: false,
        ),
        _WorkspaceTabItem(
          id: 'kanban-doc',
          title: 'Kanban',
          kind: _WorkspaceTabKind.kanban,
          closable: false,
        ),
        _WorkspaceTabItem(
          id: 'chat-home',
          title: 'Chat Home',
          kind: _WorkspaceTabKind.chat,
          closable: false,
        ),
        _WorkspaceTabItem(
          id: 'process-monitor',
          title: 'Processes',
          kind: _WorkspaceTabKind.process,
          closable: false,
        ),
        _WorkspaceTabItem(
          id: 'skills-center',
          title: 'Skills',
          kind: _WorkspaceTabKind.skills,
          closable: false,
        ),
      ],
      selectedTabIndex: 0,
      chatMessages: <String, List<_ChatMessage>>{'chat-home': <_ChatMessage>[]},
      stickies: const <_KanbanSticky>[],
    );
  }

  String _workspaceMappedFolderPath(String workspaceId) {
    final trimmed = workspaceId.trim();
    final normalized = trimmed.isEmpty ? 'workspace' : trimmed;
    final safeName = normalized.replaceAll(RegExp(r'[^a-zA-Z0-9._-]'), '-');
    return '$_workspaceMappingRoot/$safeName';
  }

  String _previewForLog(String value) {
    final singleLine = value.replaceAll('\n', r'\n');
    if (singleLine.length <= _uiLogPreviewMaxChars) {
      return singleLine;
    }
    return '${singleLine.substring(0, _uiLogPreviewMaxChars)}...(truncated)';
  }

  String _formatProcessTimestamp(int timestampMs) {
    if (timestampMs <= 0) {
      return '-';
    }
    final time = DateTime.fromMillisecondsSinceEpoch(timestampMs);
    final hh = time.hour.toString().padLeft(2, '0');
    final mm = time.minute.toString().padLeft(2, '0');
    final ss = time.second.toString().padLeft(2, '0');
    return '${time.year}-${time.month.toString().padLeft(2, '0')}-${time.day.toString().padLeft(2, '0')} $hh:$mm:$ss';
  }

  String _formatPercent(double? value) {
    if (value == null) {
      return '-';
    }
    return '${value.toStringAsFixed(1)}%';
  }

  Future<void> _refreshClientProcesses({bool silent = false}) async {
    if (_clientProcessesBusy) {
      return;
    }

    final client = _client(context);
    final globalCubit = context.read<GlobalCubit>();
    setState(() {
      _clientProcessesBusy = true;
      if (!silent) {
        _clientProcessesError = null;
      }
    });

    try {
      final processes = await client.listLocalClientProcesses();
      final updatedAt = DateTime.now().millisecondsSinceEpoch;
      if (!mounted) {
        return;
      }

      setState(() {
        _clientProcesses = processes;
        _clientProcessesLastUpdatedMs = updatedAt;
        _clientProcessesBusy = false;
        _clientProcessesError = null;
      });

      if (!silent) {
        final processCount = processes.fold<int>(
          0,
          (sum, item) => sum + item.processes.length,
        );
        globalCubit.appendLog(
          'process-monitor refresh: dispatches=${processes.length} processes=$processCount',
        );
      }
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _clientProcessesBusy = false;
        _clientProcessesError = err.toString();
      });
      if (!silent) {
        globalCubit.appendLog('process-monitor failed: $err');
      }
    }
  }

  String _nextStickyId() {
    final id = _stickySeed;
    _stickySeed += 1;
    return 'sticky-$id';
  }

  String _nextMessageId() {
    final id = _messageSeed;
    _messageSeed += 1;
    return 'msg-$id';
  }

  _WorkspaceState? _workspaceById(String workspaceId) {
    for (final item in _workspaces) {
      if (item.id == workspaceId) {
        return item;
      }
    }
    return null;
  }

  _WorkspaceState? get _activeWorkspace {
    final id = _activeWorkspaceId;
    if (id == null) {
      return _workspaces.isEmpty ? null : _workspaces.first;
    }
    return _workspaceById(id) ??
        (_workspaces.isEmpty ? null : _workspaces.first);
  }

  void _updateWorkspaceById(
    String workspaceId,
    _WorkspaceState Function(_WorkspaceState current) mapper,
  ) {
    setState(() {
      _workspaces = _workspaces.map((item) {
        if (item.id != workspaceId) {
          return item;
        }
        return mapper(item);
      }).toList();
    });
  }

  void _updateActiveWorkspace(
    _WorkspaceState Function(_WorkspaceState current) mapper,
  ) {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }
    _updateWorkspaceById(ws.id, mapper);
  }

  Map<String, List<_ChatMessage>> _cloneChatMessages(
    Map<String, List<_ChatMessage>> source,
  ) {
    final next = <String, List<_ChatMessage>>{};
    for (final entry in source.entries) {
      next[entry.key] = List<_ChatMessage>.from(entry.value);
    }
    return next;
  }

  KernelClient _client(BuildContext context) {
    final settings = context.read<SettingCubit>().state;
    return KernelClient(baseUrl: settings.baseUrl, token: settings.tokenOrNull);
  }

  List<_DispatcherChoice> _dispatcherChoices() {
    final localClientChoices =
        _localClients
            .where((item) => item.installed && item.supportsDispatch)
            .toList()
          ..sort((left, right) {
            final leftIsCodex = left.id == 'codex';
            final rightIsCodex = right.id == 'codex';
            if (leftIsCodex != rightIsCodex) {
              return leftIsCodex ? -1 : 1;
            }
            return left.name.toLowerCase().compareTo(right.name.toLowerCase());
          });

    final localDispatcherChoices = localClientChoices.map(
      (item) => _DispatcherChoice(
        kind: 'local_client',
        ref: item.id,
        label: item.name,
        subtitle: item.command,
      ),
    );

    final providerChoices = _runtimeProviders
        .where((item) => item.id != null)
        .map(
          (item) => _DispatcherChoice(
            kind: 'provider',
            ref: item.id!.toString(),
            label: item.name,
            subtitle: '${item.kind} / ${item.model}',
          ),
        );

    final sshChoices =
        _sshTargets.where((item) => item.sshTargetId.trim().isNotEmpty).toList()
          ..sort(
            (left, right) =>
                left.name.toLowerCase().compareTo(right.name.toLowerCase()),
          );
    final sshDispatcherChoices = sshChoices.map(
      (item) => _DispatcherChoice(
        kind: 'ssh',
        ref: item.sshTargetId,
        label: 'SSH · ${item.name}',
        subtitle: item.host,
      ),
    );

    final merged = <_DispatcherChoice>[
      ...providerChoices,
      ...sshDispatcherChoices,
      ...localDispatcherChoices,
    ];

    if (merged.isEmpty) {
      return const <_DispatcherChoice>[
        _DispatcherChoice(kind: 'provider', ref: 'default', label: 'Default'),
        _DispatcherChoice(
          kind: 'local_client',
          ref: 'codex',
          label: 'Codex CLI',
          subtitle: 'default',
        ),
      ];
    }
    return merged;
  }

  List<_DispatcherChoice> _chatDispatcherChoices() {
    final filtered = _dispatcherChoices()
        .where(
          (item) =>
              item.kind == 'provider' ||
              (item.kind == 'local_client' && item.ref == 'codex'),
        )
        .toList();
    if (filtered.isEmpty) {
      return const <_DispatcherChoice>[
        _DispatcherChoice(kind: 'provider', ref: 'default', label: 'Default'),
        _DispatcherChoice(
          kind: 'local_client',
          ref: 'codex',
          label: 'Codex CLI',
          subtitle: 'default',
        ),
      ];
    }
    return filtered;
  }

  String _activeDispatcherValue() {
    final ws = _activeWorkspace;
    final choices = _dispatcherChoices();
    if (ws == null) {
      return choices.first.value;
    }

    final current = '${ws.dispatcherKind}::${ws.dispatcherRef}';
    if (choices.any((item) => item.value == current)) {
      return current;
    }
    return choices.first.value;
  }

  String _activeChatDispatcherValue(_WorkspaceState ws) {
    final choices = _chatDispatcherChoices();
    final current = '${ws.dispatcherKind}::${ws.dispatcherRef}';
    if (choices.any((item) => item.value == current)) {
      return current;
    }
    return choices.first.value;
  }

  Future<void> _loadRuntimeConfiguration({
    String? workspaceId,
    bool silent = false,
  }) async {
    final targetWorkspaceId = workspaceId ?? _activeWorkspaceId;
    if (targetWorkspaceId == null) {
      return;
    }

    final ws = _workspaceById(targetWorkspaceId);
    if (ws == null) {
      return;
    }

    if (!silent && mounted) {
      setState(() {
        _runtimeBusy = true;
        _runtimeError = null;
      });
    }

    final project = ws.projectId.trim();

    try {
      final client = _client(context);
      final providers = await client.listRuntimeProviders();
      final localClients = await client.listLocalClients();
      final sshTargets = await client.listRuntimeSshTargets();
      ProjectDispatcherConfig? dispatcher;
      if (project.isNotEmpty) {
        dispatcher = await client.getProjectDispatcher(project);
      }

      if (!mounted) {
        return;
      }

      setState(() {
        _runtimeProviders = providers;
        _localClients = localClients;
        _sshTargets = sshTargets;
        _runtimeBusy = false;
        _runtimeError = null;
      });

      if (dispatcher != null) {
        _updateWorkspaceById(
          targetWorkspaceId,
          (item) => item.copyWith(
            dispatcherKind: dispatcher!.targetKind,
            dispatcherRef: dispatcher.targetRef,
          ),
        );
      } else {
        final choices = _dispatcherChoices();
        final updated = _workspaceById(targetWorkspaceId);
        if (updated == null) {
          return;
        }
        final selected = '${updated.dispatcherKind}::${updated.dispatcherRef}';
        if (!choices.any((item) => item.value == selected)) {
          final fallback = choices.first;
          _updateWorkspaceById(
            targetWorkspaceId,
            (item) => item.copyWith(
              dispatcherKind: fallback.kind,
              dispatcherRef: fallback.ref,
            ),
          );
        }
      }
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _runtimeBusy = false;
        _runtimeError = err.toString();
      });
      context.read<GlobalCubit>().appendLog('runtime config load failed: $err');
    }
  }

  Future<void> _refreshSkills({bool silent = false}) async {
    if (_skillsBusy) {
      return;
    }

    if (mounted) {
      setState(() {
        _skillsBusy = true;
        if (!silent) {
          _skillsError = null;
        }
      });
    }

    final client = _client(context);
    final global = context.read<GlobalCubit>();

    try {
      final skills = await client.listSkills();
      final storeItems = await client.listSkillStoreItems();

      if (!mounted) {
        return;
      }

      setState(() {
        _skills = skills;
        _skillStoreItems = storeItems;
        _skillsBusy = false;
        _skillsError = null;
      });
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _skillsBusy = false;
        _skillsError = err.toString();
      });
      global.appendLog('skills refresh failed: $err');
    }
  }

  Future<void> _refreshSandboxApprovals({bool silent = false}) async {
    if (_sandboxApprovalsBusy) {
      return;
    }

    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final sessionId = ws.sessionId.trim();
    if (sessionId.isEmpty) {
      if (mounted) {
        setState(() {
          _sandboxApprovals = const <SandboxApproval>[];
          _sandboxApprovalsBusy = false;
          if (!silent) {
            _sandboxError = null;
          }
        });
      }
      return;
    }

    if (mounted) {
      setState(() {
        _sandboxApprovalsBusy = true;
        if (!silent) {
          _sandboxError = null;
        }
      });
    }

    final client = _client(context);
    final global = context.read<GlobalCubit>();

    try {
      final approvals = await client.listSandboxApprovals(sessionId: sessionId);
      if (!mounted) {
        return;
      }
      setState(() {
        _sandboxApprovals = approvals;
        _sandboxApprovalsBusy = false;
        _sandboxError = null;
      });
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _sandboxApprovalsBusy = false;
        _sandboxError = err.toString();
      });
      global.appendLog('sandbox approvals refresh failed: $err');
    }
  }

  List<String> _parseSandboxArgs(String raw) {
    final trimmed = raw.trim();
    if (trimmed.isEmpty) {
      return const <String>[];
    }
    return trimmed
        .split(RegExp(r'\s+'))
        .where((item) => item.trim().isNotEmpty)
        .toList();
  }

  String _previewSandboxResult(Map<String, dynamic>? result) {
    if (result == null || result.isEmpty) {
      return '{}';
    }
    final text = jsonEncode(result);
    if (text.length <= 160) {
      return text;
    }
    return '${text.substring(0, 160)}...(truncated)';
  }

  Future<void> _executeSandboxTool({
    required String operation,
    String? path,
    String? content,
    String? command,
    List<String> args = const <String>[],
    int? timeoutMs,
  }) async {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }
    final sessionId = ws.sessionId.trim();
    if (sessionId.isEmpty) {
      setState(() {
        _sandboxError = 'open a session before running sandbox actions';
      });
      return;
    }

    final client = _client(context);
    final global = context.read<GlobalCubit>();

    setState(() {
      _sandboxActionBusy = true;
      _sandboxError = null;
    });

    try {
      final result = await client.executeSandboxTool(
        SandboxToolExecuteRequest(
          sessionId: sessionId,
          operation: operation,
          path: path?.trim().isEmpty == true ? null : path?.trim(),
          content: content,
          command: command?.trim().isEmpty == true ? null : command?.trim(),
          args: args,
          timeoutMs: timeoutMs,
        ),
      );

      if (!mounted) {
        return;
      }

      if (result.approvalRequired) {
        global.appendLog(
          'sandbox action requires approval: op=${result.operation} approval=${result.approvalId}',
        );
      } else {
        global.appendLog(
          'sandbox action executed: op=${result.operation} result=${_previewSandboxResult(result.result)}',
        );
      }

      await _refreshSandboxApprovals(silent: true);
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _sandboxError = err.toString();
      });
      global.appendLog('sandbox execute failed: $err');
    } finally {
      if (mounted) {
        setState(() {
          _sandboxActionBusy = false;
        });
      }
    }
  }

  Future<void> _decideSandboxApproval(
    SandboxApproval approval, {
    required bool approve,
  }) async {
    final client = _client(context);
    final global = context.read<GlobalCubit>();
    setState(() {
      _sandboxActionBusy = true;
      _sandboxError = null;
    });

    try {
      final result = approve
          ? await client.approveSandboxApproval(
              approvalId: approval.approvalId,
              actor: 'flutter-user',
            )
          : await client.rejectSandboxApproval(
              approvalId: approval.approvalId,
              actor: 'flutter-user',
            );

      if (!mounted) {
        return;
      }
      global.appendLog(
        'sandbox approval ${approve ? 'approved' : 'rejected'}: ${result.approvalId} status=${result.status}',
      );
      await _refreshSandboxApprovals(silent: true);
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _sandboxError = err.toString();
      });
      global.appendLog('sandbox approval decision failed: $err');
    } finally {
      if (mounted) {
        setState(() {
          _sandboxActionBusy = false;
        });
      }
    }
  }

  Future<void> _openSandboxActionDialog() async {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }
    final sessionId = ws.sessionId.trim();
    if (sessionId.isEmpty) {
      setState(() {
        _sandboxError = 'open a session before running sandbox actions';
      });
      return;
    }

    final operationController = TextEditingController(text: 'file_write');
    final pathController = TextEditingController(text: 'notes/demo.txt');
    final contentController = TextEditingController(text: 'hello from sandbox');
    final commandController = TextEditingController(text: 'ls');
    final argsController = TextEditingController();
    final timeoutController = TextEditingController(text: '120000');

    await showDialog<void>(
      context: context,
      builder: (dialogContext) {
        String selectedOp = operationController.text;

        bool needsPath(String op) {
          return op == 'file_write' ||
              op == 'file_delete' ||
              op == 'directory_create' ||
              op == 'directory_delete';
        }

        bool needsContent(String op) {
          return op == 'file_write' || op == 'apply_patch';
        }

        bool needsCommand(String op) {
          return op == 'command_run';
        }

        String contentLabel(String op) {
          if (op == 'apply_patch') {
            return 'Patch (apply_patch format)';
          }
          return 'Content';
        }

        return StatefulBuilder(
          builder: (context, setInnerState) {
            return ContentDialog(
              title: const Text('Run Sandbox Action'),
              content: SizedBox(
                width: 520,
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    InfoLabel(
                      label: 'Operation',
                      child: ComboBox<String>(
                        isExpanded: true,
                        value: selectedOp,
                        items: const [
                          ComboBoxItem(
                            value: 'file_write',
                            child: Text('file_write'),
                          ),
                          ComboBoxItem(
                            value: 'file_delete',
                            child: Text('file_delete'),
                          ),
                          ComboBoxItem(
                            value: 'directory_create',
                            child: Text('directory_create'),
                          ),
                          ComboBoxItem(
                            value: 'directory_delete',
                            child: Text('directory_delete'),
                          ),
                          ComboBoxItem(
                            value: 'command_run',
                            child: Text('command_run'),
                          ),
                          ComboBoxItem(
                            value: 'apply_patch',
                            child: Text('apply_patch'),
                          ),
                        ],
                        onChanged: (value) {
                          if (value == null) {
                            return;
                          }
                          setInnerState(() {
                            selectedOp = value;
                            operationController.text = value;
                          });
                        },
                      ),
                    ),
                    if (needsPath(selectedOp)) ...[
                      const SizedBox(height: 10),
                      InfoLabel(
                        label: 'Path (workspace-relative)',
                        child: TextBox(controller: pathController),
                      ),
                    ],
                    if (needsContent(selectedOp)) ...[
                      const SizedBox(height: 10),
                      InfoLabel(
                        label: contentLabel(selectedOp),
                        child: TextBox(
                          controller: contentController,
                          maxLines: 6,
                        ),
                      ),
                    ],
                    if (needsCommand(selectedOp)) ...[
                      const SizedBox(height: 10),
                      InfoLabel(
                        label: 'Command',
                        child: TextBox(controller: commandController),
                      ),
                      const SizedBox(height: 10),
                      InfoLabel(
                        label: 'Args (space separated)',
                        child: TextBox(controller: argsController),
                      ),
                      const SizedBox(height: 10),
                      InfoLabel(
                        label: 'Timeout ms',
                        child: TextBox(controller: timeoutController),
                      ),
                    ],
                  ],
                ),
              ),
              actions: [
                Button(
                  onPressed: () {
                    Navigator.of(dialogContext).pop();
                  },
                  child: const Text('Cancel'),
                ),
                FilledButton(
                  onPressed: () async {
                    final timeout = int.tryParse(timeoutController.text.trim());
                    final op = operationController.text.trim();
                    final path = pathController.text.trim();
                    final content = contentController.text;
                    final command = commandController.text.trim();
                    final args = _parseSandboxArgs(argsController.text);

                    Navigator.of(dialogContext).pop();
                    await _executeSandboxTool(
                      operation: op,
                      path: path,
                      content: content,
                      command: command,
                      args: args,
                      timeoutMs: timeout,
                    );
                  },
                  child: const Text('Run'),
                ),
              ],
            );
          },
        );
      },
    );

    operationController.dispose();
    pathController.dispose();
    contentController.dispose();
    commandController.dispose();
    argsController.dispose();
    timeoutController.dispose();
  }

  Future<void> _createSkill({
    required String name,
    String? description,
    String? path,
  }) async {
    final skillName = name.trim();
    if (skillName.isEmpty) {
      setState(() {
        _skillsError = 'skill name must not be empty';
      });
      return;
    }

    final descriptionValue = description?.trim();
    final pathValue = path?.trim();
    final client = _client(context);
    final global = context.read<GlobalCubit>();

    try {
      final result = await client.createSkill(
        name: skillName,
        description: descriptionValue?.isEmpty == true
            ? null
            : descriptionValue,
        path: pathValue?.isEmpty == true ? null : pathValue,
      );
      global.appendLog(
        'skill created: ${result.skillId} "$skillName" status=${result.status}',
      );
      await _refreshSkills(silent: true);
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _skillsError = err.toString();
      });
      global.appendLog('skill create failed: $err');
    }
  }

  Future<void> _setSkillActivation(KernelSkill skill, bool active) async {
    final client = _client(context);
    final global = context.read<GlobalCubit>();

    try {
      final result = active
          ? await client.activateSkill(skill.skillId)
          : await client.deactivateSkill(skill.skillId);
      global.appendLog(
        'skill ${active ? 'activated' : 'deactivated'}: ${result.skillId} status=${result.status}',
      );
      await _refreshSkills(silent: true);
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _skillsError = err.toString();
      });
      global.appendLog('skill activation update failed: $err');
    }
  }

  Future<void> _installSkillStoreItem(Map<String, dynamic> item) async {
    final itemId = item['item_id']?.toString().trim() ?? '';
    if (itemId.isEmpty) {
      setState(() {
        _skillsError = 'skill store item is missing item_id';
      });
      return;
    }

    final client = _client(context);
    final global = context.read<GlobalCubit>();
    try {
      final result = await client.installSkillStoreItem(itemId);
      global.appendLog(
        'skill store install: item=$itemId skill=${result.skillId} status=${result.status}',
      );
      await _refreshSkills(silent: true);
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _skillsError = err.toString();
      });
      global.appendLog('skill store install failed: $err');
    }
  }

  Future<void> _openSkillCreator() async {
    final nameController = TextEditingController();
    final descriptionController = TextEditingController();
    final pathController = TextEditingController();

    await showDialog<void>(
      context: context,
      builder: (dialogContext) {
        return ContentDialog(
          title: const Text('Create Skill'),
          content: SizedBox(
            width: 460,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoLabel(
                  label: 'Name',
                  child: TextBox(controller: nameController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Description (optional)',
                  child: TextBox(
                    controller: descriptionController,
                    maxLines: 3,
                  ),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'SKILL.md Path (optional)',
                  child: TextBox(
                    controller: pathController,
                    placeholder: 'skills/my-skill/SKILL.md',
                  ),
                ),
              ],
            ),
          ),
          actions: [
            Button(
              onPressed: () {
                Navigator.of(dialogContext).pop();
              },
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () async {
                final name = nameController.text;
                final description = descriptionController.text;
                final path = pathController.text;
                Navigator.of(dialogContext).pop();
                await _createSkill(
                  name: name,
                  description: description,
                  path: path,
                );
              },
              child: const Text('Create'),
            ),
          ],
        );
      },
    );

    nameController.dispose();
    descriptionController.dispose();
    pathController.dispose();
  }

  Future<void> _persistProjectDispatcher({
    required String projectId,
    required String kind,
    required String ref,
  }) async {
    final project = projectId.trim();
    if (project.isEmpty) {
      return;
    }

    try {
      await _client(context).setProjectDispatcher(
        projectId: project,
        targetKind: kind,
        targetRef: ref,
      );
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _runtimeError = err.toString();
      });
      context.read<GlobalCubit>().appendLog('set dispatcher failed: $err');
    }
  }

  Future<void> _ensureWorkspaceFolder({
    required String workspaceId,
    required String target,
  }) async {
    final path = target.trim();
    if (path.isEmpty) {
      return;
    }
    final global = context.read<GlobalCubit>();

    try {
      final response = await _client(context).ensureWorkspaceFolder(path);
      final created = response['created'] == true;
      final existed = response['existed'] == true;
      final status = created
          ? 'created'
          : (existed ? 'already-exists' : 'ensured');
      global.appendLog(
        'workspace-folder: ws=$workspaceId path=$path status=$status',
      );
    } catch (err) {
      global.appendLog(
        'workspace-folder failed: ws=$workspaceId path=$path err=$err',
      );
    }
  }

  Future<void> _setActiveDispatcher(String value) async {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final parts = value.split('::');
    if (parts.length != 2) {
      return;
    }

    final kind = parts[0];
    final ref = parts[1];

    _updateWorkspaceById(
      ws.id,
      (item) => item.copyWith(dispatcherKind: kind, dispatcherRef: ref),
    );

    await _persistProjectDispatcher(
      projectId: ws.projectId,
      kind: kind,
      ref: ref,
    );

    if (!mounted) {
      return;
    }
    context.read<GlobalCubit>().appendLog('dispatcher set: $kind/$ref');
  }

  Future<void> _saveRuntimeProvider(RuntimeProviderConfig draft) async {
    final global = context.read<GlobalCubit>();
    final client = _client(context);
    final activeWorkspaceId = _activeWorkspaceId;

    try {
      if (draft.id == null) {
        final providerId = await client.addRuntimeProvider(draft);
        global.appendLog('provider created: ${draft.name} (#$providerId)');
      } else {
        await client.updateRuntimeProvider(draft);
        global.appendLog('provider updated: ${draft.name} (#${draft.id})');
      }

      await _loadRuntimeConfiguration(
        workspaceId: activeWorkspaceId,
        silent: true,
      );
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _runtimeError = err.toString();
      });
      global.appendLog('provider save failed: $err');
    }
  }

  Future<void> _removeRuntimeProvider(RuntimeProviderConfig provider) async {
    final id = provider.id;
    if (id == null) {
      return;
    }

    final global = context.read<GlobalCubit>();
    final client = _client(context);
    final activeWorkspaceId = _activeWorkspaceId;

    try {
      await client.deleteRuntimeProvider(id);
      global.appendLog('provider deleted: ${provider.name} (#$id)');
      await _loadRuntimeConfiguration(
        workspaceId: activeWorkspaceId,
        silent: true,
      );
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _runtimeError = err.toString();
      });
      global.appendLog('provider delete failed: $err');
    }
  }

  Future<void> _openProviderEditor({RuntimeProviderConfig? provider}) async {
    final nameController = TextEditingController(text: provider?.name ?? '');
    final kindController = TextEditingController(
      text: (provider?.kind ?? '').isEmpty ? 'openai' : provider!.kind,
    );
    final modelController = TextEditingController(text: provider?.model ?? '');
    final baseUrlController = TextEditingController(
      text: provider?.baseUrl ?? '',
    );
    final apiKeyController = TextEditingController(
      text: provider?.apiKey ?? '',
    );

    await showDialog<void>(
      context: context,
      builder: (dialogContext) {
        return ContentDialog(
          title: Text(provider == null ? 'Add Provider' : 'Edit Provider'),
          content: SizedBox(
            width: 460,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoLabel(
                  label: 'Display Name',
                  child: TextBox(controller: nameController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Kind',
                  child: TextBox(controller: kindController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Model',
                  child: TextBox(controller: modelController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Base URL (optional)',
                  child: TextBox(controller: baseUrlController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'API Key (optional)',
                  child: TextBox(controller: apiKeyController),
                ),
              ],
            ),
          ),
          actions: [
            Button(
              onPressed: () {
                Navigator.of(dialogContext).pop();
              },
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () async {
                final draft = RuntimeProviderConfig(
                  id: provider?.id,
                  name: nameController.text.trim(),
                  kind: kindController.text.trim(),
                  model: modelController.text.trim(),
                  baseUrl: baseUrlController.text.trim(),
                  apiKey: apiKeyController.text,
                );
                Navigator.of(dialogContext).pop();
                await _saveRuntimeProvider(draft);
              },
              child: const Text('Save'),
            ),
          ],
        );
      },
    );

    nameController.dispose();
    kindController.dispose();
    modelController.dispose();
    baseUrlController.dispose();
    apiKeyController.dispose();
  }

  Future<void> _confirmDeleteProvider(RuntimeProviderConfig provider) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) {
        return ContentDialog(
          title: const Text('Delete Provider'),
          content: Text('Delete provider "${provider.name}"?'),
          actions: [
            Button(
              onPressed: () {
                Navigator.of(dialogContext).pop(false);
              },
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () {
                Navigator.of(dialogContext).pop(true);
              },
              child: const Text('Delete'),
            ),
          ],
        );
      },
    );

    if (confirmed == true) {
      await _removeRuntimeProvider(provider);
    }
  }

  Future<void> _saveRuntimeSshTarget(SshTargetConfig draft) async {
    final global = context.read<GlobalCubit>();
    final client = _client(context);
    final activeWorkspaceId = _activeWorkspaceId;

    try {
      if (draft.isPersisted) {
        await client.updateRuntimeSshTarget(draft);
        global.appendLog(
          'ssh target updated: ${draft.name} (${draft.sshTargetId})',
        );
      } else {
        final sshTargetId = await client.addRuntimeSshTarget(draft);
        global.appendLog('ssh target created: ${draft.name} ($sshTargetId)');
      }

      await _loadRuntimeConfiguration(
        workspaceId: activeWorkspaceId,
        silent: true,
      );
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _runtimeError = err.toString();
      });
      global.appendLog('ssh target save failed: $err');
    }
  }

  Future<void> _removeRuntimeSshTarget(SshTargetConfig target) async {
    final sshTargetId = target.sshTargetId.trim();
    if (sshTargetId.isEmpty) {
      return;
    }

    final global = context.read<GlobalCubit>();
    final client = _client(context);
    final activeWorkspaceId = _activeWorkspaceId;

    try {
      await client.deleteRuntimeSshTarget(sshTargetId);
      global.appendLog('ssh target deleted: ${target.name} ($sshTargetId)');
      await _loadRuntimeConfiguration(
        workspaceId: activeWorkspaceId,
        silent: true,
      );
    } catch (err) {
      if (!mounted) {
        return;
      }
      setState(() {
        _runtimeError = err.toString();
      });
      global.appendLog('ssh target delete failed: $err');
    }
  }

  Future<void> _openSshTargetEditor({SshTargetConfig? target}) async {
    final nameController = TextEditingController(text: target?.name ?? '');
    final hostController = TextEditingController(text: target?.host ?? '');
    final portController = TextEditingController(
      text: target?.port?.toString() ?? '',
    );
    final usernameController = TextEditingController(
      text: target?.username ?? '',
    );
    final identityFileController = TextEditingController(
      text: target?.identityFile ?? '',
    );
    final remoteWorkdirController = TextEditingController(
      text: target?.remoteWorkdir ?? '',
    );
    final optionsController = TextEditingController(
      text: target?.options.join('\n') ?? '',
    );

    await showDialog<void>(
      context: context,
      builder: (dialogContext) {
        return ContentDialog(
          title: Text(target == null ? 'Add SSH Target' : 'Edit SSH Target'),
          content: SizedBox(
            width: 520,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoLabel(
                  label: 'Name',
                  child: TextBox(controller: nameController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Host',
                  child: TextBox(controller: hostController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Port (optional)',
                  child: TextBox(controller: portController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Username (optional)',
                  child: TextBox(controller: usernameController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Identity File (optional)',
                  child: TextBox(controller: identityFileController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Remote Workdir (optional)',
                  child: TextBox(controller: remoteWorkdirController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'SSH Options (one token per line, optional)',
                  child: TextBox(controller: optionsController, maxLines: 4),
                ),
              ],
            ),
          ),
          actions: [
            Button(
              onPressed: () {
                Navigator.of(dialogContext).pop();
              },
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () async {
                final portText = portController.text.trim();
                final port = portText.isEmpty ? null : int.tryParse(portText);
                final options = optionsController.text
                    .split('\n')
                    .map((line) => line.trim())
                    .where((line) => line.isNotEmpty)
                    .toList();

                final draft = SshTargetConfig(
                  sshTargetId: target?.sshTargetId ?? '',
                  name: nameController.text.trim(),
                  host: hostController.text.trim(),
                  port: port,
                  username: usernameController.text.trim(),
                  identityFile: identityFileController.text.trim(),
                  remoteWorkdir: remoteWorkdirController.text.trim(),
                  options: options,
                );
                Navigator.of(dialogContext).pop();
                await _saveRuntimeSshTarget(draft);
              },
              child: const Text('Save'),
            ),
          ],
        );
      },
    );

    nameController.dispose();
    hostController.dispose();
    portController.dispose();
    usernameController.dispose();
    identityFileController.dispose();
    remoteWorkdirController.dispose();
    optionsController.dispose();
  }

  Future<void> _confirmDeleteSshTarget(SshTargetConfig target) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialogContext) {
        return ContentDialog(
          title: const Text('Delete SSH Target'),
          content: Text('Delete SSH target "${target.name}"?'),
          actions: [
            Button(
              onPressed: () {
                Navigator.of(dialogContext).pop(false);
              },
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () {
                Navigator.of(dialogContext).pop(true);
              },
              child: const Text('Delete'),
            ),
          ],
        );
      },
    );
    if (confirmed == true) {
      await _removeRuntimeSshTarget(target);
    }
  }

  void _syncPolling(bool enabled) {
    _pollTimer?.cancel();
    if (!enabled || _kernelEventsConnected) {
      return;
    }

    _pollTimer = Timer.periodic(_pollInterval, (_) {
      unawaited(_pollKernelState());
    });
  }

  void _syncEventSubscription({bool force = false}) {
    final ws = _activeWorkspace;
    if (ws == null) {
      _cancelEventSubscription();
      return;
    }

    final sessionId = ws.sessionId.trim();
    if (sessionId.isEmpty) {
      _cancelEventSubscription();
      return;
    }

    final alreadySubscribed =
        !force &&
        _kernelEventSubscription != null &&
        _kernelEventWorkspaceId == ws.id &&
        _kernelEventSessionId == sessionId;
    if (alreadySubscribed) {
      return;
    }

    _cancelEventSubscription();
    _kernelEventWorkspaceId = ws.id;
    _kernelEventSessionId = sessionId;

    final client = _client(context);
    final globalCubit = context.read<GlobalCubit>();
    globalCubit.appendLog(
      'kernel-events subscribe: ws=${ws.id} session=$sessionId',
    );
    _kernelEventSubscription = client
        .watchEvents(sessionId: sessionId)
        .listen(
          (event) {
            _kernelEventsConnected = true;
            _syncPolling(_connectionAutoRefresh);
            _eventRetryTimer?.cancel();
            _onKernelRealtimeEvent(workspaceId: ws.id, event: event);
          },
          onError: (Object err, StackTrace stackTrace) {
            _kernelEventsConnected = false;
            _syncPolling(_connectionAutoRefresh);
            globalCubit.appendLog(
              'kernel-events error: ws=${ws.id} session=$sessionId err=$err',
            );
            _scheduleEventSubscriptionRetry();
          },
          onDone: () {
            _kernelEventsConnected = false;
            _syncPolling(_connectionAutoRefresh);
            globalCubit.appendLog(
              'kernel-events done: ws=${ws.id} session=$sessionId',
            );
            _scheduleEventSubscriptionRetry();
          },
        );
  }

  void _scheduleEventSubscriptionRetry() {
    _eventRetryTimer?.cancel();
    _eventRetryTimer = Timer(const Duration(seconds: 2), () {
      if (!mounted) {
        return;
      }
      _syncEventSubscription(force: true);
    });
  }

  void _cancelEventSubscription() {
    _eventRetryTimer?.cancel();
    _kernelEventSubscription?.cancel();
    _kernelEventSubscription = null;
    _kernelEventWorkspaceId = null;
    _kernelEventSessionId = null;
    _kernelEventsConnected = false;
    _syncPolling(_connectionAutoRefresh);
  }

  void _onKernelRealtimeEvent({
    required String workspaceId,
    required Map<String, dynamic> event,
  }) {
    final eventType = event['event_type']?.toString() ?? '';
    if (eventType == 'kernel.ws.connected') {
      return;
    }

    context.read<NotificationCubit>().ingestTaskEvents([event]);

    if (eventType.startsWith('tool.')) {
      unawaited(_refreshSandboxApprovals(silent: true));
    }

    final taskId = event['task_id']?.toString().trim() ?? '';
    if (taskId.isEmpty) {
      return;
    }

    final ws = _workspaceById(workspaceId);
    if (ws == null) {
      return;
    }
    _KanbanSticky? sticky;
    for (final item in ws.stickies) {
      if (item.taskId.trim() == taskId) {
        sticky = item;
        break;
      }
    }
    if (sticky == null) {
      return;
    }

    _applyRealtimeTaskEventToSticky(workspaceId, sticky.id, event);
  }

  void _applyRealtimeTaskEventToSticky(
    String workspaceId,
    String stickyId,
    Map<String, dynamic> event,
  ) {
    final sticky = _stickyById(workspaceId, stickyId);
    if (sticky == null) {
      return;
    }

    final eventId = event['event_id']?.toString() ?? '';
    final seen = Set<String>.from(sticky.seenEventIds);
    if (eventId.isNotEmpty && seen.contains(eventId)) {
      return;
    }
    if (eventId.isNotEmpty) {
      seen.add(eventId);
    }

    final eventType = event['event_type']?.toString() ?? '';
    final payload = event['payload'];
    final summary = _extractEventSummary(eventType, payload);

    _StickyStatus nextStatus = sticky.status;
    String nextEngineStatus = sticky.engineStatus;

    if (eventType == 'task.done') {
      nextStatus = _StickyStatus.done;
      nextEngineStatus = 'done';
    } else if (eventType == 'task.error') {
      nextStatus = _StickyStatus.failed;
      nextEngineStatus = 'error';
    } else if (eventType == 'task.aborted') {
      nextStatus = _StickyStatus.cancelled;
      nextEngineStatus = 'cancelled';
    } else if (eventType.startsWith('task.')) {
      nextStatus = _StickyStatus.running;
      nextEngineStatus = eventType;
    }

    _updateSticky(
      workspaceId,
      stickyId,
      (item) => item.copyWith(
        busy: false,
        status: nextStatus,
        engineStatus: nextEngineStatus,
        note: summary,
        updatedAt: DateTime.now(),
        seenEventIds: seen,
      ),
    );

    if (_isAiOutputEvent(eventType)) {
      final aiText = _extractAiTextFromPayload(payload);
      if (aiText != null && aiText.trim().isNotEmpty) {
        _appendChatMessage(
          workspaceId: workspaceId,
          tabId: sticky.chatTabId,
          fromUser: false,
          text: aiText,
        );
      }
    }
  }

  String _extractEventSummary(String eventType, dynamic payload) {
    if (payload is Map<String, dynamic>) {
      final summary = payload['summary']?.toString().trim();
      if (summary != null && summary.isNotEmpty) {
        return summary;
      }
      final text = payload['text']?.toString().trim();
      if (text != null && text.isNotEmpty) {
        return text;
      }
      final reason = payload['reason']?.toString().trim();
      if (reason != null && reason.isNotEmpty) {
        return reason;
      }
    }
    return eventType;
  }

  Future<void> _pollKernelState() async {
    if (!mounted || _pollInFlight) {
      return;
    }

    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final workspaceId = ws.id;

    _pollInFlight = true;
    try {
      final running = ws.stickies
          .where(
            (item) =>
                item.status == _StickyStatus.running &&
                item.taskId.trim().isNotEmpty &&
                !item.busy,
          )
          .map((item) => item.id)
          .toList();

      for (final stickyId in running) {
        await _refreshSticky(workspaceId, stickyId, silent: true);
      }
      if (running.isNotEmpty) {
        await _refreshClientProcesses(silent: true);
      }
    } finally {
      _pollInFlight = false;
    }
  }

  void _saveWorkspaceSettings() {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final name = _workspaceNameController.text.trim();
    final projectId = _projectIdController.text.trim();
    final target = _targetController.text.trim();
    final nextName = name.isEmpty ? ws.name : name;
    final nextProjectId = projectId.isEmpty ? ws.projectId : projectId;
    final nextTarget = target.isEmpty
        ? _workspaceMappedFolderPath(ws.id)
        : target;

    _workspaceNameController.text = nextName;
    _projectIdController.text = nextProjectId;
    _targetController.text = nextTarget;

    _updateWorkspaceById(
      ws.id,
      (item) => item.copyWith(
        name: nextName,
        projectId: nextProjectId,
        target: nextTarget,
      ),
    );

    context.read<SettingCubit>().updateProject(
      projectId: nextProjectId,
      target: nextTarget,
    );

    final updated = _workspaceById(ws.id);
    if (updated != null) {
      unawaited(
        _persistProjectDispatcher(
          projectId: nextProjectId,
          kind: updated.dispatcherKind,
          ref: updated.dispatcherRef,
        ),
      );
      unawaited(
        _loadRuntimeConfiguration(workspaceId: updated.id, silent: true),
      );
      unawaited(_ensureWorkspaceFolder(workspaceId: ws.id, target: nextTarget));
    }

    context.read<GlobalCubit>().appendLog(
      'workspace saved: $nextName ($nextProjectId) folder=$nextTarget',
    );
  }

  void _saveConnectionSettings() {
    final parsedTimeout = int.tryParse(
      _noResponseTimeoutController.text.trim(),
    );
    final normalizedTimeout = (parsedTimeout ?? _noResponseTimeoutMinutes)
        .clamp(1, 120)
        .toInt();
    _noResponseTimeoutMinutes = normalizedTimeout;
    _noResponseTimeoutController.text = normalizedTimeout.toString();

    context.read<SettingCubit>().updateConnection(
      baseUrl: _baseUrlController.text,
      token: _tokenController.text,
    );
    context.read<SettingCubit>().setAutoRefresh(_connectionAutoRefresh);
    context.read<SettingCubit>().setNoResponseTimeoutMinutes(normalizedTimeout);
    _syncPolling(_connectionAutoRefresh);
    _syncEventSubscription(force: true);
    unawaited(_refreshSkills(silent: true));
    unawaited(_refreshSandboxApprovals(silent: true));
    context.read<GlobalCubit>().appendLog(
      'connection settings saved (no-response-timeout=${normalizedTimeout}m)',
    );
  }

  void _applySessionId() {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final sessionId = _sessionIdController.text.trim();
    context.read<SessionCubit>().setSessionId(sessionId);

    _updateWorkspaceById(
      ws.id,
      (item) => item.copyWith(
        sessionId: sessionId,
        sessionStatus: sessionId.isEmpty ? 'idle' : item.sessionStatus,
      ),
    );

    context.read<GlobalCubit>().appendLog('session id updated: $sessionId');
    _syncEventSubscription(force: true);
    unawaited(_refreshSandboxApprovals(silent: true));
  }

  Future<String?> _openSessionForWorkspace(
    String workspaceId, {
    bool syncSessionCubit = false,
  }) async {
    final ws = _workspaceById(workspaceId);
    if (ws == null) {
      return null;
    }

    final client = _client(context);
    final globalCubit = context.read<GlobalCubit>();
    final sessionCubit = context.read<SessionCubit>();

    try {
      final response = await client.openSession(
        projectId: ws.projectId,
        target: ws.target,
        dispatcherKind: ws.dispatcherKind,
        dispatcherRef: ws.dispatcherRef,
      );

      final sessionId = response['session_id']?.toString() ?? '';
      final status = response['status']?.toString() ?? 'running';

      _updateWorkspaceById(
        workspaceId,
        (item) => item.copyWith(sessionId: sessionId, sessionStatus: status),
      );

      if (workspaceId == _activeWorkspaceId) {
        _sessionIdController.text = sessionId;
      }

      if (syncSessionCubit && workspaceId == _activeWorkspaceId) {
        sessionCubit.setSessionId(sessionId);
      }
      if (workspaceId == _activeWorkspaceId) {
        _syncEventSubscription(force: true);
        unawaited(_refreshSandboxApprovals(silent: true));
      }

      globalCubit.appendLog(
        'open-session: ws=$workspaceId session=$sessionId status=$status dispatcher=${ws.dispatcherKind}/${ws.dispatcherRef}',
      );
      return sessionId;
    } catch (err) {
      _updateWorkspaceById(
        workspaceId,
        (item) => item.copyWith(sessionStatus: 'error'),
      );
      globalCubit.appendLog('open-session failed: ws=$workspaceId err=$err');
      return null;
    }
  }

  Future<void> _openSession() async {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    _saveWorkspaceSettings();
    await _openSessionForWorkspace(ws.id, syncSessionCubit: true);
  }

  Future<String?> _ensureSession(String workspaceId) async {
    final ws = _workspaceById(workspaceId);
    if (ws == null) {
      return null;
    }

    final existing = ws.sessionId.trim();
    if (existing.isNotEmpty) {
      return existing;
    }

    return _openSessionForWorkspace(
      workspaceId,
      syncSessionCubit: workspaceId == _activeWorkspaceId,
    );
  }

  _TaskDraft _draftTaskFromPrompt(String prompt) {
    final normalized = prompt.trim().replaceAll(RegExp(r'\s+'), ' ');
    final category = _classifyTaskCategory(normalized);

    String title = normalized;
    final splitBySentence = RegExp(r'[。.!?\n]').firstMatch(normalized);
    if (splitBySentence != null && splitBySentence.start > 0) {
      title = normalized.substring(0, splitBySentence.start).trim();
    }
    if (title.isEmpty) {
      title = normalized;
    }
    if (title.length > 42) {
      title = '${title.substring(0, 39)}...';
    }

    return _TaskDraft(
      title: title.isEmpty ? 'Untitled request' : title,
      category: category,
    );
  }

  String _classifyTaskCategory(String text) {
    final lower = text.toLowerCase();

    if (lower.contains('bug') ||
        lower.contains('error') ||
        lower.contains('修复')) {
      return 'Bugfix';
    }
    if (lower.contains('ui') ||
        lower.contains('页面') ||
        lower.contains('布局') ||
        lower.contains('sidebar')) {
      return 'UI';
    }
    if (lower.contains('test') || lower.contains('测试')) {
      return 'Test';
    }
    if (lower.contains('doc') || lower.contains('文档')) {
      return 'Doc';
    }
    if (lower.contains('deploy') || lower.contains('发布')) {
      return 'Release';
    }
    return 'Feature';
  }

  String _suggestTaskId(String category) {
    final stamp = DateTime.now().millisecondsSinceEpoch.toString();
    final prefix = category.toLowerCase().replaceAll(RegExp(r'[^a-z]'), '');
    return '${prefix.isEmpty ? 'task' : prefix}-${stamp.substring(stamp.length - 6)}';
  }

  void _appendChatMessage({
    required String workspaceId,
    required String tabId,
    required bool fromUser,
    required String text,
  }) {
    final ws = _workspaceById(workspaceId);
    if (ws == null) {
      return;
    }

    final nextMessages = _cloneChatMessages(ws.chatMessages);
    final messages =
        List<_ChatMessage>.from(nextMessages[tabId] ?? const <_ChatMessage>[])
          ..add(
            _ChatMessage(
              id: _nextMessageId(),
              fromUser: fromUser,
              text: text,
              createdAt: DateTime.now(),
            ),
          );
    nextMessages[tabId] = messages;

    _updateWorkspaceById(
      workspaceId,
      (item) => item.copyWith(chatMessages: nextMessages),
    );
  }

  _KanbanSticky? _stickyById(String workspaceId, String stickyId) {
    final ws = _workspaceById(workspaceId);
    if (ws == null) {
      return null;
    }

    for (final sticky in ws.stickies) {
      if (sticky.id == stickyId) {
        return sticky;
      }
    }
    return null;
  }

  void _updateSticky(
    String workspaceId,
    String stickyId,
    _KanbanSticky Function(_KanbanSticky sticky) mapper,
  ) {
    final ws = _workspaceById(workspaceId);
    if (ws == null) {
      return;
    }

    _updateWorkspaceById(
      workspaceId,
      (item) => item.copyWith(
        stickies: item.stickies.map((sticky) {
          if (sticky.id != stickyId) {
            return sticky;
          }
          return mapper(sticky);
        }).toList(),
      ),
    );
  }

  Future<void> _sendPromptFromChat(String tabId) async {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final workspaceId = ws.id;
    final prompt = _chatInputController.text.trim();
    if (prompt.isEmpty) {
      return;
    }

    _chatInputController.clear();

    _appendChatMessage(
      workspaceId: workspaceId,
      tabId: tabId,
      fromUser: true,
      text: prompt,
    );

    final draft = _draftTaskFromPrompt(prompt);
    final sticky = _KanbanSticky(
      id: _nextStickyId(),
      chatTabId: tabId,
      suggestedTaskId: _suggestTaskId(draft.category),
      title: draft.title,
      prompt: prompt,
      category: draft.category,
      status: _StickyStatus.running,
      createdAt: DateTime.now(),
      updatedAt: DateTime.now(),
      note: 'Created from chat and started automatically.',
      busy: true,
    );

    _updateWorkspaceById(
      workspaceId,
      (item) =>
          item.copyWith(stickies: <_KanbanSticky>[sticky, ...item.stickies]),
    );
    context.read<GlobalCubit>().appendLog(
      'chat-submit: ws=$workspaceId tab=$tabId sticky=${sticky.id} title=${sticky.title}',
    );

    unawaited(_executeSticky(workspaceId: workspaceId, stickyId: sticky.id));
  }

  Future<void> _executeSticky({
    required String workspaceId,
    required String stickyId,
  }) async {
    final sticky = _stickyById(workspaceId, stickyId);
    if (sticky == null) {
      return;
    }

    final client = _client(context);
    final globalCubit = context.read<GlobalCubit>();
    globalCubit.appendLog(
      'sticky-execute: ws=$workspaceId sticky=$stickyId run=${sticky.runCount + 1}',
    );

    _updateSticky(
      workspaceId,
      stickyId,
      (item) => item.copyWith(
        busy: true,
        clearError: true,
        status: _StickyStatus.running,
        note: 'Submitting to kernel...',
        updatedAt: DateTime.now(),
        runCount: item.runCount + 1,
      ),
    );

    final sessionId = await _ensureSession(workspaceId);
    if (sessionId == null) {
      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          status: _StickyStatus.failed,
          error: 'No active session.',
          note: 'Session creation failed.',
          updatedAt: DateTime.now(),
        ),
      );
      globalCubit.appendLog(
        'sticky-execute failed: ws=$workspaceId sticky=$stickyId reason=no-session',
      );
      return;
    }

    try {
      final workspace = _workspaceById(workspaceId);
      final dispatcherValue = workspace == null
          ? null
          : _activeChatDispatcherValue(workspace);
      final dispatcherParts = dispatcherValue?.split('::');

      var chatDispatcherKind = workspace?.dispatcherKind;
      var chatDispatcherRef = workspace?.dispatcherRef;
      if (dispatcherParts != null && dispatcherParts.length == 2) {
        chatDispatcherKind = dispatcherParts[0];
        chatDispatcherRef = dispatcherParts[1];
      }
      final response = await client.submitTask(
        sessionId: sessionId,
        title: sticky.title,
        input: sticky.prompt,
        dispatcherKind: chatDispatcherKind,
        dispatcherRef: chatDispatcherRef,
      );

      final taskId = response['task_id']?.toString() ?? '';
      if (taskId.trim().isEmpty) {
        throw Exception('kernel returned empty task id');
      }

      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          taskId: taskId,
          engineStatus: 'submitted',
          busy: false,
          clearError: true,
          status: _StickyStatus.running,
          note: 'Task submitted: $taskId',
          updatedAt: DateTime.now(),
        ),
      );
      globalCubit.appendLog(
        'sticky-submitted: ws=$workspaceId sticky=$stickyId task=$taskId',
      );

      await _refreshSticky(workspaceId, stickyId, silent: true);
      unawaited(_watchStickyUntilTerminal(workspaceId, stickyId));
    } catch (err) {
      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          status: _StickyStatus.failed,
          error: err.toString(),
          note: 'Submit failed.',
          updatedAt: DateTime.now(),
        ),
      );
      globalCubit.appendLog('submit-sticky failed: $err');
    }
  }

  Future<void> _watchStickyUntilTerminal(
    String workspaceId,
    String stickyId,
  ) async {
    final watchKey = '$workspaceId:$stickyId';
    if (_stickyWatchers.contains(watchKey)) {
      return;
    }
    _stickyWatchers.add(watchKey);
    final globalCubit = context.read<GlobalCubit>();
    globalCubit.appendLog(
      'sticky-watch start: ws=$workspaceId sticky=$stickyId',
    );

    try {
      final timeoutMinutes = _noResponseTimeoutMinutes.clamp(1, 120).toInt();
      final noResponseTimeout = Duration(minutes: timeoutMinutes);
      final maxAttempts =
          (noResponseTimeout.inSeconds / _stickyWatchInterval.inSeconds)
              .ceil() +
          _stickyWatchAttemptsPadding;
      var lastProgressAt = DateTime.now();
      var lastSeenEventCount = 0;
      var lastEngineStatus = '';

      for (var attempt = 1; attempt <= maxAttempts; attempt++) {
        if (!mounted) {
          return;
        }

        final sticky = _stickyById(workspaceId, stickyId);
        if (sticky == null) {
          return;
        }

        final status = sticky.status;
        if (status == _StickyStatus.done ||
            status == _StickyStatus.failed ||
            status == _StickyStatus.cancelled) {
          globalCubit.appendLog(
            'sticky-watch done: ws=$workspaceId sticky=$stickyId status=${status.name} attempts=$attempt',
          );
          return;
        }

        if (!_kernelEventsConnected) {
          await _refreshSticky(workspaceId, stickyId, silent: true);
          if (attempt == 1 || attempt % 2 == 0) {
            await _refreshClientProcesses(silent: true);
          }
        }
        final refreshedSticky = _stickyById(workspaceId, stickyId);
        if (refreshedSticky == null) {
          return;
        }

        final hasProgress =
            refreshedSticky.seenEventIds.length > lastSeenEventCount ||
            refreshedSticky.engineStatus != lastEngineStatus;
        if (hasProgress) {
          lastProgressAt = DateTime.now();
          lastSeenEventCount = refreshedSticky.seenEventIds.length;
          lastEngineStatus = refreshedSticky.engineStatus;
        }

        final inactiveFor = DateTime.now().difference(lastProgressAt);
        if (inactiveFor >= noResponseTimeout) {
          _updateSticky(
            workspaceId,
            stickyId,
            (item) => item.copyWith(
              busy: false,
              status: _StickyStatus.failed,
              error: 'No response for $timeoutMinutes m.',
              note:
                  'No task response for $timeoutMinutes minutes. Marked as failed.',
              updatedAt: DateTime.now(),
            ),
          );
          globalCubit.appendLog(
            'sticky-watch timeout-failed: ws=$workspaceId sticky=$stickyId inactivity=${inactiveFor.inSeconds}s threshold=${noResponseTimeout.inSeconds}s',
          );
          return;
        }

        await Future<void>.delayed(_stickyWatchInterval);
      }

      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          status: _StickyStatus.failed,
          error: 'Watch timeout after extended polling.',
          note: 'Task watch timed out and was marked as failed.',
          updatedAt: DateTime.now(),
        ),
      );
      globalCubit.appendLog(
        'sticky-watch timeout-failed: ws=$workspaceId sticky=$stickyId attempts=$maxAttempts',
      );
    } finally {
      _stickyWatchers.remove(watchKey);
    }
  }

  _StickyStatus _mapKernelStatus(String? rawStatus) {
    final status = (rawStatus ?? '').toLowerCase();

    if (status.contains('done') ||
        status.contains('success') ||
        status.contains('complete')) {
      return _StickyStatus.done;
    }

    if (status.contains('cancel') ||
        status.contains('abort') ||
        status.contains('stopped')) {
      return _StickyStatus.cancelled;
    }

    if (status.contains('error') ||
        status.contains('fail') ||
        status.contains('panic')) {
      return _StickyStatus.failed;
    }

    return _StickyStatus.running;
  }

  String _extractTaskSummary(
    Map<String, dynamic> status,
    List<Map<String, dynamic>> events,
  ) {
    final statusSummary = status['summary']?.toString();
    if (statusSummary != null && statusSummary.trim().isNotEmpty) {
      return statusSummary;
    }

    if (events.isNotEmpty) {
      final latest = events.last;
      final payload = latest['payload'];
      if (payload is Map<String, dynamic>) {
        final summary = payload['summary']?.toString();
        if (summary != null && summary.trim().isNotEmpty) {
          return summary;
        }
      }
      final eventType = latest['event_type']?.toString() ?? 'event';
      return 'Latest event: $eventType';
    }

    return 'No detail yet';
  }

  String? _extractAiTextFromPayload(dynamic payload) {
    if (payload is String) {
      final streamed = _extractAiTextFromEventStream(payload);
      if (streamed != null) {
        return streamed;
      }
      final text = payload.trim();
      return text.isEmpty ? null : text;
    }

    if (payload is! Map<String, dynamic>) {
      return null;
    }

    final pipelineResult = payload['pipeline_result'];
    if (pipelineResult is Map<String, dynamic>) {
      final fromPipeline = _extractAiTextFromPipelineResult(pipelineResult);
      if (fromPipeline != null) {
        return fromPipeline;
      }
    }

    const keys = <String>[
      'answer',
      'response',
      'output',
      'content',
      'text',
      'message',
      'reason',
      'error',
      'stderr',
      'final_output',
    ];

    for (final key in keys) {
      final value = payload[key];
      if (value is! String || value.trim().isEmpty) {
        continue;
      }
      final trimmed = value.trim();
      final streamed = _extractAiTextFromEventStream(trimmed);
      if (streamed != null) {
        return streamed;
      }
      if (_looksLikeStructuredEventJson(trimmed)) {
        continue;
      }
      return trimmed;
    }

    const fallbackKeys = <String>['summary'];

    for (final key in fallbackKeys) {
      final value = payload[key];
      if (value is String && value.trim().isNotEmpty) {
        return value.trim();
      }
    }

    return null;
  }

  String? _extractAiTextFromPipelineResult(
    Map<String, dynamic> pipelineResult,
  ) {
    String? fallback;
    final stages = pipelineResult['stages'];
    if (stages is List) {
      for (final stage in stages.reversed) {
        if (stage is! Map<String, dynamic>) {
          continue;
        }
        final output = stage['output'];
        if (output is String && output.trim().isNotEmpty) {
          final streamed = _extractAiTextFromEventStream(output);
          if (streamed != null) {
            return streamed;
          }
          final trimmed = output.trim();
          if (!_isLowSignalAiText(trimmed)) {
            return trimmed;
          }
          fallback ??= trimmed;
        }

        final stageError = stage['error'];
        if (stageError is String && stageError.trim().isNotEmpty) {
          final stageId = stage['stage_id']?.toString().trim();
          if (stageId != null && stageId.isNotEmpty) {
            fallback ??= '$stageId: ${stageError.trim()}';
          } else {
            fallback ??= stageError.trim();
          }
        }
      }
    }

    final state = pipelineResult['state'];
    if (state is Map<String, dynamic>) {
      final lastDispatch = state['last_dispatch'];
      if (lastDispatch is Map<String, dynamic>) {
        final output = lastDispatch['output'];
        if (output is String && output.trim().isNotEmpty) {
          final streamed = _extractAiTextFromEventStream(output);
          if (streamed != null) {
            return streamed;
          }
          final trimmed = output.trim();
          if (!_isLowSignalAiText(trimmed)) {
            return trimmed;
          }
          fallback ??= trimmed;
        }

        final stderr = lastDispatch['stderr'];
        if (stderr is String && stderr.trim().isNotEmpty) {
          final exitCode = lastDispatch['exit_code']?.toString().trim();
          if (exitCode != null && exitCode.isNotEmpty) {
            fallback ??= 'stderr(exit=$exitCode): ${stderr.trim()}';
          } else {
            fallback ??= 'stderr: ${stderr.trim()}';
          }
        }

        final dispatchError = lastDispatch['error'];
        if (dispatchError is String && dispatchError.trim().isNotEmpty) {
          fallback ??= dispatchError.trim();
        }
      }
    }

    final pipelineError = pipelineResult['error'];
    if (pipelineError is String && pipelineError.trim().isNotEmpty) {
      return pipelineError.trim();
    }

    final finalOutput = pipelineResult['final_output'];
    if (finalOutput is String && finalOutput.trim().isNotEmpty) {
      final trimmed = finalOutput.trim();
      if (!_isLowSignalAiText(trimmed)) {
        return trimmed;
      }
      fallback ??= trimmed;
    }

    return fallback;
  }

  bool _isLowSignalAiText(String text) {
    final value = text.toLowerCase();
    return value.startsWith('stage ') ||
        value.contains('executed by') ||
        value == 'scheduler selected worker pool';
  }

  String? _extractAiTextFromEventStream(String raw) {
    String? latest;
    String? latestCommandOutput;
    for (final line in const LineSplitter().convert(raw)) {
      final trimmed = line.trim();
      if (trimmed.isEmpty || !trimmed.startsWith('{')) {
        continue;
      }

      try {
        final decoded = jsonDecode(trimmed);
        if (decoded is! Map<String, dynamic>) {
          continue;
        }
        final item = decoded['item'];
        if (item is! Map<String, dynamic>) {
          continue;
        }
        final itemType = item['type']?.toString();
        if (itemType == 'agent_message') {
          final text = item['text']?.toString().trim();
          if (text != null && text.isNotEmpty) {
            latest = text;
          }
          continue;
        }

        if (itemType == 'command_execution') {
          final text = item['aggregated_output']?.toString().trim();
          if (text != null && text.isNotEmpty) {
            latestCommandOutput = text;
          }
        }
      } catch (_) {
        // Ignore non-JSON lines or payload variants.
      }
    }
    return latest ?? latestCommandOutput;
  }

  bool _looksLikeStructuredEventJson(String text) {
    final trimmed = text.trim();
    if (!trimmed.startsWith('{') || !trimmed.endsWith('}')) {
      return false;
    }

    try {
      final decoded = jsonDecode(trimmed);
      if (decoded is! Map<String, dynamic>) {
        return false;
      }
      return decoded.containsKey('type') ||
          decoded.containsKey('item') ||
          decoded.containsKey('event_type');
    } catch (_) {
      return false;
    }
  }

  bool _isAiOutputEvent(String eventType) {
    final type = eventType.toLowerCase();
    return type.contains('assistant') ||
        type.contains('reply') ||
        type.contains('message') ||
        type.contains('output') ||
        type.contains('error') ||
        type.contains('done');
  }

  Future<void> _refreshSticky(
    String workspaceId,
    String stickyId, {
    bool silent = false,
  }) async {
    final sticky = _stickyById(workspaceId, stickyId);
    if (sticky == null) {
      return;
    }

    final taskId = sticky.taskId.trim();
    if (taskId.isEmpty) {
      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          status: _StickyStatus.failed,
          error: 'task id is empty',
          note: 'No task id attached.',
          updatedAt: DateTime.now(),
        ),
      );
      return;
    }

    final client = _client(context);
    final globalCubit = context.read<GlobalCubit>();
    final notificationCubit = context.read<NotificationCubit>();

    _updateSticky(
      workspaceId,
      stickyId,
      (item) => item.copyWith(
        busy: true,
        clearError: true,
        note: silent ? item.note : 'Refreshing task status...',
        updatedAt: DateTime.now(),
      ),
    );

    try {
      final status = await client.taskStatus(taskId);
      final events = await client.taskEvents(taskId);
      final latestSticky = _stickyById(workspaceId, stickyId) ?? sticky;
      final seenEventIds = Set<String>.from(latestSticky.seenEventIds);
      final aiReplies = <String>[];

      for (final event in events) {
        final eventId = event['event_id']?.toString() ?? '';
        if (eventId.isNotEmpty && seenEventIds.contains(eventId)) {
          continue;
        }
        if (eventId.isNotEmpty) {
          seenEventIds.add(eventId);
        }

        final eventType = event['event_type']?.toString() ?? '';
        if (!_isAiOutputEvent(eventType)) {
          continue;
        }

        final payload = event['payload'];
        final aiText = _extractAiTextFromPayload(payload);
        if (aiText == null) {
          continue;
        }
        aiReplies.add(aiText);
        globalCubit.appendLog(
          'sticky-ai: ws=$workspaceId sticky=$stickyId task=$taskId event=$eventType text=${_previewForLog(aiText)}',
        );
      }

      final mapped = _mapKernelStatus(status['status']?.toString());
      final summary = _extractTaskSummary(status, events);
      final currentKernelStatus =
          status['status']?.toString() ?? latestSticky.engineStatus;
      final hasProgress =
          currentKernelStatus != latestSticky.engineStatus ||
          seenEventIds.length != latestSticky.seenEventIds.length ||
          aiReplies.isNotEmpty;
      if (!silent || hasProgress) {
        globalCubit.appendLog(
          'sticky-refresh: ws=$workspaceId sticky=$stickyId task=$taskId status=${status['status']} events=${events.length} ai_replies=${aiReplies.length}',
        );
      }

      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          clearError: true,
          status: mapped,
          engineStatus: status['status']?.toString() ?? item.engineStatus,
          note: summary,
          updatedAt: DateTime.now(),
          seenEventIds: seenEventIds,
        ),
      );

      for (final text in aiReplies) {
        _appendChatMessage(
          workspaceId: workspaceId,
          tabId: sticky.chatTabId,
          fromUser: false,
          text: text,
        );
      }

      notificationCubit.ingestTaskEvents(events);
    } catch (err) {
      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          status: _StickyStatus.failed,
          error: err.toString(),
          note: 'Refresh failed.',
          updatedAt: DateTime.now(),
        ),
      );
      globalCubit.appendLog('refresh-sticky failed: $err');
    }
  }

  Future<void> _cancelSticky(String workspaceId, String stickyId) async {
    final sticky = _stickyById(workspaceId, stickyId);
    if (sticky == null) {
      return;
    }

    final taskId = sticky.taskId.trim();
    if (taskId.isEmpty) {
      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          status: _StickyStatus.cancelled,
          busy: false,
          note: 'Cancelled before task id assignment.',
          updatedAt: DateTime.now(),
        ),
      );
      return;
    }

    final client = _client(context);
    final globalCubit = context.read<GlobalCubit>();

    _updateSticky(
      workspaceId,
      stickyId,
      (item) => item.copyWith(
        busy: true,
        clearError: true,
        note: 'Cancelling...',
        updatedAt: DateTime.now(),
      ),
    );

    try {
      await client.abortTask(
        taskId: taskId,
        reason: 'aborted from kanban card',
      );
      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          status: _StickyStatus.cancelled,
          engineStatus: 'cancelled',
          note: 'Task cancelled by user.',
          updatedAt: DateTime.now(),
        ),
      );
    } catch (err) {
      _updateSticky(
        workspaceId,
        stickyId,
        (item) => item.copyWith(
          busy: false,
          status: _StickyStatus.failed,
          error: err.toString(),
          note: 'Cancel failed.',
          updatedAt: DateTime.now(),
        ),
      );
      globalCubit.appendLog('cancel-sticky failed: $err');
    }
  }

  Future<void> _rerunSticky(String workspaceId, String stickyId) async {
    final sticky = _stickyById(workspaceId, stickyId);
    if (sticky == null) {
      return;
    }

    _updateSticky(
      workspaceId,
      stickyId,
      (item) => item.copyWith(
        busy: false,
        clearError: true,
        status: _StickyStatus.running,
        taskId: '',
        engineStatus: 'rerun_requested',
        note: 'Re-run requested.',
        updatedAt: DateTime.now(),
      ),
    );

    await _executeSticky(workspaceId: workspaceId, stickyId: stickyId);
  }

  void _switchWorkspace(String workspaceId) {
    final ws = _workspaceById(workspaceId);
    if (ws == null) {
      return;
    }

    setState(() {
      _activeWorkspaceId = workspaceId;
      _workspaceNameController.text = ws.name;
      _projectIdController.text = ws.projectId;
      _targetController.text = ws.target;
      _sessionIdController.text = ws.sessionId;
      _chatInputController.clear();
    });

    context.read<SettingCubit>().updateProject(
      projectId: ws.projectId,
      target: ws.target,
    );
    context.read<SessionCubit>().setSessionId(ws.sessionId);
    _syncEventSubscription(force: true);

    unawaited(
      _ensureWorkspaceFolder(workspaceId: workspaceId, target: ws.target),
    );
    unawaited(_loadRuntimeConfiguration(workspaceId: workspaceId));
    unawaited(_refreshSkills(silent: true));
    unawaited(_refreshSandboxApprovals(silent: true));
  }

  void _addWorkspace() {
    final nextWorkspaceId = 'ws-$_workspaceSeed';
    final nameController = TextEditingController(
      text: 'Project $_workspaceSeed',
    );
    final projectController = TextEditingController(
      text: 'project-$_workspaceSeed',
    );
    final targetController = TextEditingController(
      text: _workspaceMappedFolderPath(nextWorkspaceId),
    );

    showDialog<void>(
      context: context,
      builder: (dialogContext) {
        return ContentDialog(
          title: const Text('Create Project Workspace'),
          content: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              InfoLabel(
                label: 'Name',
                child: TextBox(controller: nameController),
              ),
              const SizedBox(height: 10),
              InfoLabel(
                label: 'Project ID',
                child: TextBox(controller: projectController),
              ),
              const SizedBox(height: 10),
              InfoLabel(
                label: 'Workspace Folder',
                child: TextBox(controller: targetController, maxLines: 2),
              ),
            ],
          ),
          actions: [
            Button(
              onPressed: () {
                Navigator.of(dialogContext).pop();
              },
              child: const Text('Cancel'),
            ),
            FilledButton(
              onPressed: () {
                final id = 'ws-$_workspaceSeed';
                _workspaceSeed += 1;

                final workspace = _createWorkspace(
                  id: id,
                  name: nameController.text.trim().isEmpty
                      ? 'Project $id'
                      : nameController.text.trim(),
                  projectId: projectController.text.trim().isEmpty
                      ? 'project-$id'
                      : projectController.text.trim(),
                  target: targetController.text.trim().isEmpty
                      ? _workspaceMappedFolderPath(id)
                      : targetController.text.trim(),
                );

                setState(() {
                  _workspaces = <_WorkspaceState>[..._workspaces, workspace];
                });
                unawaited(
                  _ensureWorkspaceFolder(
                    workspaceId: id,
                    target: workspace.target,
                  ),
                );

                Navigator.of(dialogContext).pop();
                _switchWorkspace(id);
              },
              child: const Text('Create'),
            ),
          ],
        );
      },
    ).whenComplete(() {
      nameController.dispose();
      projectController.dispose();
      targetController.dispose();
    });
  }

  void _removeWorkspace(String workspaceId) {
    if (_workspaces.length <= 1) {
      context.read<GlobalCubit>().appendLog('cannot remove the last workspace');
      return;
    }

    final next = _workspaces.where((item) => item.id != workspaceId).toList();
    if (next.isEmpty) {
      return;
    }

    String nextActive = _activeWorkspaceId ?? next.first.id;
    if (nextActive == workspaceId) {
      nextActive = next.first.id;
    }

    setState(() {
      _workspaces = next;
      _activeWorkspaceId = nextActive;
    });

    _switchWorkspace(nextActive);
  }

  void _addChatTab() {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final id = 'chat-${DateTime.now().millisecondsSinceEpoch}';
    final title = 'Chat ${_chatSeed.toString()}';
    _chatSeed += 1;

    final nextTabs = <_WorkspaceTabItem>[
      ...ws.tabs,
      _WorkspaceTabItem(
        id: id,
        title: title,
        kind: _WorkspaceTabKind.chat,
        closable: true,
      ),
    ];

    final nextMessages = _cloneChatMessages(ws.chatMessages);
    nextMessages[id] = <_ChatMessage>[];

    _updateWorkspaceById(
      ws.id,
      (item) => item.copyWith(
        tabs: nextTabs,
        chatMessages: nextMessages,
        selectedTabIndex: nextTabs.length - 1,
      ),
    );
  }

  void _closeTab(String tabId) {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final index = ws.tabs.indexWhere((tab) => tab.id == tabId);
    if (index < 0) {
      return;
    }

    final tab = ws.tabs[index];
    if (!tab.closable) {
      return;
    }

    final nextTabs = List<_WorkspaceTabItem>.from(ws.tabs)..removeAt(index);
    var nextSelected = ws.selectedTabIndex;
    if (nextSelected >= nextTabs.length) {
      nextSelected = nextTabs.length - 1;
    }
    if (nextSelected < 0) {
      nextSelected = 0;
    }

    final nextMessages = _cloneChatMessages(ws.chatMessages)..remove(tabId);

    _updateWorkspaceById(
      ws.id,
      (item) => item.copyWith(
        tabs: nextTabs,
        chatMessages: nextMessages,
        selectedTabIndex: nextSelected,
      ),
    );
  }

  void _openTab(String tabId) {
    final ws = _activeWorkspace;
    if (ws == null) {
      return;
    }

    final index = ws.tabs.indexWhere((tab) => tab.id == tabId);
    if (index < 0) {
      return;
    }

    _updateWorkspaceById(
      ws.id,
      (item) => item.copyWith(selectedTabIndex: index),
    );

    final tab = ws.tabs[index];
    if (tab.kind == _WorkspaceTabKind.process) {
      unawaited(_refreshClientProcesses(silent: true));
    } else if (tab.kind == _WorkspaceTabKind.skills) {
      unawaited(_refreshSkills(silent: true));
      unawaited(_refreshSandboxApprovals(silent: true));
    }
  }

  String _statusLabel(_StickyStatus status) {
    switch (status) {
      case _StickyStatus.running:
        return 'Running';
      case _StickyStatus.done:
        return 'Done';
      case _StickyStatus.failed:
        return 'Failed';
      case _StickyStatus.cancelled:
        return 'Cancelled';
    }
  }

  Color _statusColor(_StickyStatus status) {
    switch (status) {
      case _StickyStatus.running:
        return const Color(0xFF0F6CBD);
      case _StickyStatus.done:
        return const Color(0xFF107C10);
      case _StickyStatus.failed:
        return const Color(0xFFD13438);
      case _StickyStatus.cancelled:
        return const Color(0xFF605E5C);
    }
  }

  Map<_StickyStatus, int> _stickyCounts(List<_KanbanSticky> stickies) {
    final counts = <_StickyStatus, int>{
      _StickyStatus.running: 0,
      _StickyStatus.done: 0,
      _StickyStatus.failed: 0,
      _StickyStatus.cancelled: 0,
    };

    for (final sticky in stickies) {
      counts[sticky.status] = (counts[sticky.status] ?? 0) + 1;
    }
    return counts;
  }

  Widget _buildWorkspaceNavigation() {
    final activeIndex = _workspaces.indexWhere(
      (item) => item.id == _activeWorkspaceId,
    );
    final selectedIndex = activeIndex < 0 ? 0 : activeIndex;
    final addIndex = _workspaces.length;
    final removeIndex = _workspaces.length + 1;
    final canRemoveWorkspace = _workspaces.length > 1;

    final items = <NavigationPaneItem>[
      ..._workspaces.map(
        (ws) => PaneItem(
          icon: const Icon(FluentIcons.folder),
          title: Text(ws.name),
          body: const SizedBox.shrink(),
        ),
      ),
      PaneItem(
        icon: const Icon(FluentIcons.add),
        title: const Text('New Project'),
        body: const SizedBox.shrink(),
      ),
    ];

    return SizedBox(
      width: 280,
      child: NavigationView(
        paneBodyBuilder: (item, child) => const SizedBox.shrink(),
        pane: NavigationPane(
          displayMode: PaneDisplayMode.expanded,
          toggleable: false,
          size: const NavigationPaneSize(openWidth: 280),
          selected: selectedIndex,
          header: Padding(
            padding: const EdgeInsets.fromLTRB(12, 14, 12, 8),
            child: Row(
              children: [
                Container(
                  width: 30,
                  height: 30,
                  decoration: BoxDecoration(
                    color: const Color(0xFF1E3E36),
                    borderRadius: BorderRadius.circular(8),
                  ),
                  alignment: Alignment.center,
                  child: const Text(
                    'SO',
                    style: TextStyle(
                      color: Colors.white,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
                const SizedBox(width: 10),
                const Expanded(
                  child: Text(
                    'Projects',
                    style: TextStyle(fontWeight: FontWeight.w700, fontSize: 15),
                  ),
                ),
              ],
            ),
          ),
          items: items,
          footerItems: [
            PaneItem(
              icon: const Icon(FluentIcons.delete),
              title: const Text('Remove Current'),
              body: const SizedBox.shrink(),
              enabled: canRemoveWorkspace,
            ),
          ],
          onChanged: (index) {
            if (index < _workspaces.length) {
              _switchWorkspace(_workspaces[index].id);
              return;
            }
            if (index == addIndex) {
              _addWorkspace();
              return;
            }
            if (index == removeIndex && canRemoveWorkspace) {
              final activeId = _activeWorkspaceId;
              if (activeId != null) {
                _removeWorkspace(activeId);
              }
            }
          },
        ),
      ),
    );
  }

  Widget _buildGlobalControlTab(_WorkspaceState ws) {
    final counts = _stickyCounts(ws.stickies);
    final snapshot = <String, dynamic>{
      'workspace': {
        'id': ws.id,
        'name': ws.name,
        'project_id': ws.projectId,
        'target': ws.target,
      },
      'dispatcher': {'kind': ws.dispatcherKind, 'ref': ws.dispatcherRef},
      'session': {'id': ws.sessionId, 'status': ws.sessionStatus},
      'tasks': {
        'running': counts[_StickyStatus.running] ?? 0,
        'done': counts[_StickyStatus.done] ?? 0,
        'failed': counts[_StickyStatus.failed] ?? 0,
        'cancelled': counts[_StickyStatus.cancelled] ?? 0,
      },
    };

    return Container(
      color: const Color(0xFFF8FBFA),
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          PanelCard(
            title: 'Workspace Control (Current Workspace)',
            trailing: Button(
              onPressed: _saveWorkspaceSettings,
              child: const Text('Save Workspace'),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoLabel(
                  label: 'Workspace Name',
                  child: TextBox(controller: _workspaceNameController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Project ID',
                  child: TextBox(controller: _projectIdController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Workspace Folder',
                  child: TextBox(controller: _targetController, maxLines: 2),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Dispatcher',
                  child: ComboBox<String>(
                    isExpanded: true,
                    value: _activeDispatcherValue(),
                    onChanged: (value) {
                      if (value == null) {
                        return;
                      }
                      unawaited(_setActiveDispatcher(value));
                    },
                    items: _dispatcherChoices().map((item) {
                      final text = item.subtitle == null
                          ? item.label
                          : '${item.label} · ${item.subtitle!}';
                      return ComboBoxItem<String>(
                        value: item.value,
                        child: Text(
                          text,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                      );
                    }).toList(),
                  ),
                ),
                const SizedBox(height: 10),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    FilledButton(
                      onPressed: _openSession,
                      child: const Text('Open Session'),
                    ),
                    Button(
                      onPressed: () {
                        unawaited(_pollKernelState());
                      },
                      child: const Text('Poll Running Tasks'),
                    ),
                    Button(
                      onPressed: () {
                        _openTab('kanban-doc');
                      },
                      child: const Text('Open Kanban'),
                    ),
                    Button(
                      onPressed: () {
                        _openTab('process-monitor');
                        unawaited(_refreshClientProcesses());
                      },
                      child: const Text('Open Process Monitor'),
                    ),
                    Button(
                      onPressed: () {
                        _openTab('skills-center');
                      },
                      child: const Text('Open Skills'),
                    ),
                  ],
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          PanelCard(
            title: 'Session / Task Status',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoLabel(
                  label: 'Session ID',
                  child: TextBox(controller: _sessionIdController),
                ),
                const SizedBox(height: 8),
                Wrap(
                  spacing: 8,
                  runSpacing: 8,
                  children: [
                    Button(
                      onPressed: _applySessionId,
                      child: const Text('Apply Session ID'),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Text('Session status: ${ws.sessionStatus}'),
                Text('Running: ${counts[_StickyStatus.running] ?? 0}'),
                Text('Done: ${counts[_StickyStatus.done] ?? 0}'),
                Text('Failed: ${counts[_StickyStatus.failed] ?? 0}'),
                Text('Cancelled: ${counts[_StickyStatus.cancelled] ?? 0}'),
              ],
            ),
          ),
          const SizedBox(height: 12),
          PanelCard(
            title: 'Connection (App-level)',
            trailing: FilledButton(
              onPressed: _saveConnectionSettings,
              child: const Text('Save Connection'),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoLabel(
                  label: 'Base URL',
                  child: TextBox(controller: _baseUrlController),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'Bearer Token (optional)',
                  child: TextBox(controller: _tokenController),
                ),
                const SizedBox(height: 10),
                ToggleSwitch(
                  checked: _connectionAutoRefresh,
                  onChanged: (value) {
                    setState(() {
                      _connectionAutoRefresh = value;
                    });
                  },
                  content: const Text('Auto refresh every 3 seconds'),
                ),
                const SizedBox(height: 10),
                InfoLabel(
                  label: 'No Response Timeout (minutes)',
                  child: TextBox(
                    controller: _noResponseTimeoutController,
                    placeholder: '1 - 120',
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          PanelCard(
            title: 'Runtime Providers',
            trailing: Wrap(
              spacing: 8,
              children: [
                if (_runtimeBusy) const ProgressRing(strokeWidth: 2),
                Button(
                  onPressed: () {
                    unawaited(_openProviderEditor());
                  },
                  child: const Text('Add Provider'),
                ),
              ],
            ),
            child: _runtimeProviders.isEmpty
                ? const Text(
                    'No providers configured. Add one to start routing tasks.',
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: _runtimeProviders.map((provider) {
                      final subtitle = [
                        provider.kind,
                        provider.model,
                      ].where((item) => item.trim().isNotEmpty).join(' / ');

                      return Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: Container(
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: const Color(0x19000000)),
                          ),
                          child: Row(
                            children: [
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      provider.name,
                                      style: const TextStyle(
                                        fontWeight: FontWeight.w700,
                                      ),
                                    ),
                                    Text(
                                      subtitle,
                                      style: TextStyle(color: Colors.grey[110]),
                                    ),
                                  ],
                                ),
                              ),
                              Button(
                                onPressed: () {
                                  unawaited(
                                    _openProviderEditor(provider: provider),
                                  );
                                },
                                child: const Text('Edit'),
                              ),
                              const SizedBox(width: 6),
                              Button(
                                onPressed: () {
                                  unawaited(_confirmDeleteProvider(provider));
                                },
                                child: const Text('Delete'),
                              ),
                            ],
                          ),
                        ),
                      );
                    }).toList(),
                  ),
          ),
          const SizedBox(height: 12),
          PanelCard(
            title: 'Local Clients',
            child: _localClients.isEmpty
                ? const Text('No local clients detected yet.')
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: _localClients.map((client) {
                      final statusText = client.installed
                          ? 'Installed'
                          : 'Not installed';
                      return Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: Container(
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: const Color(0x19000000)),
                          ),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  Expanded(
                                    child: Text(
                                      client.name,
                                      style: const TextStyle(
                                        fontWeight: FontWeight.w700,
                                      ),
                                    ),
                                  ),
                                  Text(statusText),
                                ],
                              ),
                              const SizedBox(height: 4),
                              Text('Command: ${client.command}'),
                              Text(
                                client.running
                                    ? 'State: Running'
                                    : 'State: Idle',
                              ),
                              Text(
                                client.supportsDispatch
                                    ? 'Dispatch: supported'
                                    : 'Dispatch: detect only',
                                style: TextStyle(color: Colors.grey[110]),
                              ),
                            ],
                          ),
                        ),
                      );
                    }).toList(),
                  ),
          ),
          const SizedBox(height: 12),
          PanelCard(
            title: 'SSH Targets',
            trailing: Button(
              onPressed: () {
                unawaited(_openSshTargetEditor());
              },
              child: const Text('Add SSH Target'),
            ),
            child: _sshTargets.isEmpty
                ? const Text('No SSH targets configured yet.')
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: _sshTargets.map((target) {
                      final subtitleParts = <String>[
                        target.host,
                        if (target.port != null) 'port ${target.port}',
                        if ((target.username ?? '').trim().isNotEmpty)
                          'user ${target.username}',
                      ];
                      return Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: Container(
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: const Color(0x19000000)),
                          ),
                          child: Row(
                            children: [
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      target.name,
                                      style: const TextStyle(
                                        fontWeight: FontWeight.w700,
                                      ),
                                    ),
                                    const SizedBox(height: 4),
                                    Text(
                                      subtitleParts.join(' · '),
                                      style: TextStyle(color: Colors.grey[110]),
                                    ),
                                    if ((target.remoteWorkdir ?? '')
                                        .trim()
                                        .isNotEmpty) ...[
                                      const SizedBox(height: 4),
                                      Text(
                                        'workdir: ${target.remoteWorkdir}',
                                        style: TextStyle(
                                          color: Colors.grey[110],
                                        ),
                                      ),
                                    ],
                                  ],
                                ),
                              ),
                              Button(
                                onPressed: () {
                                  unawaited(
                                    _openSshTargetEditor(target: target),
                                  );
                                },
                                child: const Text('Edit'),
                              ),
                              const SizedBox(width: 6),
                              Button(
                                onPressed: () {
                                  unawaited(_confirmDeleteSshTarget(target));
                                },
                                child: const Text('Delete'),
                              ),
                            ],
                          ),
                        ),
                      );
                    }).toList(),
                  ),
          ),
          if (_runtimeError != null) ...[
            const SizedBox(height: 12),
            Text(
              _runtimeError!,
              style: const TextStyle(color: Color(0xFFD13438)),
            ),
          ],
          const SizedBox(height: 12),
          PanelCard(
            title: 'Snapshot',
            child: JsonViewer(data: snapshot),
          ),
        ],
      ),
    );
  }

  Widget _buildSkillsTab(_WorkspaceState ws) {
    final activeCount = _skills.where((skill) => skill.active).length;
    final sessionId = ws.sessionId.trim();
    final snapshot = <String, dynamic>{
      'workspace': ws.id,
      'skills': {
        'total': _skills.length,
        'active': activeCount,
        'store_items': _skillStoreItems.length,
      },
      'sandbox': {
        'session_id': sessionId,
        'pending_approvals': _sandboxApprovals.length,
      },
    };

    return Container(
      color: const Color(0xFFF8FBFA),
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          PanelCard(
            title: 'Skill Management',
            trailing: Wrap(
              spacing: 8,
              children: [
                if (_skillsBusy) const ProgressRing(strokeWidth: 2),
                Button(
                  onPressed: () {
                    unawaited(_refreshSkills());
                  },
                  child: const Text('Refresh'),
                ),
                FilledButton(
                  onPressed: () {
                    unawaited(_openSkillCreator());
                  },
                  child: const Text('Create Skill'),
                ),
              ],
            ),
            child: _skills.isEmpty
                ? const Text(
                    'No skills configured. Create one or provide an existing SKILL.md path.',
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: _skills.map((skill) {
                      final description = skill.description.trim();
                      final subtitleParts = <String>[
                        if (skill.path.trim().isNotEmpty) skill.path.trim(),
                        'updated ${_formatProcessTimestamp(skill.updatedAtMs)}',
                      ];

                      return Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: Container(
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: const Color(0x19000000)),
                          ),
                          child: Row(
                            children: [
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Row(
                                      children: [
                                        Expanded(
                                          child: Text(
                                            skill.name,
                                            style: const TextStyle(
                                              fontWeight: FontWeight.w700,
                                            ),
                                          ),
                                        ),
                                        Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 8,
                                            vertical: 2,
                                          ),
                                          decoration: BoxDecoration(
                                            color: skill.active
                                                ? const Color(0xFF107C10)
                                                : const Color(0xFF605E5C),
                                            borderRadius: BorderRadius.circular(
                                              999,
                                            ),
                                          ),
                                          child: Text(
                                            skill.active
                                                ? 'Active'
                                                : 'Inactive',
                                            style: const TextStyle(
                                              color: Colors.white,
                                              fontSize: 11,
                                            ),
                                          ),
                                        ),
                                      ],
                                    ),
                                    const SizedBox(height: 4),
                                    Text(
                                      subtitleParts.join(' · '),
                                      style: TextStyle(color: Colors.grey[110]),
                                    ),
                                    if (description.isNotEmpty) ...[
                                      const SizedBox(height: 4),
                                      Text(description),
                                    ],
                                  ],
                                ),
                              ),
                              const SizedBox(width: 10),
                              Button(
                                onPressed: _skillsBusy
                                    ? null
                                    : () {
                                        unawaited(
                                          _setSkillActivation(
                                            skill,
                                            !skill.active,
                                          ),
                                        );
                                      },
                                child: Text(
                                  skill.active ? 'Deactivate' : 'Activate',
                                ),
                              ),
                            ],
                          ),
                        ),
                      );
                    }).toList(),
                  ),
          ),
          const SizedBox(height: 12),
          PanelCard(
            title: 'Skill Store',
            child: _skillStoreItems.isEmpty
                ? const Text('Store is empty (placeholder).')
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: _skillStoreItems.map((item) {
                      final title =
                          (item['name'] ??
                                  item['title'] ??
                                  item['id'] ??
                                  'Unnamed item')
                              .toString();
                      final description =
                          item['description']?.toString().trim() ?? '';
                      final itemId = item['item_id']?.toString().trim() ?? '';
                      final installed = item['installed'] == true;
                      final active = item['active'] == true;
                      return Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: Container(
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: const Color(0x19000000)),
                          ),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Row(
                                children: [
                                  Expanded(
                                    child: Text(
                                      title,
                                      style: const TextStyle(
                                        fontWeight: FontWeight.w700,
                                      ),
                                    ),
                                  ),
                                  if (installed)
                                    Container(
                                      padding: const EdgeInsets.symmetric(
                                        horizontal: 8,
                                        vertical: 2,
                                      ),
                                      decoration: BoxDecoration(
                                        color: active
                                            ? const Color(0xFF107C10)
                                            : const Color(0xFF605E5C),
                                        borderRadius: BorderRadius.circular(
                                          999,
                                        ),
                                      ),
                                      child: Text(
                                        active
                                            ? 'Installed · Active'
                                            : 'Installed',
                                        style: const TextStyle(
                                          color: Colors.white,
                                          fontSize: 11,
                                        ),
                                      ),
                                    ),
                                ],
                              ),
                              if (description.isNotEmpty) ...[
                                const SizedBox(height: 4),
                                Text(description),
                              ],
                              if (itemId.isNotEmpty) ...[
                                const SizedBox(height: 8),
                                Align(
                                  alignment: Alignment.centerLeft,
                                  child: Button(
                                    onPressed: (_skillsBusy || installed)
                                        ? null
                                        : () {
                                            unawaited(
                                              _installSkillStoreItem(item),
                                            );
                                          },
                                    child: Text(
                                      installed ? 'Installed' : 'Install',
                                    ),
                                  ),
                                ),
                              ],
                            ],
                          ),
                        ),
                      );
                    }).toList(),
                  ),
          ),
          const SizedBox(height: 12),
          PanelCard(
            title: 'Sandbox Queue (Ask-First)',
            trailing: Wrap(
              spacing: 8,
              children: [
                if (_sandboxApprovalsBusy || _sandboxActionBusy)
                  const ProgressRing(strokeWidth: 2),
                Button(
                  onPressed: () {
                    unawaited(_refreshSandboxApprovals());
                  },
                  child: const Text('Refresh Queue'),
                ),
                FilledButton(
                  onPressed: sessionId.isEmpty
                      ? null
                      : () {
                          unawaited(_openSandboxActionDialog());
                        },
                  child: const Text('Run Sandbox Action'),
                ),
              ],
            ),
            child: sessionId.isEmpty
                ? const Text('Open a session first to run sandbox actions.')
                : _sandboxApprovals.isEmpty
                ? const Text('No pending approvals.')
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: _sandboxApprovals.map((approval) {
                      final action = approval.action;
                      final operation =
                          action['operation']?.toString() ?? 'unknown';
                      final path = action['path']?.toString().trim() ?? '';
                      final command =
                          action['command']?.toString().trim() ?? '';
                      final summaryParts = <String>[
                        operation,
                        if (path.isNotEmpty) 'path=$path',
                        if (command.isNotEmpty) 'cmd=$command',
                      ];

                      return Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: Container(
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(8),
                            border: Border.all(color: const Color(0x19000000)),
                          ),
                          child: Row(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      approval.approvalId,
                                      style: const TextStyle(
                                        fontWeight: FontWeight.w700,
                                      ),
                                    ),
                                    const SizedBox(height: 4),
                                    Text(summaryParts.join(' · ')),
                                    const SizedBox(height: 4),
                                    Text(
                                      approval.reason,
                                      style: TextStyle(color: Colors.grey[110]),
                                    ),
                                    const SizedBox(height: 4),
                                    Text(
                                      'created ${_formatProcessTimestamp(approval.createdAtMs)}',
                                      style: TextStyle(color: Colors.grey[110]),
                                    ),
                                  ],
                                ),
                              ),
                              const SizedBox(width: 10),
                              Button(
                                onPressed:
                                    (_sandboxApprovalsBusy ||
                                        _sandboxActionBusy)
                                    ? null
                                    : () {
                                        unawaited(
                                          _decideSandboxApproval(
                                            approval,
                                            approve: false,
                                          ),
                                        );
                                      },
                                child: const Text('Reject'),
                              ),
                              const SizedBox(width: 6),
                              FilledButton(
                                onPressed:
                                    (_sandboxApprovalsBusy ||
                                        _sandboxActionBusy)
                                    ? null
                                    : () {
                                        unawaited(
                                          _decideSandboxApproval(
                                            approval,
                                            approve: true,
                                          ),
                                        );
                                      },
                                child: const Text('Approve'),
                              ),
                            ],
                          ),
                        ),
                      );
                    }).toList(),
                  ),
          ),
          if (_skillsError != null) ...[
            const SizedBox(height: 12),
            Text(
              _skillsError!,
              style: const TextStyle(color: Color(0xFFD13438)),
            ),
          ],
          if (_sandboxError != null) ...[
            const SizedBox(height: 12),
            Text(
              _sandboxError!,
              style: const TextStyle(color: Color(0xFFD13438)),
            ),
          ],
          const SizedBox(height: 12),
          PanelCard(
            title: 'Snapshot',
            child: JsonViewer(data: snapshot),
          ),
        ],
      ),
    );
  }

  Widget _buildKanbanColumn({
    required String workspaceId,
    required _StickyStatus status,
    required List<_KanbanSticky> items,
  }) {
    return Container(
      decoration: BoxDecoration(
        color: const Color(0xFFFDFEFE),
        borderRadius: BorderRadius.circular(10),
        border: Border.all(color: const Color(0x14000000)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            decoration: BoxDecoration(
              color: const Color(0xFFF3F7F6),
              borderRadius: const BorderRadius.vertical(
                top: Radius.circular(10),
              ),
              border: Border(
                bottom: BorderSide(color: const Color(0x12000000)),
              ),
            ),
            child: Row(
              children: [
                Text(
                  _statusLabel(status),
                  style: const TextStyle(fontWeight: FontWeight.w700),
                ),
                const Spacer(),
                Text(
                  items.length.toString(),
                  style: TextStyle(color: Colors.grey[100]),
                ),
              ],
            ),
          ),
          Expanded(
            child: items.isEmpty
                ? Center(
                    child: Text(
                      'No cards',
                      style: TextStyle(color: Colors.grey[100]),
                    ),
                  )
                : ListView.separated(
                    padding: const EdgeInsets.all(10),
                    itemCount: items.length,
                    separatorBuilder: (context, index) =>
                        const SizedBox(height: 8),
                    itemBuilder: (context, index) {
                      return _buildStickyCard(workspaceId, items[index]);
                    },
                  ),
          ),
        ],
      ),
    );
  }

  Widget _buildStickyCard(String workspaceId, _KanbanSticky sticky) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(10),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 8,
                    vertical: 4,
                  ),
                  decoration: BoxDecoration(
                    color: _statusColor(sticky.status),
                    borderRadius: BorderRadius.circular(999),
                  ),
                  child: Text(
                    _statusLabel(sticky.status),
                    style: const TextStyle(
                      color: Colors.white,
                      fontSize: 11,
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
                const Spacer(),
                Text(
                  sticky.category,
                  style: TextStyle(color: Colors.grey[110]),
                ),
              ],
            ),
            const SizedBox(height: 8),
            Text(
              sticky.title,
              style: const TextStyle(fontWeight: FontWeight.w700),
            ),
            const SizedBox(height: 6),
            Text(sticky.prompt, maxLines: 3, overflow: TextOverflow.ellipsis),
            const SizedBox(height: 8),
            Text(
              'suggested id: ${sticky.suggestedTaskId}',
              style: TextStyle(color: Colors.grey[110]),
            ),
            Text(
              sticky.taskId.trim().isEmpty
                  ? 'task id: pending'
                  : 'task id: ${sticky.taskId}',
              style: TextStyle(color: Colors.grey[110]),
            ),
            if (sticky.note.trim().isNotEmpty) ...[
              const SizedBox(height: 8),
              Text(sticky.note),
            ],
            if (sticky.error != null) ...[
              const SizedBox(height: 6),
              Text(
                sticky.error!,
                style: const TextStyle(color: Color(0xFFD13438)),
              ),
            ],
            const SizedBox(height: 8),
            Row(
              children: [
                if (sticky.busy) ...[
                  const ProgressRing(strokeWidth: 2),
                  const SizedBox(width: 8),
                ],
                Expanded(
                  child: Wrap(
                    spacing: 6,
                    runSpacing: 6,
                    children: [
                      Button(
                        onPressed: sticky.busy
                            ? null
                            : () {
                                unawaited(
                                  _cancelSticky(workspaceId, sticky.id),
                                );
                              },
                        child: const Text('Cancel'),
                      ),
                      Button(
                        onPressed: sticky.busy
                            ? null
                            : () {
                                unawaited(
                                  _refreshSticky(workspaceId, sticky.id),
                                );
                              },
                        child: const Text('Refresh'),
                      ),
                      FilledButton(
                        onPressed: sticky.busy
                            ? null
                            : () {
                                unawaited(_rerunSticky(workspaceId, sticky.id));
                              },
                        child: const Text('Re-run'),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildKanbanTab(_WorkspaceState ws) {
    final running = ws.stickies
        .where((item) => item.status == _StickyStatus.running)
        .toList();
    final failed = ws.stickies
        .where((item) => item.status == _StickyStatus.failed)
        .toList();
    final done = ws.stickies
        .where((item) => item.status == _StickyStatus.done)
        .toList();
    final cancelled = ws.stickies
        .where((item) => item.status == _StickyStatus.cancelled)
        .toList();

    return Container(
      color: const Color(0xFFF7FBFA),
      child: Column(
        children: [
          Container(
            padding: const EdgeInsets.fromLTRB(14, 12, 14, 10),
            decoration: BoxDecoration(
              border: Border(
                bottom: BorderSide(color: const Color(0x12000000)),
              ),
            ),
            child: Row(
              children: [
                const Expanded(
                  child: Text(
                    'Kanban Stickies',
                    style: TextStyle(fontWeight: FontWeight.w700, fontSize: 16),
                  ),
                ),
                Button(
                  onPressed: () {
                    for (final sticky in ws.stickies) {
                      if (sticky.status == _StickyStatus.running) {
                        unawaited(_refreshSticky(ws.id, sticky.id));
                      }
                    }
                  },
                  child: const Text('Refresh Running'),
                ),
              ],
            ),
          ),
          Expanded(
            child: LayoutBuilder(
              builder: (context, constraints) {
                final compact = constraints.maxWidth < 1200;
                if (compact) {
                  return ListView(
                    padding: const EdgeInsets.all(12),
                    children: [
                      SizedBox(
                        height: 300,
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.running,
                          items: running,
                        ),
                      ),
                      const SizedBox(height: 10),
                      SizedBox(
                        height: 300,
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.failed,
                          items: failed,
                        ),
                      ),
                      const SizedBox(height: 10),
                      SizedBox(
                        height: 300,
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.done,
                          items: done,
                        ),
                      ),
                      const SizedBox(height: 10),
                      SizedBox(
                        height: 300,
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.cancelled,
                          items: cancelled,
                        ),
                      ),
                    ],
                  );
                }

                return Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      Expanded(
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.running,
                          items: running,
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.failed,
                          items: failed,
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.done,
                          items: done,
                        ),
                      ),
                      const SizedBox(width: 10),
                      Expanded(
                        child: _buildKanbanColumn(
                          workspaceId: ws.id,
                          status: _StickyStatus.cancelled,
                          items: cancelled,
                        ),
                      ),
                    ],
                  ),
                );
              },
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildProcessTable(List<LocalClientProcessInfo> processes) {
    if (processes.isEmpty) {
      return const Text('No process details available yet.');
    }

    final rows = <TableRow>[
      const TableRow(
        children: [
          Padding(
            padding: EdgeInsets.symmetric(vertical: 6, horizontal: 4),
            child: Text('Name', style: TextStyle(fontWeight: FontWeight.w700)),
          ),
          Padding(
            padding: EdgeInsets.symmetric(vertical: 6, horizontal: 4),
            child: Text('PID', style: TextStyle(fontWeight: FontWeight.w700)),
          ),
          Padding(
            padding: EdgeInsets.symmetric(vertical: 6, horizontal: 4),
            child: Text('PPID', style: TextStyle(fontWeight: FontWeight.w700)),
          ),
          Padding(
            padding: EdgeInsets.symmetric(vertical: 6, horizontal: 4),
            child: Text('CPU', style: TextStyle(fontWeight: FontWeight.w700)),
          ),
          Padding(
            padding: EdgeInsets.symmetric(vertical: 6, horizontal: 4),
            child: Text(
              'Memory',
              style: TextStyle(fontWeight: FontWeight.w700),
            ),
          ),
          Padding(
            padding: EdgeInsets.symmetric(vertical: 6, horizontal: 4),
            child: Text(
              'Elapsed',
              style: TextStyle(fontWeight: FontWeight.w700),
            ),
          ),
          Padding(
            padding: EdgeInsets.symmetric(vertical: 6, horizontal: 4),
            child: Text(
              'Command',
              style: TextStyle(fontWeight: FontWeight.w700),
            ),
          ),
        ],
      ),
    ];

    for (final process in processes) {
      rows.add(
        TableRow(
          decoration: BoxDecoration(
            color: process.isRoot ? const Color(0x142CA05A) : null,
          ),
          children: [
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: Text(
                process.commandName ?? '-',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: Text(process.pid.toString()),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: Text(process.parentPid?.toString() ?? '-'),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: Text(_formatPercent(process.cpuPercent)),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: Text(_formatPercent(process.memoryPercent)),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: Text(process.elapsed ?? '-'),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 6, horizontal: 4),
              child: Text(
                process.commandLine ?? '-',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ],
        ),
      );
    }

    return Table(
      columnWidths: const <int, TableColumnWidth>{
        0: FlexColumnWidth(1.2),
        1: IntrinsicColumnWidth(),
        2: IntrinsicColumnWidth(),
        3: IntrinsicColumnWidth(),
        4: IntrinsicColumnWidth(),
        5: IntrinsicColumnWidth(),
        6: FlexColumnWidth(2.2),
      },
      defaultVerticalAlignment: TableCellVerticalAlignment.middle,
      children: rows,
    );
  }

  Widget _buildProcessTab(_WorkspaceState ws) {
    return Container(
      color: const Color(0xFFF8FCFB),
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          PanelCard(
            title: 'Runtime Process Monitor',
            trailing: Wrap(
              spacing: 8,
              crossAxisAlignment: WrapCrossAlignment.center,
              children: [
                if (_clientProcessesBusy) const ProgressRing(strokeWidth: 2),
                Button(
                  onPressed: () {
                    unawaited(_refreshClientProcesses());
                  },
                  child: const Text('Refresh'),
                ),
              ],
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text('Workspace: ${ws.name} (${ws.id})'),
                Text(
                  'Last update: ${_formatProcessTimestamp(_clientProcessesLastUpdatedMs)}',
                ),
                if (_clientProcessesError != null) ...[
                  const SizedBox(height: 8),
                  Text(
                    'Error: $_clientProcessesError',
                    style: const TextStyle(color: Color(0xFFC4314B)),
                  ),
                ],
              ],
            ),
          ),
          const SizedBox(height: 12),
          if (_clientProcesses.isEmpty)
            const PanelCard(
              title: 'Processes',
              child: Text(
                'No active local-client dispatch process. Submit a task to inspect Codex child processes.',
              ),
            )
          else
            ..._clientProcesses.map((snapshot) {
              final total = snapshot.processes.length;
              final rootPid = snapshot.rootPid?.toString() ?? '-';
              return Padding(
                padding: const EdgeInsets.only(bottom: 12),
                child: PanelCard(
                  title:
                      'Client ${snapshot.clientId} · PID $rootPid · $total process(es)',
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Text('Dispatch ID: ${snapshot.dispatchId}'),
                      Text(
                        'Started: ${_formatProcessTimestamp(snapshot.startedAtMs)}',
                      ),
                      Text('CWD: ${snapshot.cwd ?? '-'}'),
                      Text('Prompt: ${snapshot.promptPreview}'),
                      const SizedBox(height: 8),
                      _buildProcessTable(snapshot.processes),
                    ],
                  ),
                ),
              );
            }),
        ],
      ),
    );
  }

  IconData _tabIcon(_WorkspaceTabKind kind) {
    switch (kind) {
      case _WorkspaceTabKind.globalControl:
        return FluentIcons.settings;
      case _WorkspaceTabKind.kanban:
        return FluentIcons.bulleted_list;
      case _WorkspaceTabKind.chat:
        return FluentIcons.message;
      case _WorkspaceTabKind.process:
        return FluentIcons.processing;
      case _WorkspaceTabKind.skills:
        return FluentIcons.edit_contact;
    }
  }

  Widget _buildChatBubble(_ChatMessage message) {
    final align = message.fromUser
        ? Alignment.centerRight
        : Alignment.centerLeft;
    final background = message.fromUser
        ? const Color(0xFF0F6CBD)
        : const Color(0xFFF1F4F3);
    final foreground = message.fromUser ? Colors.white : Colors.black;

    return Align(
      alignment: align,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 700),
        child: Container(
          margin: const EdgeInsets.only(bottom: 8),
          padding: const EdgeInsets.all(10),
          decoration: BoxDecoration(
            color: background,
            borderRadius: BorderRadius.circular(10),
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(message.text, style: TextStyle(color: foreground)),
              const SizedBox(height: 4),
              Text(
                message.createdAt.toIso8601String(),
                style: TextStyle(
                  color: message.fromUser
                      ? const Color(0xFFECF6FF)
                      : Colors.grey[110],
                  fontSize: 11,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildChatTab(_WorkspaceState ws, _WorkspaceTabItem tab) {
    final messages = ws.chatMessages[tab.id] ?? const <_ChatMessage>[];

    return Container(
      color: const Color(0xFFF8FCFB),
      child: Column(
        children: [
          Container(
            padding: const EdgeInsets.fromLTRB(14, 12, 14, 10),
            decoration: BoxDecoration(
              border: Border(
                bottom: BorderSide(color: const Color(0x12000000)),
              ),
            ),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    tab.title,
                    style: const TextStyle(
                      fontWeight: FontWeight.w700,
                      fontSize: 16,
                    ),
                  ),
                ),
                Button(
                  onPressed: () {
                    _openTab('kanban-doc');
                  },
                  child: const Text('View Kanban'),
                ),
              ],
            ),
          ),
          Expanded(
            child: messages.isEmpty
                ? const SizedBox.shrink()
                : ListView.builder(
                    padding: const EdgeInsets.all(14),
                    itemCount: messages.length,
                    itemBuilder: (context, index) {
                      return _buildChatBubble(messages[index]);
                    },
                  ),
          ),
          Container(
            padding: const EdgeInsets.fromLTRB(14, 8, 14, 14),
            decoration: BoxDecoration(
              color: const Color(0xFFFDFEFE),
              border: Border(top: BorderSide(color: const Color(0x14000000))),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                InfoLabel(
                  label: 'Requirement',
                  child: TextBox(
                    controller: _chatInputController,
                    maxLines: 3,
                    onSubmitted: (_) {
                      unawaited(_sendPromptFromChat(tab.id));
                    },
                  ),
                ),
                const SizedBox(height: 8),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.end,
                  children: [
                    Expanded(
                      child: InfoLabel(
                        label: 'Writer',
                        child: ComboBox<String>(
                          isExpanded: true,
                          value: _activeChatDispatcherValue(ws),
                          onChanged: (value) {
                            if (value == null) {
                              return;
                            }
                            unawaited(_setActiveDispatcher(value));
                          },
                          items: _chatDispatcherChoices().map((item) {
                            final text = item.subtitle == null
                                ? item.label
                                : '${item.label} · ${item.subtitle!}';
                            return ComboBoxItem<String>(
                              value: item.value,
                              child: Text(
                                text,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                            );
                          }).toList(),
                        ),
                      ),
                    ),
                    const SizedBox(width: 12),
                    FilledButton(
                      onPressed: () {
                        unawaited(_sendPromptFromChat(tab.id));
                      },
                      child: const Text('Send'),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Tab _buildTab(_WorkspaceState ws, _WorkspaceTabItem item) {
    Widget body;
    switch (item.kind) {
      case _WorkspaceTabKind.globalControl:
        body = _buildGlobalControlTab(ws);
        break;
      case _WorkspaceTabKind.kanban:
        body = _buildKanbanTab(ws);
        break;
      case _WorkspaceTabKind.chat:
        body = _buildChatTab(ws, item);
        break;
      case _WorkspaceTabKind.process:
        body = _buildProcessTab(ws);
        break;
      case _WorkspaceTabKind.skills:
        body = _buildSkillsTab(ws);
        break;
    }

    return Tab(
      text: Row(
        children: [
          Icon(_tabIcon(item.kind), size: 14),
          const SizedBox(width: 6),
          Text(item.title),
        ],
      ),
      body: body,
      onClosed: item.closable
          ? () {
              _closeTab(item.id);
            }
          : null,
    );
  }

  @override
  Widget build(BuildContext context) {
    final ws = _activeWorkspace;
    if (ws == null) {
      return const SizedBox.shrink();
    }

    int currentIndex = ws.selectedTabIndex;
    if (currentIndex < 0 || currentIndex >= ws.tabs.length) {
      currentIndex = 0;
    }

    return MultiBlocListener(
      listeners: [
        BlocListener<SettingCubit, SettingState>(
          listenWhen: (previous, current) =>
              previous.autoRefresh != current.autoRefresh ||
              previous.baseUrl != current.baseUrl ||
              previous.token != current.token ||
              previous.noResponseTimeoutMinutes !=
                  current.noResponseTimeoutMinutes,
          listener: (context, state) {
            _connectionAutoRefresh = state.autoRefresh;
            _noResponseTimeoutMinutes = state.noResponseTimeoutMinutes;
            if (_baseUrlController.text != state.baseUrl) {
              _baseUrlController.text = state.baseUrl;
            }
            if (_tokenController.text != state.token) {
              _tokenController.text = state.token;
            }
            final timeoutText = state.noResponseTimeoutMinutes.toString();
            if (_noResponseTimeoutController.text != timeoutText) {
              _noResponseTimeoutController.text = timeoutText;
            }
            _syncPolling(state.autoRefresh);
            unawaited(
              _loadRuntimeConfiguration(
                workspaceId: _activeWorkspaceId,
                silent: true,
              ),
            );
          },
        ),
        BlocListener<SessionCubit, SessionState>(
          listenWhen: (previous, current) =>
              previous.sessionId != current.sessionId,
          listener: (context, state) {
            if (_sessionIdController.text != state.sessionId) {
              _sessionIdController.text = state.sessionId;
            }
            _updateActiveWorkspace(
              (item) => item.copyWith(
                sessionId: state.sessionId,
                sessionStatus: state.sessionId.trim().isEmpty
                    ? 'idle'
                    : item.sessionStatus,
              ),
            );
          },
        ),
        BlocListener<NotificationCubit, NotificationState>(
          listenWhen: (previous, current) =>
              current.items.length > previous.items.length,
          listener: (context, state) {
            if (state.items.isEmpty) {
              return;
            }
            final item = state.items.first;
            displayInfoBar(
              context,
              builder: (context, close) {
                return InfoBar(
                  title: Text(item.title),
                  content: Text(item.body),
                  severity: item.title.toLowerCase().contains('error')
                      ? InfoBarSeverity.error
                      : InfoBarSeverity.info,
                );
              },
            );
          },
        ),
      ],
      child: Container(
        color: const Color(0xFFF4F9F7),
        child: Row(
          children: [
            _buildWorkspaceNavigation(),
            Expanded(
              child: TabView(
                currentIndex: currentIndex,
                onChanged: (index) {
                  _updateActiveWorkspace(
                    (item) => item.copyWith(selectedTabIndex: index),
                  );
                },
                onNewPressed: _addChatTab,
                closeButtonVisibility: CloseButtonVisibilityMode.onHover,
                tabWidthBehavior: TabWidthBehavior.sizeToContent,
                tabs: ws.tabs.map((item) => _buildTab(ws, item)).toList(),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
