import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:hydrated_bloc/hydrated_bloc.dart';
import 'package:spiral_organ/app/app.dart';

Future<void> main() async {
  TestWidgetsFlutterBinding.ensureInitialized();
  HydratedBloc.storage = await HydratedStorage.build(
    storageDirectory: await Directory.systemTemp.createTemp(),
  );

  testWidgets('renders workspace tabs', (tester) async {
    await tester.pumpWidget(const SpiralFlutterApp());
    expect(find.text('Global Control'), findsOneWidget);
    expect(find.text('Kanban'), findsOneWidget);
    expect(find.text('Chat Home'), findsOneWidget);
  });
}
