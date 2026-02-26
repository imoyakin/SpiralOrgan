import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import '../../widgets/panel_card.dart';
import 'notification_cubit.dart';
import 'notification_state.dart';

class NotificationsScreen extends StatelessWidget {
  const NotificationsScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: BlocBuilder<NotificationCubit, NotificationState>(
        builder: (context, state) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              PanelCard(
                title: 'Notification Center',
                trailing: Wrap(
                  spacing: 6,
                  children: [
                    Button(
                      onPressed: context.read<NotificationCubit>().markAllRead,
                      child: const Text('Mark All Read'),
                    ),
                    Button(
                      onPressed: context.read<NotificationCubit>().clear,
                      child: const Text('Clear'),
                    ),
                  ],
                ),
                child: Row(
                  children: [
                    Text('Total: ${state.items.length}'),
                    const SizedBox(width: 16),
                    Text('Unread: ${state.unreadCount}'),
                  ],
                ),
              ),
              const SizedBox(height: 12),
              Expanded(
                child: PanelCard(
                  title: 'Events',
                  child: state.items.isEmpty
                      ? const Text('No notifications yet')
                      : ListView.separated(
                          itemCount: state.items.length,
                          separatorBuilder: (context, index) =>
                              const Divider(size: 8),
                          itemBuilder: (context, index) {
                            final item = state.items[index];
                            return Container(
                              padding: const EdgeInsets.all(8),
                              decoration: BoxDecoration(
                                borderRadius: BorderRadius.circular(8),
                                border: Border.all(
                                  color: const Color(0x33000000),
                                ),
                              ),
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Row(
                                    children: [
                                      Expanded(
                                        child: Text(
                                          item.title,
                                          style: const TextStyle(
                                            fontWeight: FontWeight.w700,
                                          ),
                                        ),
                                      ),
                                      if (!item.read)
                                        FilledButton(
                                          onPressed: () {
                                            context
                                                .read<NotificationCubit>()
                                                .markRead(item.id);
                                          },
                                          child: const Text('Read'),
                                        ),
                                    ],
                                  ),
                                  const SizedBox(height: 6),
                                  Text(item.body),
                                  const SizedBox(height: 4),
                                  Text(
                                    item.timestamp.toIso8601String(),
                                    style: const TextStyle(
                                      color: Color(0xFF666666),
                                    ),
                                  ),
                                ],
                              ),
                            );
                          },
                        ),
                ),
              ),
            ],
          );
        },
      ),
    );
  }
}
