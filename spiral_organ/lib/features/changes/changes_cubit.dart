import 'dart:convert';

import 'package:flutter_bloc/flutter_bloc.dart';

import '../../bloc/global/global_cubit.dart';
import '../../core/network/kernel_client.dart';
import 'changes_state.dart';

class ChangesCubit extends Cubit<ChangesState> {
  ChangesCubit() : super(const ChangesState());

  void selectPath(String path) {
    emit(state.copyWith(selectedPath: path, clearError: true));
  }

  void setViewMode(FileViewMode mode) {
    emit(state.copyWith(viewMode: mode, clearError: true));
  }

  Future<void> refreshChanges({
    required KernelClient client,
    required String projectId,
    required String sessionId,
    required GlobalCubit global,
  }) async {
    if (state.busy) {
      return;
    }
    if (projectId.trim().isEmpty || sessionId.trim().isEmpty) {
      global.appendLog('refresh-changes skipped: project/session id missing');
      return;
    }

    emit(state.copyWith(busy: true, clearError: true));
    try {
      final files = await client.changedFiles(
        projectId: projectId,
        sessionId: sessionId,
      );
      final summary = await client.changeSummary(
        projectId: projectId,
        sessionId: sessionId,
      );

      final selectedPath = state.selectedPath;
      final nextSelectedPath =
          selectedPath != null &&
              files.any((file) => file['path']?.toString() == selectedPath)
          ? selectedPath
          : (files.isNotEmpty ? files.first['path']?.toString() : null);

      emit(
        state.copyWith(
          files: files,
          summary: summary,
          selectedPath: nextSelectedPath,
          busy: false,
          clearError: true,
        ),
      );
      global.appendLog('refresh-changes: files=${files.length}');
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('refresh-changes failed: $err');
    }
  }

  Future<void> loadFileView({
    required KernelClient client,
    required String projectId,
    required String sessionId,
    required String path,
    required FileViewMode mode,
    required GlobalCubit global,
  }) async {
    if (state.busy) {
      return;
    }
    emit(state.copyWith(busy: true, clearError: true));
    try {
      final view = await client.fileView(
        projectId: projectId,
        sessionId: sessionId,
        path: path,
        view: mode.value,
      );
      emit(
        state.copyWith(
          selectedPath: path,
          viewMode: mode,
          fileView: view,
          busy: false,
          clearError: true,
        ),
      );
      global.appendLog('load-file-view: path=$path mode=${mode.value}');
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('load-file-view failed: $err');
    }
  }

  Future<void> ackChanges({
    required KernelClient client,
    required String projectId,
    required String sessionId,
    required GlobalCubit global,
  }) async {
    if (state.busy) {
      return;
    }

    emit(state.copyWith(busy: true, clearError: true));
    try {
      final response = await client.ackChanges(
        projectId: projectId,
        sessionId: sessionId,
        actor: 'flutter-ui',
      );
      emit(state.copyWith(busy: false));
      global.appendLog('ack-changes: ${jsonEncode(response)}');
      await refreshChanges(
        client: client,
        projectId: projectId,
        sessionId: sessionId,
        global: global,
      );
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('ack-changes failed: $err');
    }
  }
}
