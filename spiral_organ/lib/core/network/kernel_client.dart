import 'dart:convert';
import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:http/http.dart' as http;
import 'package:web_socket_channel/web_socket_channel.dart';

const int _kernelLogMaxChars = 1200;

void _kernelLog(String message) {
  debugPrint('spiral_organ/kernel: $message');
}

String _compactForLog(dynamic value) {
  if (value == null) {
    return 'null';
  }

  String text;
  if (value is String) {
    text = value;
  } else {
    try {
      text = jsonEncode(value);
    } catch (_) {
      text = value.toString();
    }
  }

  if (text.length <= _kernelLogMaxChars) {
    return text;
  }
  return '${text.substring(0, _kernelLogMaxChars)}...(truncated)';
}

class KernelClient {
  KernelClient({
    required this.baseUrl,
    this.token,
    http.Client? httpClient,
    KernelTransport? transport,
  }) : _transport =
           transport ??
           KernelTransport.defaultTransport(
             baseUrl: baseUrl,
             token: token,
             httpClient: httpClient,
           );

  final String baseUrl;
  final String? token;
  final KernelTransport _transport;

  Future<Map<String, dynamic>> openSession({
    required String projectId,
    required String target,
    String? dispatcherKind,
    String? dispatcherRef,
  }) async {
    return _transport.post(
      '/kernel/session/open',
      body: {
        'project_id': projectId,
        'target': target,
        'dispatcher_kind': dispatcherKind,
        'dispatcher_ref': dispatcherRef,
      },
    );
  }

  Future<Map<String, dynamic>> submitTask({
    required String sessionId,
    required String title,
    String? input,
    String? dispatcherKind,
    String? dispatcherRef,
  }) async {
    return _transport.post(
      '/kernel/task/submit',
      body: {
        'session_id': sessionId,
        'title': title,
        'input': input,
        'dispatcher_kind': dispatcherKind,
        'dispatcher_ref': dispatcherRef,
      },
    );
  }

  Future<Map<String, dynamic>> abortTask({
    required String taskId,
    String? reason,
  }) async {
    return _transport.post(
      '/kernel/task/abort',
      body: {'task_id': taskId, 'reason': reason},
    );
  }

  Future<Map<String, dynamic>> taskStatus(String taskId) async {
    return _transport.get('/kernel/task/$taskId/status');
  }

  Future<List<Map<String, dynamic>>> taskEvents(String taskId) async {
    final response = await _transport.get('/kernel/task/$taskId/events');
    final events = response['events'];
    if (events is! List) {
      return <Map<String, dynamic>>[];
    }

    return events
        .whereType<Map<dynamic, dynamic>>()
        .map((event) => Map<String, dynamic>.from(event))
        .toList();
  }

  Future<Map<String, dynamic>> deploy({
    String? projectId,
    String? sessionId,
  }) async {
    return _transport.post(
      '/kernel/deploy',
      body: {'project_id': projectId, 'session_id': sessionId},
    );
  }

  Future<List<Map<String, dynamic>>> changedFiles({
    required String projectId,
    required String sessionId,
  }) async {
    final response = await _transport.get(
      '/project/$projectId/session/$sessionId/file/status',
    );
    final files = response['files'];
    if (files is! List) {
      return <Map<String, dynamic>>[];
    }

    return files
        .whereType<Map<dynamic, dynamic>>()
        .map((file) => Map<String, dynamic>.from(file))
        .toList();
  }

  Future<Map<String, dynamic>> fileView({
    required String projectId,
    required String sessionId,
    required String path,
    required String view,
  }) async {
    final encodedPath = Uri.encodeQueryComponent(path);
    final encodedView = Uri.encodeQueryComponent(view);
    return _transport.get(
      '/project/$projectId/session/$sessionId/file?path=$encodedPath&view=$encodedView',
    );
  }

  Future<Map<String, dynamic>> changeSummary({
    required String projectId,
    required String sessionId,
  }) async {
    return _transport.get(
      '/project/$projectId/session/$sessionId/changes/summary',
    );
  }

  Future<Map<String, dynamic>> ackChanges({
    required String projectId,
    required String sessionId,
    String actor = 'flutter-user',
    String? note,
  }) async {
    return _transport.post(
      '/project/$projectId/session/$sessionId/changes/ack',
      body: {'actor': actor, 'note': note},
    );
  }

  Future<List<RuntimeProviderConfig>> listRuntimeProviders() async {
    final response = await _transport.get('/kernel/runtime/providers');
    final providers = response['providers'];
    if (providers is! List) {
      return const <RuntimeProviderConfig>[];
    }
    return providers
        .whereType<Map<dynamic, dynamic>>()
        .map(
          (entry) =>
              RuntimeProviderConfig.fromJson(Map<String, dynamic>.from(entry)),
        )
        .toList();
  }

  Future<int> addRuntimeProvider(RuntimeProviderConfig draft) async {
    final response = await _transport.post(
      '/kernel/runtime/providers',
      body: draft.toCreateJson(),
    );
    final value = response['provider_id'];
    if (value is int) {
      return value;
    }
    return int.tryParse(value?.toString() ?? '') ?? 0;
  }

  Future<void> updateRuntimeProvider(RuntimeProviderConfig draft) async {
    if (draft.id == null) {
      throw Exception('provider id is required for update');
    }
    await _transport.post(
      '/kernel/runtime/providers/${draft.id}',
      body: draft.toCreateJson(),
    );
  }

  Future<void> deleteRuntimeProvider(int providerId) async {
    await _transport.delete('/kernel/runtime/providers/$providerId');
  }

  Future<List<LocalClientInfo>> listLocalClients() async {
    final response = await _transport.get('/kernel/runtime/local-clients');
    final clients = response['clients'];
    if (clients is! List) {
      return const <LocalClientInfo>[];
    }
    return clients
        .whereType<Map<dynamic, dynamic>>()
        .map(
          (entry) => LocalClientInfo.fromJson(Map<String, dynamic>.from(entry)),
        )
        .toList();
  }

  Future<List<LocalClientProcessSnapshot>> listLocalClientProcesses() async {
    final response = await _transport.get(
      '/kernel/runtime/local-clients/processes',
    );
    final items = response['processes'];
    if (items is! List) {
      return const <LocalClientProcessSnapshot>[];
    }
    return items
        .whereType<Map<dynamic, dynamic>>()
        .map(
          (entry) => LocalClientProcessSnapshot.fromJson(
            Map<String, dynamic>.from(entry),
          ),
        )
        .toList();
  }

  Future<List<SshTargetConfig>> listRuntimeSshTargets() async {
    final response = await _transport.get('/kernel/runtime/ssh-targets');
    final targets = response['targets'];
    if (targets is! List) {
      return const <SshTargetConfig>[];
    }
    return targets
        .whereType<Map<dynamic, dynamic>>()
        .map(
          (entry) => SshTargetConfig.fromJson(Map<String, dynamic>.from(entry)),
        )
        .toList();
  }

  Future<String> addRuntimeSshTarget(SshTargetConfig draft) async {
    final response = await _transport.post(
      '/kernel/runtime/ssh-targets',
      body: draft.toCreateJson(),
    );
    return response['ssh_target_id']?.toString() ?? '';
  }

  Future<void> updateRuntimeSshTarget(SshTargetConfig draft) async {
    if (draft.sshTargetId.trim().isEmpty) {
      throw Exception('ssh_target_id is required for update');
    }
    final encoded = Uri.encodeComponent(draft.sshTargetId);
    await _transport.post(
      '/kernel/runtime/ssh-targets/$encoded',
      body: draft.toCreateJson(),
    );
  }

  Future<void> deleteRuntimeSshTarget(String sshTargetId) async {
    final encoded = Uri.encodeComponent(sshTargetId);
    await _transport.delete('/kernel/runtime/ssh-targets/$encoded');
  }

  Future<ProjectDispatcherConfig?> getProjectDispatcher(
    String projectId,
  ) async {
    final encoded = Uri.encodeQueryComponent(projectId);
    final response = await _transport.get(
      '/kernel/runtime/dispatcher?project_id=$encoded',
    );
    final value = response['dispatcher'];
    if (value is Map<dynamic, dynamic>) {
      return ProjectDispatcherConfig.fromJson(Map<String, dynamic>.from(value));
    }
    return null;
  }

  Future<ProjectDispatcherConfig?> setProjectDispatcher({
    required String projectId,
    required String targetKind,
    required String targetRef,
  }) async {
    final response = await _transport.post(
      '/kernel/runtime/dispatcher',
      body: {
        'project_id': projectId,
        'target_kind': targetKind,
        'target_ref': targetRef,
      },
    );
    final value = response['dispatcher'];
    if (value is Map<dynamic, dynamic>) {
      return ProjectDispatcherConfig.fromJson(Map<String, dynamic>.from(value));
    }
    return null;
  }

  Future<Map<String, dynamic>> ensureWorkspaceFolder(String path) async {
    return _transport.post(
      '/kernel/workspace/ensure-folder',
      body: {'path': path},
    );
  }

  Future<List<KernelSkill>> listSkills() async {
    final response = await _transport.get('/kernel/skills');
    final skills = response['skills'];
    if (skills is! List) {
      return const <KernelSkill>[];
    }
    return skills
        .whereType<Map<dynamic, dynamic>>()
        .map((entry) => KernelSkill.fromJson(Map<String, dynamic>.from(entry)))
        .toList();
  }

  Future<SkillMutationResult> createSkill({
    required String name,
    String? description,
    String? path,
  }) async {
    final response = await _transport.post(
      '/kernel/skills',
      body: {'name': name, 'description': description, 'path': path},
    );
    return SkillMutationResult.fromJson(response);
  }

  Future<SkillMutationResult> activateSkill(String skillId) async {
    final encoded = Uri.encodeComponent(skillId);
    final response = await _transport.post(
      '/kernel/skills/$encoded/activate',
      body: const <String, dynamic>{},
    );
    return SkillMutationResult.fromJson(response);
  }

  Future<SkillMutationResult> deactivateSkill(String skillId) async {
    final encoded = Uri.encodeComponent(skillId);
    final response = await _transport.post(
      '/kernel/skills/$encoded/deactivate',
      body: const <String, dynamic>{},
    );
    return SkillMutationResult.fromJson(response);
  }

  Future<List<Map<String, dynamic>>> listSkillStoreItems() async {
    final response = await _transport.get('/kernel/skills/store');
    final items = response['items'];
    if (items is! List) {
      return const <Map<String, dynamic>>[];
    }
    return items
        .whereType<Map<dynamic, dynamic>>()
        .map((entry) => Map<String, dynamic>.from(entry))
        .toList();
  }

  Future<SkillMutationResult> installSkillStoreItem(String itemId) async {
    final response = await _transport.post(
      '/kernel/skills/store/install',
      body: {'item_id': itemId},
    );
    return SkillMutationResult.fromJson(response);
  }

  Future<SandboxToolExecuteResult> executeSandboxTool(
    SandboxToolExecuteRequest request,
  ) async {
    final response = await _transport.post(
      '/kernel/sandbox/tools/execute',
      body: request.toJson(),
    );
    return SandboxToolExecuteResult.fromJson(response);
  }

  Future<List<SandboxApproval>> listSandboxApprovals({
    String? sessionId,
    String? taskId,
  }) async {
    final query = <String>[];
    final session = sessionId?.trim();
    if (session != null && session.isNotEmpty) {
      query.add('session_id=${Uri.encodeQueryComponent(session)}');
    }
    final task = taskId?.trim();
    if (task != null && task.isNotEmpty) {
      query.add('task_id=${Uri.encodeQueryComponent(task)}');
    }
    final path = query.isEmpty
        ? '/kernel/sandbox/approvals'
        : '/kernel/sandbox/approvals?${query.join('&')}';
    final response = await _transport.get(path);
    final approvals = response['approvals'];
    if (approvals is! List) {
      return const <SandboxApproval>[];
    }
    return approvals
        .whereType<Map<dynamic, dynamic>>()
        .map(
          (entry) => SandboxApproval.fromJson(Map<String, dynamic>.from(entry)),
        )
        .toList();
  }

  Future<SandboxApprovalDecisionResult> approveSandboxApproval({
    required String approvalId,
    String? actor,
    String? note,
  }) async {
    final encoded = Uri.encodeComponent(approvalId);
    final response = await _transport.post(
      '/kernel/sandbox/approvals/$encoded/approve',
      body: {'actor': actor, 'note': note},
    );
    return SandboxApprovalDecisionResult.fromJson(response);
  }

  Future<SandboxApprovalDecisionResult> rejectSandboxApproval({
    required String approvalId,
    String? actor,
    String? note,
  }) async {
    final encoded = Uri.encodeComponent(approvalId);
    final response = await _transport.post(
      '/kernel/sandbox/approvals/$encoded/reject',
      body: {'actor': actor, 'note': note},
    );
    return SandboxApprovalDecisionResult.fromJson(response);
  }

  Stream<Map<String, dynamic>> watchEvents({
    String? sessionId,
    String? taskId,
  }) {
    return _transport.events(sessionId: sessionId, taskId: taskId);
  }
}

class RuntimeProviderConfig {
  const RuntimeProviderConfig({
    this.id,
    required this.name,
    required this.kind,
    required this.model,
    this.baseUrl,
    this.apiKey,
  });

  final int? id;
  final String name;
  final String kind;
  final String model;
  final String? baseUrl;
  final String? apiKey;

  factory RuntimeProviderConfig.fromJson(Map<String, dynamic> json) {
    final idValue = json['id'];
    final parsedId = idValue is int
        ? idValue
        : int.tryParse(idValue?.toString() ?? '');
    return RuntimeProviderConfig(
      id: parsedId,
      name: json['name']?.toString() ?? '',
      kind: json['kind']?.toString() ?? '',
      model: json['model']?.toString() ?? '',
      baseUrl: _asNullableString(json['base_url']),
      apiKey: _asNullableString(json['api_key']),
    );
  }

  RuntimeProviderConfig copyWith({
    int? id,
    String? name,
    String? kind,
    String? model,
    String? baseUrl,
    String? apiKey,
  }) {
    return RuntimeProviderConfig(
      id: id ?? this.id,
      name: name ?? this.name,
      kind: kind ?? this.kind,
      model: model ?? this.model,
      baseUrl: baseUrl ?? this.baseUrl,
      apiKey: apiKey ?? this.apiKey,
    );
  }

  Map<String, dynamic> toCreateJson() {
    return {
      'name': name,
      'kind': kind,
      'model': model,
      'base_url': baseUrl?.trim().isEmpty == true ? null : baseUrl?.trim(),
      'api_key': apiKey?.trim().isEmpty == true ? null : apiKey?.trim(),
    };
  }
}

class SshTargetConfig {
  const SshTargetConfig({
    this.sshTargetId = '',
    required this.name,
    required this.host,
    this.port,
    this.username,
    this.identityFile,
    this.remoteWorkdir,
    this.options = const <String>[],
    this.createdAtMs = 0,
    this.updatedAtMs = 0,
  });

  final String sshTargetId;
  final String name;
  final String host;
  final int? port;
  final String? username;
  final String? identityFile;
  final String? remoteWorkdir;
  final List<String> options;
  final int createdAtMs;
  final int updatedAtMs;

  bool get isPersisted => sshTargetId.trim().isNotEmpty;

  factory SshTargetConfig.fromJson(Map<String, dynamic> json) {
    final options = json['options'];
    return SshTargetConfig(
      sshTargetId: json['ssh_target_id']?.toString() ?? '',
      name: json['name']?.toString() ?? '',
      host: json['host']?.toString() ?? '',
      port: _toInt(json['port']),
      username: _asNullableString(json['username']),
      identityFile: _asNullableString(json['identity_file']),
      remoteWorkdir: _asNullableString(json['remote_workdir']),
      options: options is List
          ? options.map((entry) => entry.toString()).toList()
          : const <String>[],
      createdAtMs: _toInt(json['created_at_ms']) ?? 0,
      updatedAtMs: _toInt(json['updated_at_ms']) ?? 0,
    );
  }

  SshTargetConfig copyWith({
    String? sshTargetId,
    String? name,
    String? host,
    int? port,
    String? username,
    String? identityFile,
    String? remoteWorkdir,
    List<String>? options,
    int? createdAtMs,
    int? updatedAtMs,
  }) {
    return SshTargetConfig(
      sshTargetId: sshTargetId ?? this.sshTargetId,
      name: name ?? this.name,
      host: host ?? this.host,
      port: port ?? this.port,
      username: username ?? this.username,
      identityFile: identityFile ?? this.identityFile,
      remoteWorkdir: remoteWorkdir ?? this.remoteWorkdir,
      options: options ?? this.options,
      createdAtMs: createdAtMs ?? this.createdAtMs,
      updatedAtMs: updatedAtMs ?? this.updatedAtMs,
    );
  }

  Map<String, dynamic> toCreateJson() {
    return {
      'name': name,
      'host': host,
      'port': port,
      'username': username?.trim().isEmpty == true ? null : username?.trim(),
      'identity_file': identityFile?.trim().isEmpty == true
          ? null
          : identityFile?.trim(),
      'remote_workdir': remoteWorkdir?.trim().isEmpty == true
          ? null
          : remoteWorkdir?.trim(),
      'options': options,
    };
  }
}

class LocalClientInfo {
  const LocalClientInfo({
    required this.id,
    required this.name,
    required this.command,
    required this.processNames,
    required this.installed,
    required this.path,
    required this.running,
    required this.supportsDispatch,
  });

  final String id;
  final String name;
  final String command;
  final List<String> processNames;
  final bool installed;
  final String? path;
  final bool running;
  final bool supportsDispatch;

  factory LocalClientInfo.fromJson(Map<String, dynamic> json) {
    final names = json['process_names'];
    return LocalClientInfo(
      id: json['id']?.toString() ?? '',
      name: json['name']?.toString() ?? '',
      command: json['command']?.toString() ?? '',
      processNames: names is List
          ? names.map((entry) => entry.toString()).toList()
          : const <String>[],
      installed: json['installed'] == true,
      path: _asNullableString(json['path']),
      running: json['running'] == true,
      supportsDispatch: json['supports_dispatch'] == true,
    );
  }
}

class ProjectDispatcherConfig {
  const ProjectDispatcherConfig({
    required this.projectId,
    required this.targetKind,
    required this.targetRef,
    required this.updatedAtMs,
  });

  final String projectId;
  final String targetKind;
  final String targetRef;
  final int updatedAtMs;

  factory ProjectDispatcherConfig.fromJson(Map<String, dynamic> json) {
    final value = json['updated_at_ms'];
    final parsed = value is int ? value : int.tryParse(value?.toString() ?? '');
    return ProjectDispatcherConfig(
      projectId: json['project_id']?.toString() ?? '',
      targetKind: json['target_kind']?.toString() ?? 'provider',
      targetRef: json['target_ref']?.toString() ?? 'default',
      updatedAtMs: parsed ?? 0,
    );
  }
}

class KernelSkill {
  const KernelSkill({
    required this.skillId,
    required this.name,
    required this.description,
    required this.path,
    required this.active,
    required this.createdAtMs,
    required this.updatedAtMs,
  });

  final String skillId;
  final String name;
  final String description;
  final String path;
  final bool active;
  final int createdAtMs;
  final int updatedAtMs;

  factory KernelSkill.fromJson(Map<String, dynamic> json) {
    return KernelSkill(
      skillId: json['skill_id']?.toString() ?? '',
      name: json['name']?.toString() ?? '',
      description: json['description']?.toString() ?? '',
      path: json['path']?.toString() ?? '',
      active: json['active'] == true,
      createdAtMs: _toInt(json['created_at_ms']) ?? 0,
      updatedAtMs: _toInt(json['updated_at_ms']) ?? 0,
    );
  }
}

class SkillMutationResult {
  const SkillMutationResult({required this.skillId, required this.status});

  final String skillId;
  final String status;

  factory SkillMutationResult.fromJson(Map<String, dynamic> json) {
    return SkillMutationResult(
      skillId: json['skill_id']?.toString() ?? '',
      status: json['status']?.toString() ?? '',
    );
  }
}

class SandboxToolExecuteRequest {
  const SandboxToolExecuteRequest({
    required this.sessionId,
    required this.operation,
    this.taskId,
    this.path,
    this.content,
    this.command,
    this.args = const <String>[],
    this.timeoutMs,
  });

  final String sessionId;
  final String? taskId;
  final String operation;
  final String? path;
  final String? content;
  final String? command;
  final List<String> args;
  final int? timeoutMs;

  Map<String, dynamic> toJson() {
    return {
      'session_id': sessionId,
      'task_id': taskId,
      'operation': operation,
      'path': path,
      'content': content,
      'command': command,
      'args': args,
      'timeout_ms': timeoutMs,
    };
  }
}

class SandboxToolExecuteResult {
  const SandboxToolExecuteResult({
    required this.status,
    required this.approvalRequired,
    required this.operation,
    this.approvalId,
    this.result,
  });

  final String status;
  final bool approvalRequired;
  final String operation;
  final String? approvalId;
  final Map<String, dynamic>? result;

  factory SandboxToolExecuteResult.fromJson(Map<String, dynamic> json) {
    final value = json['result'];
    return SandboxToolExecuteResult(
      status: json['status']?.toString() ?? '',
      approvalRequired: json['approval_required'] == true,
      operation: json['operation']?.toString() ?? '',
      approvalId: _asNullableString(json['approval_id']),
      result: value is Map<dynamic, dynamic>
          ? Map<String, dynamic>.from(value)
          : null,
    );
  }
}

class SandboxApproval {
  const SandboxApproval({
    required this.approvalId,
    required this.sessionId,
    required this.taskId,
    required this.reason,
    required this.action,
    required this.createdAtMs,
  });

  final String approvalId;
  final String sessionId;
  final String? taskId;
  final String reason;
  final Map<String, dynamic> action;
  final int createdAtMs;

  factory SandboxApproval.fromJson(Map<String, dynamic> json) {
    final actionValue = json['action'];
    return SandboxApproval(
      approvalId: json['approval_id']?.toString() ?? '',
      sessionId: json['session_id']?.toString() ?? '',
      taskId: _asNullableString(json['task_id']),
      reason: json['reason']?.toString() ?? '',
      action: actionValue is Map<dynamic, dynamic>
          ? Map<String, dynamic>.from(actionValue)
          : const <String, dynamic>{},
      createdAtMs: _toInt(json['created_at_ms']) ?? 0,
    );
  }
}

class SandboxApprovalDecisionResult {
  const SandboxApprovalDecisionResult({
    required this.approvalId,
    required this.status,
    this.result,
  });

  final String approvalId;
  final String status;
  final Map<String, dynamic>? result;

  factory SandboxApprovalDecisionResult.fromJson(Map<String, dynamic> json) {
    final value = json['result'];
    return SandboxApprovalDecisionResult(
      approvalId: json['approval_id']?.toString() ?? '',
      status: json['status']?.toString() ?? '',
      result: value is Map<dynamic, dynamic>
          ? Map<String, dynamic>.from(value)
          : null,
    );
  }
}

class LocalClientProcessSnapshot {
  const LocalClientProcessSnapshot({
    required this.dispatchId,
    required this.clientId,
    required this.startedAtMs,
    required this.rootPid,
    required this.cwd,
    required this.promptPreview,
    required this.command,
    required this.processes,
  });

  final String dispatchId;
  final String clientId;
  final int startedAtMs;
  final int? rootPid;
  final String? cwd;
  final String promptPreview;
  final List<String> command;
  final List<LocalClientProcessInfo> processes;

  factory LocalClientProcessSnapshot.fromJson(Map<String, dynamic> json) {
    final commandValue = json['command'];
    final processValue = json['processes'];
    return LocalClientProcessSnapshot(
      dispatchId: json['dispatch_id']?.toString() ?? '',
      clientId: json['client_id']?.toString() ?? '',
      startedAtMs: _toInt(json['started_at_ms']) ?? 0,
      rootPid: _toInt(json['root_pid']),
      cwd: _asNullableString(json['cwd']),
      promptPreview: json['prompt_preview']?.toString() ?? '',
      command: commandValue is List
          ? commandValue.map((item) => item.toString()).toList()
          : const <String>[],
      processes: processValue is List
          ? processValue
                .whereType<Map<dynamic, dynamic>>()
                .map(
                  (entry) => LocalClientProcessInfo.fromJson(
                    Map<String, dynamic>.from(entry),
                  ),
                )
                .toList()
          : const <LocalClientProcessInfo>[],
    );
  }
}

class LocalClientProcessInfo {
  const LocalClientProcessInfo({
    required this.pid,
    required this.parentPid,
    required this.cpuPercent,
    required this.memoryPercent,
    required this.elapsed,
    required this.commandName,
    required this.commandLine,
    required this.isRoot,
  });

  final int pid;
  final int? parentPid;
  final double? cpuPercent;
  final double? memoryPercent;
  final String? elapsed;
  final String? commandName;
  final String? commandLine;
  final bool isRoot;

  factory LocalClientProcessInfo.fromJson(Map<String, dynamic> json) {
    return LocalClientProcessInfo(
      pid: _toInt(json['pid']) ?? 0,
      parentPid: _toInt(json['parent_pid']),
      cpuPercent: _toDouble(json['cpu_percent']),
      memoryPercent: _toDouble(json['memory_percent']),
      elapsed: _asNullableString(json['elapsed']),
      commandName: _asNullableString(json['command_name']),
      commandLine: _asNullableString(json['command_line']),
      isRoot: json['is_root'] == true,
    );
  }
}

String? _asNullableString(dynamic value) {
  if (value == null) {
    return null;
  }
  final text = value.toString();
  return text.isEmpty ? null : text;
}

int? _toInt(dynamic value) {
  if (value == null) {
    return null;
  }
  if (value is int) {
    return value;
  }
  return int.tryParse(value.toString());
}

double? _toDouble(dynamic value) {
  if (value == null) {
    return null;
  }
  if (value is double) {
    return value;
  }
  if (value is int) {
    return value.toDouble();
  }
  return double.tryParse(value.toString());
}

abstract class KernelTransport {
  Future<Map<String, dynamic>> get(String path);
  Future<Map<String, dynamic>> delete(String path);
  Future<Map<String, dynamic>> post(
    String path, {
    required Map<String, dynamic> body,
  });
  Stream<Map<String, dynamic>> events({String? sessionId, String? taskId});

  static KernelTransport defaultTransport({
    required String baseUrl,
    required String? token,
    http.Client? httpClient,
  }) {
    if (!kIsWeb && defaultTargetPlatform == TargetPlatform.macOS) {
      return MethodChannelKernelTransport();
    }
    return HttpKernelTransport(
      baseUrl: baseUrl,
      token: token,
      httpClient: httpClient,
    );
  }
}

class HttpKernelTransport implements KernelTransport {
  HttpKernelTransport({
    required this.baseUrl,
    this.token,
    http.Client? httpClient,
  }) : _http = httpClient ?? http.Client();

  final String baseUrl;
  final String? token;
  final http.Client _http;

  @override
  Future<Map<String, dynamic>> get(String path) async {
    final uri = _buildHttpUri(path);
    _kernelLog('http request: GET $uri');
    final response = await _http.get(uri, headers: _headers());
    return _decode(response, method: 'GET', uri: uri);
  }

  @override
  Future<Map<String, dynamic>> delete(String path) async {
    final uri = _buildHttpUri(path);
    _kernelLog('http request: DELETE $uri');
    final response = await _http.delete(uri, headers: _headers());
    return _decode(response, method: 'DELETE', uri: uri);
  }

  @override
  Future<Map<String, dynamic>> post(
    String path, {
    required Map<String, dynamic> body,
  }) async {
    final uri = _buildHttpUri(path);
    _kernelLog('http request: POST $uri body=${_compactForLog(body)}');
    final response = await _http.post(
      uri,
      headers: _headers(),
      body: jsonEncode(body),
    );
    return _decode(response, method: 'POST', uri: uri);
  }

  @override
  Stream<Map<String, dynamic>> events({
    String? sessionId,
    String? taskId,
  }) async* {
    final query = <String, String>{};
    final tokenValue = token?.trim();
    if (tokenValue != null && tokenValue.isNotEmpty) {
      query['token'] = tokenValue;
    }
    final trimmedSession = sessionId?.trim();
    if (trimmedSession != null && trimmedSession.isNotEmpty) {
      query['session_id'] = trimmedSession;
    }
    final trimmedTask = taskId?.trim();
    if (trimmedTask != null && trimmedTask.isNotEmpty) {
      query['task_id'] = trimmedTask;
    }

    final uri = _buildWsUri('/kernel/ws', query: query);
    _kernelLog('http ws connect: $uri');
    final channel = WebSocketChannel.connect(uri);
    try {
      await for (final raw in channel.stream) {
        final parsed = _decodeEventPayload(raw);
        if (parsed != null) {
          _kernelLog('http ws event: ${_compactForLog(parsed)}');
          yield parsed;
        }
      }
    } finally {
      await channel.sink.close();
      _kernelLog('http ws closed: $uri');
    }
  }

  Uri _buildHttpUri(String path) {
    return Uri.parse('${baseUrl.trim().replaceAll(RegExp(r'/$'), '')}$path');
  }

  Uri _buildWsUri(String path, {Map<String, String>? query}) {
    final httpUri = _buildHttpUri(path);
    return httpUri.replace(
      scheme: httpUri.scheme == 'https' ? 'wss' : 'ws',
      queryParameters: (query == null || query.isEmpty) ? null : query,
    );
  }

  Map<String, String> _headers() {
    final headers = <String, String>{
      'Content-Type': 'application/json',
      'Accept': 'application/json',
    };
    final value = token?.trim();
    if (value != null && value.isNotEmpty) {
      headers['Authorization'] = 'Bearer $value';
    }
    return headers;
  }

  Map<String, dynamic> _decode(
    http.Response response, {
    required String method,
    required Uri uri,
  }) {
    final body = response.body.trim().isEmpty ? '{}' : response.body;
    final data = jsonDecode(body);
    _kernelLog(
      'http response: $method $uri status=${response.statusCode} body=${_compactForLog(data)}',
    );

    if (response.statusCode >= 200 && response.statusCode < 300) {
      if (data is Map<String, dynamic>) {
        return data;
      }
      return {'data': data};
    }

    final message = data is Map<String, dynamic> && data['error'] != null
        ? data['error'].toString()
        : 'request failed: ${response.statusCode} ${response.reasonPhrase}';
    _kernelLog('http error: $method $uri message=$message');
    throw Exception(message);
  }
}

class MethodChannelKernelTransport implements KernelTransport {
  MethodChannelKernelTransport();

  static const MethodChannel _channel = MethodChannel('spiral_organ/core');
  static const EventChannel _eventsChannel = EventChannel(
    'spiral_organ/events',
  );
  static bool _started = false;

  Future<void> _ensureStarted() async {
    if (_started) {
      return;
    }
    _kernelLog('method-channel request: core.start');
    final started = await _channel.invokeMethod<bool>('core.start');
    _kernelLog('method-channel response: core.start started=$started');
    if (started != true) {
      throw Exception('core.start returned false');
    }
    _started = true;
  }

  @override
  Future<Map<String, dynamic>> get(String path) {
    return _invoke(method: 'GET', path: path);
  }

  @override
  Future<Map<String, dynamic>> delete(String path) {
    return _invoke(method: 'DELETE', path: path);
  }

  @override
  Future<Map<String, dynamic>> post(
    String path, {
    required Map<String, dynamic> body,
  }) {
    return _invoke(method: 'POST', path: path, body: body);
  }

  Future<Map<String, dynamic>> _invoke({
    required String method,
    required String path,
    Map<String, dynamic>? body,
  }) async {
    await _ensureStarted();
    _kernelLog(
      'method-channel request: kernel.invoke method=$method path=$path body=${_compactForLog(body)}',
    );

    final response = await _channel.invokeMethod<dynamic>('kernel.invoke', {
      'method': method,
      'path': path,
      'body': body,
    });
    _kernelLog(
      'method-channel response: kernel.invoke method=$method path=$path payload=${_compactForLog(response)}',
    );

    if (response is Map<Object?, Object?>) {
      return response.map((key, value) => MapEntry(key.toString(), value));
    }

    if (response == null) {
      return <String, dynamic>{};
    }

    _kernelLog(
      'method-channel error: kernel.invoke method=$method path=$path unexpected=${_compactForLog(response)}',
    );
    throw Exception('kernel.invoke returned unexpected payload: $response');
  }

  @override
  Stream<Map<String, dynamic>> events({
    String? sessionId,
    String? taskId,
  }) async* {
    await _ensureStarted();
    final args = <String, dynamic>{};
    final trimmedSession = sessionId?.trim();
    if (trimmedSession != null && trimmedSession.isNotEmpty) {
      args['session_id'] = trimmedSession;
    }
    final trimmedTask = taskId?.trim();
    if (trimmedTask != null && trimmedTask.isNotEmpty) {
      args['task_id'] = trimmedTask;
    }

    _kernelLog(
      'method-channel request: events.listen args=${_compactForLog(args)}',
    );
    await for (final event in _eventsChannel.receiveBroadcastStream(args)) {
      final parsed = _decodeEventPayload(event);
      if (parsed != null) {
        _kernelLog(
          'method-channel response: events.listen payload=${_compactForLog(parsed)}',
        );
        yield parsed;
      }
    }
  }
}

Map<String, dynamic>? _decodeEventPayload(dynamic raw) {
  if (raw == null) {
    return null;
  }
  if (raw is Map<String, dynamic>) {
    return raw;
  }
  if (raw is Map<Object?, Object?>) {
    return raw.map((key, value) => MapEntry(key.toString(), value));
  }
  if (raw is String) {
    final trimmed = raw.trim();
    if (trimmed.isEmpty) {
      return null;
    }
    final decoded = jsonDecode(trimmed);
    if (decoded is Map<String, dynamic>) {
      return decoded;
    }
    if (decoded is Map<Object?, Object?>) {
      return decoded.map((key, value) => MapEntry(key.toString(), value));
    }
  }
  return null;
}
