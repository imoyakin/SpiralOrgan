import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import '../../bloc/bloc.dart';
import '../../core/network/kernel_client.dart';
import '../../widgets/json_viewer.dart';
import '../../widgets/panel_card.dart';
import '../session/session_cubit.dart';
import 'changes_cubit.dart';
import 'changes_state.dart';

class ChangesScreen extends StatelessWidget {
  const ChangesScreen({super.key});

  KernelClient _client(BuildContext context) {
    final settings = context.read<SettingCubit>().state;
    return KernelClient(baseUrl: settings.baseUrl, token: settings.tokenOrNull);
  }

  Future<void> _refresh(BuildContext context) async {
    final settings = context.read<SettingCubit>().state;
    final sessionId = context.read<SessionCubit>().state.sessionId.trim();
    if (sessionId.isEmpty) {
      context.read<GlobalCubit>().appendLog(
        'refresh-changes: session id empty',
      );
      return;
    }
    await context.read<ChangesCubit>().refreshChanges(
      client: _client(context),
      projectId: settings.projectId,
      sessionId: sessionId,
      global: context.read<GlobalCubit>(),
    );
  }

  Future<void> _loadView(
    BuildContext context,
    String path,
    FileViewMode mode,
  ) async {
    final settings = context.read<SettingCubit>().state;
    final sessionId = context.read<SessionCubit>().state.sessionId.trim();
    if (sessionId.isEmpty) {
      context.read<GlobalCubit>().appendLog('load-file-view: session id empty');
      return;
    }
    await context.read<ChangesCubit>().loadFileView(
      client: _client(context),
      projectId: settings.projectId,
      sessionId: sessionId,
      path: path,
      mode: mode,
      global: context.read<GlobalCubit>(),
    );
  }

  Future<void> _ack(BuildContext context) async {
    final settings = context.read<SettingCubit>().state;
    final sessionId = context.read<SessionCubit>().state.sessionId.trim();
    if (sessionId.isEmpty) {
      context.read<GlobalCubit>().appendLog('ack-changes: session id empty');
      return;
    }
    await context.read<ChangesCubit>().ackChanges(
      client: _client(context),
      projectId: settings.projectId,
      sessionId: sessionId,
      global: context.read<GlobalCubit>(),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(16),
      child: BlocBuilder<ChangesCubit, ChangesState>(
        builder: (context, state) {
          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  FilledButton(
                    onPressed: () => _refresh(context),
                    child: const Text('Refresh Changes'),
                  ),
                  Button(
                    onPressed: () => _ack(context),
                    child: const Text('Ack Changes'),
                  ),
                  if (state.selectedPath != null)
                    ComboBox<FileViewMode>(
                      value: state.viewMode,
                      onChanged: (mode) async {
                        if (mode == null || state.selectedPath == null) {
                          return;
                        }
                        context.read<ChangesCubit>().setViewMode(mode);
                        await _loadView(context, state.selectedPath!, mode);
                      },
                      items: FileViewMode.values
                          .map(
                            (mode) => ComboBoxItem<FileViewMode>(
                              value: mode,
                              child: Text(mode.value),
                            ),
                          )
                          .toList(),
                    ),
                ],
              ),
              const SizedBox(height: 12),
              Expanded(
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    SizedBox(
                      width: 360,
                      child: PanelCard(
                        title: 'Changed Files',
                        child: state.files.isEmpty
                            ? const Text('No changed files')
                            : ListView.separated(
                                itemCount: state.files.length,
                                separatorBuilder: (context, index) =>
                                    const Divider(size: 8),
                                itemBuilder: (context, index) {
                                  final file = state.files[index];
                                  final path = file['path']?.toString() ?? '';
                                  final selected = path == state.selectedPath;
                                  return GestureDetector(
                                    onTap: () async {
                                      context.read<ChangesCubit>().selectPath(
                                        path,
                                      );
                                      await _loadView(
                                        context,
                                        path,
                                        state.viewMode,
                                      );
                                    },
                                    child: Container(
                                      padding: const EdgeInsets.all(8),
                                      decoration: BoxDecoration(
                                        color: selected
                                            ? const Color(0x1F2E8B57)
                                            : Colors.transparent,
                                        borderRadius: BorderRadius.circular(8),
                                      ),
                                      child: Column(
                                        crossAxisAlignment:
                                            CrossAxisAlignment.start,
                                        children: [
                                          Text(
                                            path,
                                            style: const TextStyle(
                                              fontWeight: FontWeight.w600,
                                            ),
                                          ),
                                          const SizedBox(height: 4),
                                          Text(
                                            'status=${file['status']} +${file['additions']} -${file['deletions']}',
                                          ),
                                          Text(
                                            'task=${file['task_id']} agent=${file['agent_id']}',
                                            style: const TextStyle(
                                              color: Color(0xFF666666),
                                            ),
                                          ),
                                        ],
                                      ),
                                    ),
                                  );
                                },
                              ),
                      ),
                    ),
                    const SizedBox(width: 12),
                    Expanded(
                      child: Column(
                        children: [
                          PanelCard(
                            title: 'Change Summary',
                            child: JsonViewer(data: state.summary),
                          ),
                          const SizedBox(height: 12),
                          Expanded(
                            child: PanelCard(
                              title: 'File View',
                              child: JsonViewer(data: state.fileView),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
              ),
              if (state.error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 8),
                  child: Text(
                    state.error!,
                    style: const TextStyle(color: Color(0xFFD13438)),
                  ),
                ),
            ],
          );
        },
      ),
    );
  }
}
