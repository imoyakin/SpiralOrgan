import 'dart:convert';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import '../../bloc/bloc.dart';
import '../../core/network/kernel_client.dart';
import '../../widgets/json_viewer.dart';
import '../../widgets/panel_card.dart';
import '../notifications/notification_cubit.dart';
import '../session/session_cubit.dart';
import 'task_cubit.dart';
import 'task_state.dart';

class TaskScreen extends StatefulWidget {
  const TaskScreen({super.key});

  @override
  State<TaskScreen> createState() => _TaskScreenState();
}

class _TaskScreenState extends State<TaskScreen> {
  late final TextEditingController _taskTitleController;
  late final TextEditingController _taskInputController;
  late final TextEditingController _taskIdController;

  @override
  void initState() {
    super.initState();
    _taskTitleController = TextEditingController(
      text: 'implement requested feature',
    );
    _taskInputController = TextEditingController();
    _taskIdController = TextEditingController(
      text: context.read<TaskCubit>().state.taskId,
    );
  }

  @override
  void dispose() {
    _taskTitleController.dispose();
    _taskInputController.dispose();
    _taskIdController.dispose();
    super.dispose();
  }

  KernelClient _client(BuildContext context) {
    final settings = context.read<SettingCubit>().state;
    return KernelClient(baseUrl: settings.baseUrl, token: settings.tokenOrNull);
  }

  Future<void> _refreshTask(BuildContext context, {String? taskId}) async {
    final globalCubit = context.read<GlobalCubit>();
    final taskCubit = context.read<TaskCubit>();
    final notificationCubit = context.read<NotificationCubit>();
    final id = (taskId ?? _taskIdController.text).trim();
    if (id.isEmpty) {
      globalCubit.appendLog('task id is empty');
      return;
    }
    taskCubit.setTaskId(id);
    await taskCubit.refreshTask(
      client: _client(context),
      taskId: id,
      global: globalCubit,
    );
    if (!mounted) {
      return;
    }
    notificationCubit.ingestTaskEvents(taskCubit.state.events);
  }

  Future<void> _submitTask(BuildContext context) async {
    final globalCubit = context.read<GlobalCubit>();
    final taskCubit = context.read<TaskCubit>();
    final notificationCubit = context.read<NotificationCubit>();
    final sessionCubit = context.read<SessionCubit>();
    final sessionId = sessionCubit.state.sessionId.trim();
    if (sessionId.isEmpty) {
      globalCubit.appendLog('submit-task: session id is empty');
      return;
    }

    await taskCubit.submitTask(
      client: _client(context),
      sessionId: sessionId,
      title: _taskTitleController.text.trim(),
      input: _taskInputController.text.trim().isEmpty
          ? null
          : _taskInputController.text.trim(),
      global: globalCubit,
    );
    if (!mounted) {
      return;
    }
    _taskIdController.text = taskCubit.state.taskId;
    notificationCubit.ingestTaskEvents(taskCubit.state.events);
  }

  Future<void> _abortTask(BuildContext context) async {
    final globalCubit = context.read<GlobalCubit>();
    final taskCubit = context.read<TaskCubit>();
    final notificationCubit = context.read<NotificationCubit>();
    final taskId = _taskIdController.text.trim();
    await taskCubit.abortTask(
      client: _client(context),
      taskId: taskId,
      global: globalCubit,
    );
    if (!mounted) {
      return;
    }
    notificationCubit.ingestTaskEvents(taskCubit.state.events);
  }

  Future<void> _deploy(BuildContext context) async {
    final taskCubit = context.read<TaskCubit>();
    final notificationCubit = context.read<NotificationCubit>();
    final globalCubit = context.read<GlobalCubit>();
    final settings = context.read<SettingCubit>().state;
    final sessionId = context.read<SessionCubit>().state.sessionId.trim();
    await taskCubit.deploy(
      client: _client(context),
      projectId: settings.projectId,
      sessionId: sessionId.isEmpty ? null : sessionId,
      global: globalCubit,
    );
    if (!mounted) {
      return;
    }
    notificationCubit.addSystemNotification(
      title: 'Deploy Requested',
      body: 'Kernel deployment request sent.',
    );
  }

  @override
  Widget build(BuildContext context) {
    return BlocListener<TaskCubit, TaskState>(
      listener: (context, state) {
        if (state.taskId.isNotEmpty && state.taskId != _taskIdController.text) {
          _taskIdController.text = state.taskId;
        }
      },
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PanelCard(
              title: 'Task Controls',
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  InfoLabel(
                    label: 'Task Title',
                    child: TextBox(controller: _taskTitleController),
                  ),
                  const SizedBox(height: 10),
                  InfoLabel(
                    label: 'Task Input (optional)',
                    child: TextBox(
                      controller: _taskInputController,
                      maxLines: 3,
                    ),
                  ),
                  const SizedBox(height: 10),
                  InfoLabel(
                    label: 'Task ID',
                    child: TextBox(
                      controller: _taskIdController,
                      onChanged: (value) {
                        context.read<TaskCubit>().setTaskId(value);
                      },
                    ),
                  ),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      FilledButton(
                        onPressed: () => _submitTask(context),
                        child: const Text('Submit Task'),
                      ),
                      Button(
                        onPressed: () => _refreshTask(context),
                        child: const Text('Refresh Task'),
                      ),
                      Button(
                        onPressed: () => _abortTask(context),
                        child: const Text('Abort Task'),
                      ),
                      Button(
                        onPressed: () => _deploy(context),
                        child: const Text('Deploy'),
                      ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  BlocBuilder<SettingCubit, SettingState>(
                    builder: (context, settings) {
                      return ToggleSwitch(
                        content: const Text('Auto Refresh (3s)'),
                        checked: settings.autoRefresh,
                        onChanged: (enabled) {
                          context.read<SettingCubit>().setAutoRefresh(enabled);
                        },
                      );
                    },
                  ),
                ],
              ),
            ),
            const SizedBox(height: 12),
            BlocBuilder<TaskCubit, TaskState>(
              builder: (context, taskState) {
                return Wrap(
                  spacing: 12,
                  runSpacing: 12,
                  children: [
                    SizedBox(
                      width: 540,
                      child: PanelCard(
                        title: 'Task Status',
                        child: JsonViewer(data: taskState.status),
                      ),
                    ),
                    SizedBox(
                      width: 540,
                      child: PanelCard(
                        title: 'Task Events',
                        child: taskState.events.isEmpty
                            ? const Text('No events yet')
                            : SizedBox(
                                height: 280,
                                child: ListView.separated(
                                  itemCount: taskState.events.length,
                                  separatorBuilder: (context, index) =>
                                      const Divider(size: 8),
                                  itemBuilder: (context, index) {
                                    final event = taskState.events[index];
                                    return SelectableText(
                                      const JsonEncoder.withIndent(
                                        '  ',
                                      ).convert(event),
                                      style: const TextStyle(
                                        fontFamily: 'monospace',
                                      ),
                                    );
                                  },
                                ),
                              ),
                      ),
                    ),
                  ],
                );
              },
            ),
            const SizedBox(height: 12),
            BlocBuilder<GlobalCubit, GlobalState>(
              builder: (context, global) {
                return PanelCard(
                  title: 'Client Logs',
                  trailing: Button(
                    onPressed: context.read<GlobalCubit>().clearLogs,
                    child: const Text('Clear'),
                  ),
                  child: global.logs.isEmpty
                      ? const Text('No logs yet')
                      : SizedBox(
                          height: 240,
                          child: ListView.builder(
                            itemCount: global.logs.length,
                            itemBuilder: (context, index) {
                              return SelectableText(
                                global.logs[index],
                                style: const TextStyle(fontFamily: 'monospace'),
                              );
                            },
                          ),
                        ),
                );
              },
            ),
          ],
        ),
      ),
    );
  }
}
