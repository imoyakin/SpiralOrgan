import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import '../../bloc/bloc.dart';
import '../../core/network/kernel_client.dart';
import '../../widgets/panel_card.dart';
import '../session/session_cubit.dart';
import '../session/session_state.dart';

class SessionScreen extends StatefulWidget {
  const SessionScreen({super.key});

  @override
  State<SessionScreen> createState() => _SessionScreenState();
}

class _SessionScreenState extends State<SessionScreen> {
  late final TextEditingController _baseUrlController;
  late final TextEditingController _tokenController;
  late final TextEditingController _projectIdController;
  late final TextEditingController _targetController;
  late final TextEditingController _sessionIdController;

  @override
  void initState() {
    super.initState();
    final setting = context.read<SettingCubit>().state;
    final session = context.read<SessionCubit>().state;
    _baseUrlController = TextEditingController(text: setting.baseUrl);
    _tokenController = TextEditingController(text: setting.token);
    _projectIdController = TextEditingController(text: setting.projectId);
    _targetController = TextEditingController(text: setting.target);
    _sessionIdController = TextEditingController(text: session.sessionId);
  }

  @override
  void dispose() {
    _baseUrlController.dispose();
    _tokenController.dispose();
    _projectIdController.dispose();
    _targetController.dispose();
    _sessionIdController.dispose();
    super.dispose();
  }

  void _saveSettings() {
    context.read<SettingCubit>().updateConnection(
      baseUrl: _baseUrlController.text,
      token: _tokenController.text,
    );
    context.read<SettingCubit>().updateProject(
      projectId: _projectIdController.text,
      target: _targetController.text,
    );
    context.read<GlobalCubit>().appendLog('settings saved');
  }

  Future<void> _openSession() async {
    _saveSettings();
    final settings = context.read<SettingCubit>().state;
    final client = KernelClient(
      baseUrl: settings.baseUrl,
      token: settings.tokenOrNull,
    );
    await context.read<SessionCubit>().openSession(
      client: client,
      projectId: settings.projectId,
      target: settings.target,
      global: context.read<GlobalCubit>(),
    );
  }

  @override
  Widget build(BuildContext context) {
    return BlocListener<SessionCubit, SessionState>(
      listener: (context, state) {
        if (_sessionIdController.text != state.sessionId) {
          _sessionIdController.text = state.sessionId;
        }
      },
      child: SingleChildScrollView(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            PanelCard(
              title: 'Kernel Connection',
              child: Column(
                children: [
                  InfoLabel(
                    label: 'Base URL',
                    child: TextBox(
                      controller: _baseUrlController,
                      placeholder: 'http://127.0.0.1:8787',
                    ),
                  ),
                  const SizedBox(height: 10),
                  InfoLabel(
                    label: 'Bearer Token (optional)',
                    child: TextBox(controller: _tokenController),
                  ),
                  const SizedBox(height: 10),
                  InfoLabel(
                    label: 'Project ID',
                    child: TextBox(controller: _projectIdController),
                  ),
                  const SizedBox(height: 10),
                  InfoLabel(
                    label: 'Target',
                    child: TextBox(controller: _targetController),
                  ),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      FilledButton(
                        onPressed: _saveSettings,
                        child: const Text('Save Settings'),
                      ),
                      FilledButton(
                        onPressed: _openSession,
                        child: const Text('Open Session'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
            const SizedBox(height: 12),
            PanelCard(
              title: 'Session Binding',
              child: BlocBuilder<SessionCubit, SessionState>(
                builder: (context, state) {
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      InfoLabel(
                        label: 'Session ID',
                        child: TextBox(controller: _sessionIdController),
                      ),
                      const SizedBox(height: 10),
                      Wrap(
                        spacing: 8,
                        runSpacing: 8,
                        children: [
                          Button(
                            onPressed: () {
                              context.read<SessionCubit>().setSessionId(
                                _sessionIdController.text.trim(),
                              );
                              context.read<GlobalCubit>().appendLog(
                                'session-id updated: ${_sessionIdController.text.trim()}',
                              );
                            },
                            child: const Text('Apply Session ID'),
                          ),
                        ],
                      ),
                      const SizedBox(height: 10),
                      Text('Status: ${state.status}'),
                      if (state.error != null)
                        Text(
                          'Error: ${state.error}',
                          style: const TextStyle(color: Color(0xFFD13438)),
                        ),
                    ],
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }
}
