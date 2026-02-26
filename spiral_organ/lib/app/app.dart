import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import '../bloc/bloc.dart';
import '../features/changes/changes_cubit.dart';
import '../features/notifications/notification_cubit.dart';
import '../features/session/session_cubit.dart';
import '../features/task/task_cubit.dart';
import 'app_shell.dart';

class SpiralFlutterApp extends StatelessWidget {
  const SpiralFlutterApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MultiBlocProvider(
      providers: [
        BlocProvider<GlobalCubit>(create: (_) => GlobalCubit()),
        BlocProvider<SettingCubit>(create: (_) => SettingCubit()),
        BlocProvider<SessionCubit>(create: (_) => SessionCubit()),
        BlocProvider<TaskCubit>(create: (_) => TaskCubit()),
        BlocProvider<ChangesCubit>(create: (_) => ChangesCubit()),
        BlocProvider<NotificationCubit>(create: (_) => NotificationCubit()),
      ],
      child: BlocBuilder<SettingCubit, SettingState>(
        builder: (context, settings) {
          return FluentApp(
            title: 'SpiralOrgan Console',
            debugShowCheckedModeBanner: false,
            themeMode: ThemeMode.light,
            theme: FluentThemeData(
              accentColor: Colors.teal,
              visualDensity: VisualDensity.standard,
            ),
            home: const AppShell(),
          );
        },
      ),
    );
  }
}
