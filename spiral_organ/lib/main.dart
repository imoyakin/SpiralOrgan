import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';
import 'package:hydrated_bloc/hydrated_bloc.dart';
import 'package:path_provider/path_provider.dart';

import 'app/app.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  final storageDirectory = kIsWeb
      ? HydratedStorage.webStorageDirectory
      : await getApplicationDocumentsDirectory();
  final storage = await HydratedStorage.build(
    storageDirectory: storageDirectory,
  );
  HydratedBloc.storage = storage;

  runApp(const SpiralFlutterApp());
}
