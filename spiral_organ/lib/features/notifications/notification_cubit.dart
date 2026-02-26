import 'package:flutter_bloc/flutter_bloc.dart';

import 'notification_state.dart';

class NotificationCubit extends Cubit<NotificationState> {
  NotificationCubit() : super(const NotificationState());

  void ingestTaskEvents(List<Map<String, dynamic>> events) {
    var nextSeen = Set<String>.from(state.seenEventIds);
    var nextItems = List<AppNotification>.from(state.items);

    for (final event in events) {
      final eventId = event['event_id']?.toString();
      final eventType = event['event_type']?.toString() ?? '';
      final payload = event['payload'];
      final summary = payload is Map
          ? payload['summary']?.toString() ?? payload.toString()
          : payload?.toString() ?? '';

      if (eventId == null || eventId.isEmpty || nextSeen.contains(eventId)) {
        continue;
      }
      nextSeen.add(eventId);

      final mapped = _mapEventToNotification(
        eventId: eventId,
        eventType: eventType,
        body: summary,
      );
      if (mapped != null) {
        nextItems = [mapped, ...nextItems];
      }
    }

    emit(state.copyWith(items: nextItems, seenEventIds: nextSeen));
  }

  void addSystemNotification({required String title, required String body}) {
    final notification = AppNotification(
      id: 'sys_${DateTime.now().millisecondsSinceEpoch}',
      title: title,
      body: body,
      timestamp: DateTime.now(),
    );
    emit(state.copyWith(items: [notification, ...state.items]));
  }

  void markRead(String id) {
    final next = state.items
        .map((item) => item.id == id ? item.copyWith(read: true) : item)
        .toList();
    emit(state.copyWith(items: next));
  }

  void markAllRead() {
    emit(
      state.copyWith(
        items: state.items.map((item) => item.copyWith(read: true)).toList(),
      ),
    );
  }

  void clear() {
    emit(const NotificationState());
  }

  AppNotification? _mapEventToNotification({
    required String eventId,
    required String eventType,
    required String body,
  }) {
    switch (eventType) {
      case 'task.done':
        return AppNotification(
          id: eventId,
          title: 'Task Completed',
          body: body.isEmpty ? 'Task finished successfully.' : body,
          timestamp: DateTime.now(),
        );
      case 'task.idle_waiting':
        return AppNotification(
          id: eventId,
          title: 'Idle Waiting',
          body: body.isEmpty
              ? 'No pending tasks. Waiting for instruction.'
              : body,
          timestamp: DateTime.now(),
        );
      case 'task.need_approval':
        return AppNotification(
          id: eventId,
          title: 'Approval Required',
          body: body.isEmpty ? 'Task is blocked waiting approval.' : body,
          timestamp: DateTime.now(),
        );
      case 'task.error':
        return AppNotification(
          id: eventId,
          title: 'Task Error',
          body: body.isEmpty ? 'Task failed. Check logs.' : body,
          timestamp: DateTime.now(),
        );
      default:
        return null;
    }
  }
}
