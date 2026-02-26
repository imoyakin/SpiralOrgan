import 'package:equatable/equatable.dart';

enum FileViewMode { raw, patch, diff }

extension FileViewModeValue on FileViewMode {
  String get value {
    switch (this) {
      case FileViewMode.raw:
        return 'raw';
      case FileViewMode.patch:
        return 'patch';
      case FileViewMode.diff:
        return 'diff';
    }
  }
}

class ChangesState extends Equatable {
  const ChangesState({
    this.files = const <Map<String, dynamic>>[],
    this.summary,
    this.fileView,
    this.selectedPath,
    this.viewMode = FileViewMode.patch,
    this.busy = false,
    this.error,
  });

  final List<Map<String, dynamic>> files;
  final Map<String, dynamic>? summary;
  final Map<String, dynamic>? fileView;
  final String? selectedPath;
  final FileViewMode viewMode;
  final bool busy;
  final String? error;

  ChangesState copyWith({
    List<Map<String, dynamic>>? files,
    Map<String, dynamic>? summary,
    Map<String, dynamic>? fileView,
    String? selectedPath,
    FileViewMode? viewMode,
    bool? busy,
    String? error,
    bool clearError = false,
    bool clearView = false,
    bool clearSummary = false,
  }) {
    return ChangesState(
      files: files ?? this.files,
      summary: clearSummary ? null : (summary ?? this.summary),
      fileView: clearView ? null : (fileView ?? this.fileView),
      selectedPath: selectedPath ?? this.selectedPath,
      viewMode: viewMode ?? this.viewMode,
      busy: busy ?? this.busy,
      error: clearError ? null : (error ?? this.error),
    );
  }

  @override
  List<Object?> get props => [
    files,
    summary,
    fileView,
    selectedPath,
    viewMode,
    busy,
    error,
  ];
}
