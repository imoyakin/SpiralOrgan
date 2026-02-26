import 'dart:convert';

import 'package:fluent_ui/fluent_ui.dart';

class JsonViewer extends StatelessWidget {
  const JsonViewer({required this.data, this.emptyText = 'No data', super.key});

  final Object? data;
  final String emptyText;

  @override
  Widget build(BuildContext context) {
    if (data == null) {
      return Text(emptyText);
    }

    final text = const JsonEncoder.withIndent('  ').convert(data);
    return SizedBox(
      width: double.infinity,
      child: SelectableText(
        text,
        style: const TextStyle(fontFamily: 'monospace'),
      ),
    );
  }
}
