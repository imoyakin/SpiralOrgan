import 'package:equatable/equatable.dart';

class GlobalState extends Equatable {
  const GlobalState({this.logs = const <String>[]});

  final List<String> logs;

  GlobalState copyWith({List<String>? logs}) {
    return GlobalState(logs: logs ?? this.logs);
  }

  @override
  List<Object?> get props => [logs];
}
