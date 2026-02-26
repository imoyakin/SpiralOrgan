import 'package:equatable/equatable.dart';

class AppNotification extends Equatable {
  const AppNotification({
    required this.id,
    required this.title,
    required this.body,
    required this.timestamp,
    this.read = false,
  });

  final String id;
  final String title;
  final String body;
  final DateTime timestamp;
  final bool read;

  AppNotification copyWith({bool? read}) {
    return AppNotification(
      id: id,
      title: title,
      body: body,
      timestamp: timestamp,
      read: read ?? this.read,
    );
  }

  @override
  List<Object?> get props => [id, title, body, timestamp, read];
}

class NotificationState extends Equatable {
  const NotificationState({
    this.items = const <AppNotification>[],
    this.seenEventIds = const <String>{},
  });

  final List<AppNotification> items;
  final Set<String> seenEventIds;

  int get unreadCount => items.where((item) => !item.read).length;

  NotificationState copyWith({
    List<AppNotification>? items,
    Set<String>? seenEventIds,
  }) {
    return NotificationState(
      items: items ?? this.items,
      seenEventIds: seenEventIds ?? this.seenEventIds,
    );
  }

  @override
  List<Object?> get props => [items, seenEventIds];
}
