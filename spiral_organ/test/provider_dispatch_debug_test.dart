// ignore_for_file: avoid_print

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:spiral_organ/core/network/kernel_client.dart';

void main() {
  final enabled = Platform.environment['SPIRAL_DEBUG_PROVIDER'] == '1';

  test('provider dispatch debug smoke', () async {
    if (!enabled) {
      print(
        'provider debug smoke skipped (set SPIRAL_DEBUG_PROVIDER=1 to enable)',
      );
      return;
    }

    final providerName =
        Platform.environment['SPIRAL_DEBUG_PROVIDER_NAME'] ??
        'debug-provider-${DateTime.now().millisecondsSinceEpoch}';
    final providerKind =
        Platform.environment['SPIRAL_DEBUG_PROVIDER_KIND'] ?? 'openai';
    final providerModel =
        Platform.environment['SPIRAL_DEBUG_PROVIDER_MODEL'] ?? 'gpt-4o-mini';
    final providerApiKey =
        Platform.environment['SPIRAL_DEBUG_PROVIDER_API_KEY'] ?? '';
    final providerBaseUrl =
        Platform.environment['SPIRAL_DEBUG_PROVIDER_BASE_URL'] ?? '';

    if (providerApiKey.trim().isEmpty) {
      throw Exception(
        'SPIRAL_DEBUG_PROVIDER_API_KEY is required when SPIRAL_DEBUG_PROVIDER=1',
      );
    }

    final projectRoot = Directory.current.parent.path;
    final port = _pickPort();
    final baseUrl = 'http://127.0.0.1:$port';
    final server = await Process.start(
      'cargo',
      <String>[
        'run',
        '-p',
        'spiral_organ_core',
        '--',
        'serve',
        '127.0.0.1:$port',
      ],
      environment: <String, String>{
        ...Platform.environment,
        // Debug runs should not block on approvals.
        'SPIRAL_ORGAN_SANDBOX_APPROVAL_MODE': 'auto',
        // Use dev policy to allow broader tooling during debugging.
        'SPIRAL_ORGAN_POLICY_PATH': '.design/specs/policy.dev.toml',
      },
      workingDirectory: projectRoot,
    );

    final serverOut = StringBuffer();
    final serverErr = StringBuffer();
    final stdoutSub = server.stdout
        .transform(utf8.decoder)
        .listen((chunk) => serverOut.write(chunk));
    final stderrSub = server.stderr
        .transform(utf8.decoder)
        .listen((chunk) => serverErr.write(chunk));

    try {
      await _waitForServerReady(baseUrl);

      final client = KernelClient(
        baseUrl: baseUrl,
        transport: HttpKernelTransport(baseUrl: baseUrl, token: null),
      );

      final providerId = await client.addRuntimeProvider(
        RuntimeProviderConfig(
          name: providerName,
          kind: providerKind,
          model: providerModel,
          baseUrl: providerBaseUrl.trim().isEmpty ? null : providerBaseUrl,
          apiKey: providerApiKey,
        ),
      );
      expect(providerId, greaterThan(0));

      final open = await client.openSession(
        projectId: 'project-debug-provider',
        target: '.runtime/workspaces/ws-debug-provider',
        dispatcherKind: 'provider',
        dispatcherRef: providerId.toString(),
      );
      final sessionId = open['session_id']?.toString() ?? '';
      expect(sessionId, isNotEmpty, reason: 'session should be created');

      final prompt =
          Platform.environment['SPIRAL_DEBUG_PROMPT'] ??
          '请在workspace里创建 hello.txt，内容为 "hello provider"。然后运行 ls 与 cat hello.txt，并返回输出。';
      final submit = await client.submitTask(
        sessionId: sessionId,
        title: 'debug-provider-dispatch',
        input: prompt,
      );
      final taskId = submit['task_id']?.toString() ?? '';
      expect(taskId, isNotEmpty, reason: 'task id should be returned');

      String finalStatus = 'running';
      var lastEventCount = -1;
      final startedAt = DateTime.now();
      while (DateTime.now().difference(startedAt) <
          const Duration(minutes: 6)) {
        final status = await client.taskStatus(taskId);
        final events = await client.taskEvents(taskId);
        finalStatus = status['status']?.toString() ?? 'unknown';

        if (events.length != lastEventCount) {
          lastEventCount = events.length;
          final latestType = events.isEmpty
              ? 'none'
              : events.last['event_type']?.toString() ?? 'unknown';
          print(
            'debug poll: status=$finalStatus events=${events.length} latest=$latestType',
          );
        }

        final replies = _extractReplies(events);
        for (final reply in replies) {
          print('debug provider reply: ${_preview(reply)}');
        }

        if (finalStatus == 'done' ||
            finalStatus == 'error' ||
            finalStatus == 'aborted') {
          break;
        }
        await Future<void>.delayed(const Duration(seconds: 2));
      }

      expect(
        finalStatus,
        anyOf('done', 'error', 'aborted'),
        reason:
            'task never reached terminal status; check server stderr logs for blocking dispatch',
      );
    } finally {
      server.kill(ProcessSignal.sigterm);
      await server.exitCode.timeout(
        const Duration(seconds: 3),
        onTimeout: () {
          server.kill(ProcessSignal.sigkill);
          return 137;
        },
      );
      await stdoutSub.cancel();
      await stderrSub.cancel();

      final stderrText = serverErr.toString().trim();
      if (stderrText.isNotEmpty) {
        print('--- spiral_organ_core stderr ---');
        print(stderrText);
      }
      final stdoutText = serverOut.toString().trim();
      if (stdoutText.isNotEmpty) {
        print('--- spiral_organ_core stdout ---');
        print(stdoutText);
      }
    }
  }, timeout: const Timeout(Duration(minutes: 7)));
}

int _pickPort() {
  final fromEnv = int.tryParse(Platform.environment['SPIRAL_DEBUG_PORT'] ?? '');
  if (fromEnv != null && fromEnv > 0) {
    return fromEnv;
  }
  final stamp = DateTime.now().millisecondsSinceEpoch % 1000;
  return 8900 + stamp % 100;
}

Future<void> _waitForServerReady(String baseUrl) async {
  final client = HttpClient();
  try {
    for (var i = 0; i < 80; i++) {
      try {
        final req = await client.getUrl(
          Uri.parse('$baseUrl/kernel/runtime/providers'),
        );
        final resp = await req.close();
        if (resp.statusCode >= 200 && resp.statusCode < 500) {
          return;
        }
      } catch (_) {
        // Retry.
      }
      await Future<void>.delayed(const Duration(milliseconds: 250));
    }
  } finally {
    client.close(force: true);
  }
  throw Exception('kernel server did not become ready');
}

List<String> _extractReplies(List<Map<String, dynamic>> events) {
  final replies = <String>[];
  for (final event in events) {
    final payload = event['payload'];
    if (payload is! Map<String, dynamic>) {
      continue;
    }

    final direct = _extractFromPayload(payload);
    if (direct != null && direct.isNotEmpty) {
      replies.add(direct);
    }

    final pipeline = payload['pipeline_result'];
    if (pipeline is Map<String, dynamic>) {
      final fromPipeline = _extractFromPipelineResult(pipeline);
      if (fromPipeline != null && fromPipeline.isNotEmpty) {
        replies.add(fromPipeline);
      }
    }
  }
  return replies.toSet().toList();
}

String? _extractFromPayload(Map<String, dynamic> payload) {
  for (final key in <String>['message', 'text', 'output', 'reason', 'error']) {
    final value = payload[key];
    if (value is String && value.trim().isNotEmpty) {
      final parsed = _extractAgentMessage(value);
      return parsed ?? value.trim();
    }
  }
  return null;
}

String? _extractFromPipelineResult(Map<String, dynamic> pipeline) {
  final stages = pipeline['stages'];
  if (stages is List) {
    for (final stage in stages.reversed) {
      if (stage is! Map<String, dynamic>) {
        continue;
      }
      final output = stage['output'];
      if (output is String && output.trim().isNotEmpty) {
        final parsed = _extractAgentMessage(output);
        return parsed ?? output.trim();
      }
    }
  }
  return null;
}

String? _extractAgentMessage(String raw) {
  for (final line in const LineSplitter().convert(raw)) {
    final trimmed = line.trim();
    if (trimmed.isEmpty || !trimmed.startsWith('{')) {
      continue;
    }
    try {
      final decoded = jsonDecode(trimmed);
      if (decoded is! Map<String, dynamic>) {
        continue;
      }
      final item = decoded['item'];
      if (item is! Map<String, dynamic>) {
        continue;
      }
      if (item['type']?.toString() == 'agent_message') {
        final text = item['text']?.toString().trim();
        if (text != null && text.isNotEmpty) {
          return text;
        }
      }
    } catch (_) {
      // Ignore.
    }
  }
  return null;
}

String _preview(String text) {
  final trimmed = text.trim();
  if (trimmed.length <= 240) {
    return trimmed;
  }
  return '${trimmed.substring(0, 240)}...(truncated)';
}
