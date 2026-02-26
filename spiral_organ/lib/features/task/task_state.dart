import 'package:equatable/equatable.dart';

class TaskState extends Equatable {
  const TaskState({
    this.taskId = '',
    this.status,
    this.events = const <Map<String, dynamic>>[],
    this.busy = false,
    this.error,
  });

  final String taskId;
  final Map<String, dynamic>? status;
  final List<Map<String, dynamic>> events;
  final bool busy;
  final String? error;

  TaskState copyWith({
    String? taskId,
    Map<String, dynamic>? status,
    List<Map<String, dynamic>>? events,
    bool? busy,
    String? error,
    bool clearError = false,
    bool clearStatus = false,
  }) {
    return TaskState(
      taskId: taskId ?? this.taskId,
      status: clearStatus ? null : (status ?? this.status),
      events: events ?? this.events,
      busy: busy ?? this.busy,
      error: clearError ? null : (error ?? this.error),
    );
  }

  @override
  List<Object?> get props => [taskId, status, events, busy, error];
}
