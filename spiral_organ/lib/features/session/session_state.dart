import 'package:equatable/equatable.dart';

class SessionState extends Equatable {
  const SessionState({
    this.sessionId = '',
    this.status = 'idle',
    this.busy = false,
    this.error,
  });

  final String sessionId;
  final String status;
  final bool busy;
  final String? error;

  SessionState copyWith({
    String? sessionId,
    String? status,
    bool? busy,
    String? error,
    bool clearError = false,
  }) {
    return SessionState(
      sessionId: sessionId ?? this.sessionId,
      status: status ?? this.status,
      busy: busy ?? this.busy,
      error: clearError ? null : (error ?? this.error),
    );
  }

  @override
  List<Object?> get props => [sessionId, status, busy, error];
}
