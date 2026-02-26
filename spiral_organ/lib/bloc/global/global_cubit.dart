import 'package:flutter/foundation.dart';
import 'package:flutter_bloc/flutter_bloc.dart';

import 'global_state.dart';

class GlobalCubit extends Cubit<GlobalState> {
  GlobalCubit() : super(const GlobalState());

  void appendLog(String line) {
    final timestamp = DateTime.now().toIso8601String();
    final entry = '[$timestamp] $line';
    debugPrint('spiral_organ/global: $entry');
    final next = List<String>.from(state.logs)..add(entry);
    if (next.length > 200) {
      next.removeRange(0, next.length - 200);
    }
    emit(state.copyWith(logs: next));
  }

  void clearLogs() {
    emit(state.copyWith(logs: const <String>[]));
  }
}
