import 'package:flutter_bloc/flutter_bloc.dart';

import '../../bloc/global/global_cubit.dart';
import '../../core/network/kernel_client.dart';
import 'session_state.dart';

class SessionCubit extends Cubit<SessionState> {
  SessionCubit() : super(const SessionState());

  void setSessionId(String sessionId) {
    emit(
      state.copyWith(
        sessionId: sessionId,
        status: sessionId.trim().isEmpty ? 'idle' : state.status,
        clearError: true,
      ),
    );
  }

  Future<void> openSession({
    required KernelClient client,
    required String projectId,
    required String target,
    String? dispatcherKind,
    String? dispatcherRef,
    required GlobalCubit global,
  }) async {
    if (state.busy) {
      return;
    }

    emit(state.copyWith(busy: true, clearError: true));
    try {
      final response = await client.openSession(
        projectId: projectId,
        target: target,
        dispatcherKind: dispatcherKind,
        dispatcherRef: dispatcherRef,
      );
      final sessionId = response['session_id']?.toString() ?? '';
      final status = response['status']?.toString() ?? 'running';
      emit(
        state.copyWith(
          sessionId: sessionId,
          status: status,
          busy: false,
          clearError: true,
        ),
      );
      global.appendLog('open-session: session=$sessionId status=$status');
    } catch (err) {
      emit(state.copyWith(busy: false, error: err.toString()));
      global.appendLog('open-session failed: $err');
    }
  }
}
