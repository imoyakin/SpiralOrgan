import 'dart:convert';

import 'package:flutter_bloc/flutter_bloc.dart';

import '../../bloc/global/global_cubit.dart';
import '../../core/network/kernel_client.dart';
import 'task_state.dart';

class TaskCubit extends Cubit<TaskState> {
  TaskCubit() : super(const TaskState());

  void setTaskId(String taskId) {
    emit(state.copyWith(taskId: taskId.trim(), clearError: true));
  }

  Future<void> submitTask({
    required KernelClient client,
    required String sessionId,
    required String title,
    String? input,
    required GlobalCubit global,
  }) async {
    if (state.busy) {
      return;
    }
    emit(state.copyWith(busy: true, clearError: true));
    try {
      final response = await client.submitTask(
        sessionId: sessionId,
        title: title,
        input: input,
      );
      final taskId = response['task_id']?.toString() ?? '';
      emit(state.copyWith(taskId: taskId, busy: false));
      global.appendLog('submit-task: task=$taskId');
      await refreshTask(client: client, taskId: taskId, global: global);
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('submit-task failed: $err');
    }
  }

  Future<void> refreshTask({
    required KernelClient client,
    required String taskId,
    required GlobalCubit global,
  }) async {
    if (taskId.trim().isEmpty) {
      global.appendLog('refresh-task skipped: task id is empty');
      return;
    }

    emit(state.copyWith(busy: true, clearError: true));
    try {
      final status = await client.taskStatus(taskId);
      final events = await client.taskEvents(taskId);
      emit(
        state.copyWith(
          taskId: taskId,
          status: status,
          events: events,
          busy: false,
          clearError: true,
        ),
      );
      global.appendLog(
        'refresh-task: status=${status['status']} events=${events.length}',
      );
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('refresh-task failed: $err');
    }
  }

  Future<void> abortTask({
    required KernelClient client,
    required String taskId,
    required GlobalCubit global,
  }) async {
    if (state.busy) {
      return;
    }
    if (taskId.trim().isEmpty) {
      global.appendLog('abort-task skipped: task id is empty');
      return;
    }

    emit(state.copyWith(busy: true, clearError: true));
    try {
      final response = await client.abortTask(
        taskId: taskId,
        reason: 'aborted from flutter ui',
      );
      emit(state.copyWith(busy: false));
      global.appendLog('abort-task: ${jsonEncode(response)}');
      await refreshTask(client: client, taskId: taskId, global: global);
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('abort-task failed: $err');
    }
  }

  Future<void> deploy({
    required KernelClient client,
    required String projectId,
    String? sessionId,
    required GlobalCubit global,
  }) async {
    if (state.busy) {
      return;
    }
    emit(state.copyWith(busy: true, clearError: true));
    try {
      final response = await client.deploy(
        projectId: projectId,
        sessionId: sessionId,
      );
      emit(state.copyWith(busy: false));
      global.appendLog('deploy: ${jsonEncode(response)}');
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('deploy failed: $err');
    }
  }
}
