use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchHunk {
    Add {
        path: String,
        contents: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_path: Option<String>,
        chunks: Vec<UpdateFileChunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateFileChunk {
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub change_context: Option<String>,
    pub is_end_of_file: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyPatchResult {
    pub added: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
    pub moved: Vec<MoveRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveRecord {
    pub from: String,
    pub to: String,
}

pub fn apply_patch(workspace_root: &Path, patch_text: &str) -> Result<ApplyPatchResult, String> {
    let hunks = parse_patch(patch_text)?;
    if hunks.is_empty() {
        return Err("patch did not contain any file operations".to_string());
    }

    let mut result = ApplyPatchResult::default();
    for hunk in hunks {
        match hunk {
            PatchHunk::Add { path, contents } => {
                let absolute = resolve_workspace_relative_path(workspace_root, &path)?;
                if let Some(parent) = absolute.parent() {
                    fs::create_dir_all(parent).map_err(|err| {
                        format!(
                            "failed to create parent directory {}: {err}",
                            parent.display()
                        )
                    })?;
                }
                fs::write(&absolute, contents)
                    .map_err(|err| format!("failed to write file {}: {err}", absolute.display()))?;
                result.added.push(path);
            }
            PatchHunk::Delete { path } => {
                let absolute = resolve_workspace_relative_path(workspace_root, &path)?;
                fs::remove_file(&absolute).map_err(|err| {
                    format!("failed to delete file {}: {err}", absolute.display())
                })?;
                result.deleted.push(path);
            }
            PatchHunk::Update {
                path,
                move_path,
                chunks,
            } => {
                let absolute = resolve_workspace_relative_path(workspace_root, &path)?;
                let original_content = fs::read_to_string(&absolute).map_err(|err| {
                    format!(
                        "failed to read file for update {}: {err}",
                        absolute.display()
                    )
                })?;
                let new_content =
                    derive_new_contents_from_chunks(&original_content, &path, &chunks)?;

                if let Some(move_path) = move_path {
                    let destination = resolve_workspace_relative_path(workspace_root, &move_path)?;
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent).map_err(|err| {
                            format!(
                                "failed to create parent directory {}: {err}",
                                parent.display()
                            )
                        })?;
                    }
                    fs::write(&destination, &new_content).map_err(|err| {
                        format!(
                            "failed to write moved file {}: {err}",
                            destination.display()
                        )
                    })?;
                    fs::remove_file(&absolute).map_err(|err| {
                        format!("failed to delete old file {}: {err}", absolute.display())
                    })?;

                    result.modified.push(move_path.clone());
                    result.moved.push(MoveRecord {
                        from: path,
                        to: move_path,
                    });
                } else {
                    fs::write(&absolute, &new_content).map_err(|err| {
                        format!("failed to write updated file {}: {err}", absolute.display())
                    })?;
                    result.modified.push(path);
                }
            }
        }
    }

    Ok(result)
}

fn parse_patch(patch_text: &str) -> Result<Vec<PatchHunk>, String> {
    let cleaned = patch_text.replace("\r\n", "\n");
    let lines = cleaned.split('\n').collect::<Vec<_>>();

    let begin_marker = "*** Begin Patch";
    let end_marker = "*** End Patch";

    let begin_idx = lines
        .iter()
        .position(|line| line.trim() == begin_marker)
        .ok_or_else(|| "invalid patch format: missing '*** Begin Patch' marker".to_string())?;
    let end_idx = lines
        .iter()
        .skip(begin_idx + 1)
        .position(|line| line.trim() == end_marker)
        .map(|offset| offset + begin_idx + 1)
        .ok_or_else(|| "invalid patch format: missing '*** End Patch' marker".to_string())?;

    let mut hunks = Vec::new();
    let mut i = begin_idx + 1;
    while i < end_idx {
        let line = lines[i].trim_end();
        if let Some(path) = line.strip_prefix("*** Add File:") {
            let path = path.trim().to_string();
            if path.is_empty() {
                return Err("invalid patch: add file missing path".to_string());
            }
            i += 1;
            let mut content = String::new();
            while i < end_idx && !lines[i].starts_with("***") {
                let raw = lines[i];
                let Some(stripped) = raw.strip_prefix('+') else {
                    return Err(format!(
                        "invalid patch: add file content line must start with '+': {}",
                        preview_line(raw)
                    ));
                };
                content.push_str(stripped);
                content.push('\n');
                i += 1;
            }
            if content.ends_with('\n') {
                content.pop();
            }
            hunks.push(PatchHunk::Add {
                path,
                contents: content,
            });
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Delete File:") {
            let path = path.trim().to_string();
            if path.is_empty() {
                return Err("invalid patch: delete file missing path".to_string());
            }
            hunks.push(PatchHunk::Delete { path });
            i += 1;
            continue;
        }

        if let Some(path) = line.strip_prefix("*** Update File:") {
            let path = path.trim().to_string();
            if path.is_empty() {
                return Err("invalid patch: update file missing path".to_string());
            }
            i += 1;
            let mut move_path: Option<String> = None;
            if i < end_idx {
                let move_line = lines[i].trim_end();
                if let Some(dest) = move_line.strip_prefix("*** Move to:") {
                    let dest = dest.trim().to_string();
                    if dest.is_empty() {
                        return Err(format!("invalid patch: move target missing for {path}",));
                    }
                    move_path = Some(dest);
                    i += 1;
                }
            }

            let (chunks, next_idx) = parse_update_chunks(&lines, i, end_idx, &path)?;
            hunks.push(PatchHunk::Update {
                path,
                move_path,
                chunks,
            });
            i = next_idx;
            continue;
        }

        i += 1;
    }

    Ok(hunks)
}

fn parse_update_chunks(
    lines: &[&str],
    start_idx: usize,
    end_idx: usize,
    file_path: &str,
) -> Result<(Vec<UpdateFileChunk>, usize), String> {
    let mut chunks = Vec::new();
    let mut i = start_idx;

    while i < end_idx && !lines[i].starts_with("***") {
        let line = lines[i].trim_end();
        if let Some(context) = line.strip_prefix("@@") {
            let context_line = context.trim();
            let change_context = if context_line.is_empty() {
                None
            } else {
                Some(context_line.to_string())
            };
            i += 1;

            let mut old_lines = Vec::new();
            let mut new_lines = Vec::new();
            let mut is_end_of_file = false;

            while i < end_idx {
                let raw = lines[i].trim_end();
                if raw.starts_with("@@") || raw.starts_with("***") {
                    break;
                }
                if raw == "*** End of File" {
                    is_end_of_file = true;
                    i += 1;
                    break;
                }

                let mut chars = raw.chars();
                match chars.next() {
                    Some(' ') => {
                        let content = chars.as_str().to_string();
                        old_lines.push(content.clone());
                        new_lines.push(content);
                    }
                    Some('-') => old_lines.push(chars.as_str().to_string()),
                    Some('+') => new_lines.push(chars.as_str().to_string()),
                    _ => {
                        return Err(format!(
                            "invalid patch: update line must start with ' ', '-', or '+': file={} line={}",
                            file_path,
                            preview_line(raw)
                        ));
                    }
                }
                i += 1;
            }

            chunks.push(UpdateFileChunk {
                old_lines,
                new_lines,
                change_context,
                is_end_of_file,
            });
            continue;
        }

        i += 1;
    }

    if chunks.is_empty() {
        return Err(format!(
            "invalid patch: update section for {file_path} missing @@ chunk",
        ));
    }

    Ok((chunks, i))
}

fn derive_new_contents_from_chunks(
    original_content: &str,
    file_path: &str,
    chunks: &[UpdateFileChunk],
) -> Result<String, String> {
    let mut original_lines = original_content
        .split('\n')
        .map(|line| line.to_string())
        .collect::<Vec<_>>();

    if matches!(original_lines.last(), Some(last) if last.is_empty()) {
        original_lines.pop();
    }

    let replacements = compute_replacements(&original_lines, file_path, chunks)?;
    let mut next_lines = original_lines;
    for (start_idx, old_len, new_segment) in replacements.into_iter().rev() {
        let end = start_idx.saturating_add(old_len);
        if end > next_lines.len() {
            return Err(format!(
                "patch application out of bounds for {file_path}: start={start_idx} len={old_len} file_len={}",
                next_lines.len()
            ));
        }
        next_lines.splice(start_idx..end, new_segment);
    }

    if !matches!(next_lines.last(), Some(last) if last.is_empty()) {
        next_lines.push(String::new());
    }

    Ok(next_lines.join("\n"))
}

fn compute_replacements(
    original_lines: &[String],
    file_path: &str,
    chunks: &[UpdateFileChunk],
) -> Result<Vec<(usize, usize, Vec<String>)>, String> {
    let mut replacements: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut line_index: usize = 0;

    for chunk in chunks {
        if let Some(context) = chunk
            .change_context
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let context_idx =
                seek_sequence(original_lines, &[context.to_string()], line_index, false)
                    .ok_or_else(|| {
                        format!(
                            "failed to find context line in {file_path}: {}",
                            preview_line(context)
                        )
                    })?;
            line_index = context_idx + 1;
        }

        if chunk.old_lines.is_empty() {
            replacements.push((original_lines.len(), 0, chunk.new_lines.clone()));
            continue;
        }

        let mut pattern = chunk.old_lines.clone();
        let mut new_slice = chunk.new_lines.clone();
        let mut found = seek_sequence(original_lines, &pattern, line_index, chunk.is_end_of_file);

        if found.is_none() && matches!(pattern.last(), Some(last) if last.is_empty()) {
            pattern.pop();
            if matches!(new_slice.last(), Some(last) if last.is_empty()) {
                new_slice.pop();
            }
            found = seek_sequence(original_lines, &pattern, line_index, chunk.is_end_of_file);
        }

        let Some(found) = found else {
            return Err(format!(
                "failed to find expected lines in {file_path}:\n{}",
                chunk.old_lines.join("\n")
            ));
        };
        replacements.push((found, pattern.len(), new_slice));
        line_index = found + pattern.len();
    }

    replacements.sort_by_key(|(idx, _, _)| *idx);
    Ok(replacements)
}

fn seek_sequence(
    lines: &[String],
    pattern: &[String],
    start_index: usize,
    eof: bool,
) -> Option<usize> {
    if pattern.is_empty() {
        return None;
    }

    try_match(lines, pattern, start_index, eof, |a, b| a == b)
        .or_else(|| {
            try_match(lines, pattern, start_index, eof, |a, b| {
                a.trim_end() == b.trim_end()
            })
        })
        .or_else(|| {
            try_match(lines, pattern, start_index, eof, |a, b| {
                a.trim() == b.trim()
            })
        })
        .or_else(|| {
            try_match(lines, pattern, start_index, eof, |a, b| {
                normalize_unicode(a.trim()) == normalize_unicode(b.trim())
            })
        })
}

fn try_match<F: Fn(&str, &str) -> bool>(
    lines: &[String],
    pattern: &[String],
    start_index: usize,
    eof: bool,
    compare: F,
) -> Option<usize> {
    if pattern.len() > lines.len() {
        return None;
    }

    if eof {
        let from_end = lines.len().saturating_sub(pattern.len());
        if from_end >= start_index && matches_sequence(lines, pattern, from_end, &compare) {
            return Some(from_end);
        }
    }

    let max_start = lines.len().saturating_sub(pattern.len());
    for i in start_index..=max_start {
        if matches_sequence(lines, pattern, i, &compare) {
            return Some(i);
        }
    }
    None
}

fn matches_sequence<F: Fn(&str, &str) -> bool>(
    lines: &[String],
    pattern: &[String],
    index: usize,
    compare: &F,
) -> bool {
    pattern.iter().enumerate().all(|(offset, expected)| {
        lines
            .get(index + offset)
            .map(|actual| compare(actual, expected))
            .unwrap_or(false)
    })
}

fn normalize_unicode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => out.push('\''),
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => out.push('"'),
            '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}' => {
                out.push('-')
            }
            '\u{2026}' => out.push_str("..."),
            '\u{00A0}' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn resolve_workspace_relative_path(
    workspace_root: &Path,
    raw_path: &str,
) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("path must not be empty".to_string());
    }

    let relative = Path::new(trimmed);
    if relative.is_absolute() {
        return Err("path must be relative to the workspace root".to_string());
    }

    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(format!(
                    "path must not contain parent-directory traversal: {}",
                    raw_path
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("path must be relative to the workspace root".to_string());
            }
        }
    }

    if normalized.as_os_str().is_empty() {
        return Err("path must not resolve to workspace root".to_string());
    }

    Ok(workspace_root.join(normalized))
}

fn preview_line(raw: &str) -> String {
    let trimmed = raw.trim_end_matches('\n').trim_end_matches('\r');
    if trimmed.len() <= 120 {
        return trimmed.to_string();
    }
    let prefix: String = trimmed.chars().take(120).collect();
    format!("{prefix}...(truncated)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace_root() -> PathBuf {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("spiral_patch_test_{seed}"));
        fs::create_dir_all(&root).expect("create temp workspace root");
        root
    }

    #[test]
    fn apply_patch_add_update_delete_move() {
        let root = temp_workspace_root();

        let original_path = root.join("src/hello.txt");
        fs::create_dir_all(original_path.parent().unwrap()).expect("create src dir");
        fs::write(&original_path, "hello\nworld\n").expect("write original");

        let patch = r#"*** Begin Patch
*** Add File: notes/readme.md
+Hello
+Notes
*** Update File: src/hello.txt
@@
 hello
-world
+spiral
*** Update File: src/hello.txt
*** Move to: src/greeting.txt
@@
 hello
 spiral
*** Delete File: notes/readme.md
*** End Patch
"#;

        let outcome = apply_patch(&root, patch).expect("apply patch should succeed");
        assert_eq!(outcome.added, vec!["notes/readme.md".to_string()]);
        assert_eq!(outcome.deleted, vec!["notes/readme.md".to_string()]);
        assert_eq!(
            outcome.modified,
            vec!["src/hello.txt".to_string(), "src/greeting.txt".to_string()]
        );
        assert_eq!(
            outcome.moved,
            vec![MoveRecord {
                from: "src/hello.txt".to_string(),
                to: "src/greeting.txt".to_string()
            }]
        );

        let greeting = fs::read_to_string(root.join("src/greeting.txt")).expect("read moved file");
        assert_eq!(greeting, "hello\nspiral\n");
        assert!(!root.join("src/hello.txt").exists());
        assert!(!root.join("notes/readme.md").exists());
    }
}
