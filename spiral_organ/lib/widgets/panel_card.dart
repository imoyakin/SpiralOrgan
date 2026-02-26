import 'package:fluent_ui/fluent_ui.dart';

class PanelCard extends StatelessWidget {
  const PanelCard({
    required this.title,
    required this.child,
    this.trailing,
    super.key,
  });

  final String title;
  final Widget child;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.all(14),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              children: [
                Expanded(
                  child: Text(
                    title,
                    style: const TextStyle(fontWeight: FontWeight.w700),
                  ),
                ),
                ...?(trailing == null ? null : <Widget>[trailing!]),
              ],
            ),
            const SizedBox(height: 8),
            child,
          ],
        ),
      ),
    );
  }
}
