// ignore_for_file: avoid_print

import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

import 'package:spiral_organ/core/network/kernel_client.dart';

void main() {
  final enabled = Platform.environment['SPIRAL_DEBUG_CODEX'] == '1';

  test(
    'codex dispatch debug smoke',
    () async {
      if (!enabled) {
        // Keep default CI/local test runs fast and deterministic.
        // Run this debug smoke only when explicitly requested.
        // Example:
        //   SPIRAL_DEBUG_CODEX=1 flutter test test/codex_dispatch_debug_test.dart
        print('codex debug smoke skipped (set SPIRAL_DEBUG_CODEX=1 to enable)');
        return;
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
        final open = await client.openSession(
          projectId: 'project-debug-codex',
          target: '.runtime/workspaces/ws-debug-codex',
          dispatcherKind: 'local_client',
          dispatcherRef: 'codex',
        );
        final sessionId = open['session_id']?.toString() ?? '';
        expect(sessionId, isNotEmpty, reason: 'session should be created');

        final prompt =
            Platform.environment['SPIRAL_DEBUG_PROMPT'] ??
            '请在当前目录用golang创建 main.go 并打印 hello world，然后执行 go run main.go 并返回输出。';
        final submit = await client.submitTask(
          sessionId: sessionId,
          title: 'debug-codex-dispatch',
          input: prompt,
        );
        final taskId = submit['task_id']?.toString() ?? '';
        expect(taskId, isNotEmpty, reason: 'task id should be returned');

        String finalStatus = 'running';
        var lastEventCount = -1;
        final startedAt = DateTime.now();
        while (DateTime.now().difference(startedAt) < const Duration(minutes: 6)) {
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
            print('debug codex reply: ${_preview(reply)}');
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
    },
    timeout: const Timeout(Duration(minutes: 7)),
  );
}

int _pickPort() {
  final fromEnv = int.tryParse(Platform.environment['SPIRAL_DEBUG_PORT'] ?? '');
  if (fromEnv != null && fromEnv > 0) {
    return fromEnv;
  }
  final stamp = DateTime.now().millisecondsSinceEpoch % 1000;
  return 8800 + stamp % 100;
}

Future<void> _waitForServerReady(String baseUrl) async {
  final client = HttpClient();
  try {
    for (var i = 0; i < 80; i++) {
      try {
        final req = await client.getUrl(
          Uri.parse('$baseUrl/kernel/runtime/local-clients'),
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

  final state = pipeline['state'];
  if (state is Map<String, dynamic>) {
    final lastDispatch = state['last_dispatch'];
    if (lastDispatch is Map<String, dynamic>) {
      final output = lastDispatch['output'];
      if (output is String && output.trim().isNotEmpty) {
        final parsed = _extractAgentMessage(output);
        return parsed ?? output.trim();
      }
      final stderr = lastDispatch['stderr'];
      if (stderr is String && stderr.trim().isNotEmpty) {
        return stderr.trim();
      }
    }
  }
  return null;
}

String? _extractAgentMessage(String raw) {
  String? latest;
  for (final line in const LineSplitter().convert(raw)) {
    final trimmed = line.trim();
    if (!trimmed.startsWith('{')) {
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
      final type = item['type']?.toString();
      if (type == 'agent_message') {
        final text = item['text']?.toString().trim();
        if (text != null && text.isNotEmpty) {
          latest = text;
        }
      } else if (type == 'command_execution') {
        final text = item['aggregated_output']?.toString().trim();
        if (text != null && text.isNotEmpty) {
          latest = text;
        }
      }
    } catch (_) {
      // Ignore non-JSON lines.
    }
  }
  return latest;
}

String _preview(String text) {
  const max = 240;
  final value = text.replaceAll('\n', r'\n');
  if (value.length <= max) {
    return value;
  }
  return '${value.substring(0, max)}...(truncated)';
}
