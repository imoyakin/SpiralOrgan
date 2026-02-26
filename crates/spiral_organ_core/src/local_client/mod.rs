use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant as TokioInstant, timeout};

const DEFAULT_TIMEOUT_MS: u64 = 300_000;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const PROCESS_PREVIEW_MAX_CHARS: usize = 200;

static ACTIVE_DISPATCHES: OnceLock<Mutex<HashMap<String, ActiveDispatchEntry>>> = OnceLock::new();
static NEXT_DISPATCH_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
struct LocalClientPreset {
    id: &'static str,
    name: &'static str,
    command: &'static str,
    process_names: &'static [&'static str],
    supports_dispatch: bool,
}

#[derive(Debug, Clone)]
struct ActiveDispatchEntry {
    dispatch_id: String,
    client_id: String,
    command: Vec<String>,
    cwd: Option<String>,
    prompt_preview: String,
    started_at_ms: u64,
    root_pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDispatchProcessInfo {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub cpu_percent: Option<f64>,
    pub memory_percent: Option<f64>,
    pub elapsed: Option<String>,
    pub command_name: Option<String>,
    pub command_line: Option<String>,
    pub is_root: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDispatchProcessSnapshot {
    pub dispatch_id: String,
    pub client_id: String,
    pub started_at_ms: u64,
    pub root_pid: Option<u32>,
    pub cwd: Option<String>,
    pub prompt_preview: String,
    pub command: Vec<String>,
    pub processes: Vec<LocalDispatchProcessInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalClientStatus {
    pub id: String,
    pub name: String,
    pub command: String,
    pub process_names: Vec<String>,
    pub installed: bool,
    pub path: Option<String>,
    pub running: bool,
    pub supports_dispatch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDispatchRequest {
    pub client_id: String,
    pub prompt: String,
    pub cwd: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDispatchResult {
    pub ok: bool,
    pub client_id: String,
    pub command: Vec<String>,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalDispatchChunk {
    pub stream: String,
    pub text: String,
    pub sequence: u64,
}

#[derive(Debug, Clone)]
struct RawDispatchChunk {
    stream: &'static str,
    text: String,
}

pub fn list_local_clients() -> Vec<LocalClientStatus> {
    presets()
        .iter()
        .map(|preset| {
            let path = resolve_path(preset.command);
            let installed = path.is_some();
            let running = detect_running(preset.process_names);
            LocalClientStatus {
                id: preset.id.to_string(),
                name: preset.name.to_string(),
                command: preset.command.to_string(),
                process_names: preset
                    .process_names
                    .iter()
                    .map(|name| (*name).to_string())
                    .collect(),
                installed,
                path,
                running,
                supports_dispatch: preset.supports_dispatch,
            }
        })
        .collect()
}

pub fn list_active_dispatch_processes() -> Vec<LocalDispatchProcessSnapshot> {
    let entries = {
        let guard = match dispatch_registry().lock() {
            Ok(guard) => guard,
            Err(_) => return Vec::new(),
        };
        guard.values().cloned().collect::<Vec<_>>()
    };

    let mut snapshots = entries
        .into_iter()
        .map(|entry| {
            let mut processes = entry.root_pid.map(collect_process_tree).unwrap_or_default();
            if processes.is_empty() {
                if let Some(pid) = entry.root_pid {
                    processes.push(LocalDispatchProcessInfo {
                        pid,
                        parent_pid: None,
                        cpu_percent: None,
                        memory_percent: None,
                        elapsed: None,
                        command_name: entry.command.first().cloned(),
                        command_line: Some(entry.command.join(" ")),
                        is_root: true,
                    });
                }
            }

            LocalDispatchProcessSnapshot {
                dispatch_id: entry.dispatch_id,
                client_id: entry.client_id,
                started_at_ms: entry.started_at_ms,
                root_pid: entry.root_pid,
                cwd: entry.cwd,
                prompt_preview: entry.prompt_preview,
                command: entry.command,
                processes,
            }
        })
        .collect::<Vec<_>>();
    snapshots.sort_by_key(|item| item.started_at_ms);
    snapshots
}

fn dispatch_registry() -> &'static Mutex<HashMap<String, ActiveDispatchEntry>> {
    ACTIVE_DISPATCHES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_active_dispatch(
    client_id: &str,
    command: &[String],
    cwd: Option<&str>,
    prompt: &str,
    root_pid: Option<u32>,
) -> String {
    let sequence = NEXT_DISPATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dispatch_id = format!("dispatch_{:06}", sequence);
    let entry = ActiveDispatchEntry {
        dispatch_id: dispatch_id.clone(),
        client_id: client_id.to_string(),
        command: command.to_vec(),
        cwd: cwd.map(ToString::to_string),
        prompt_preview: preview_text(prompt, PROCESS_PREVIEW_MAX_CHARS),
        started_at_ms: now_ms(),
        root_pid,
    };

    if let Ok(mut guard) = dispatch_registry().lock() {
        guard.insert(dispatch_id.clone(), entry);
    }
    dispatch_id
}

fn unregister_active_dispatch(dispatch_id: &str) {
    if let Ok(mut guard) = dispatch_registry().lock() {
        guard.remove(dispatch_id);
    }
}

pub async fn dispatch_prompt(req: &LocalDispatchRequest) -> Result<LocalDispatchResult, String> {
    dispatch_prompt_with_stream(req, None).await
}

pub async fn dispatch_prompt_with_stream(
    req: &LocalDispatchRequest,
    stream_tx: Option<mpsc::UnboundedSender<LocalDispatchChunk>>,
) -> Result<LocalDispatchResult, String> {
    let client_id = req.client_id.trim().to_lowercase();
    let prompt = req.prompt.trim();
    if prompt.is_empty() {
        return Err("prompt must not be empty".to_string());
    }

    let preset = presets()
        .into_iter()
        .find(|p| p.id == client_id)
        .ok_or_else(|| format!("unknown local client id: {}", req.client_id))?;
    if !preset.supports_dispatch {
        return Err(format!(
            "local client '{}' does not support non-interactive dispatch",
            preset.id
        ));
    }

    let binary = resolve_path(preset.command)
        .ok_or_else(|| format!("client command '{}' was not found in PATH", preset.command))?;
    let args = build_dispatch_args(preset.id, prompt)?;
    let mut command_line = vec![binary.clone()];
    command_line.extend(args.iter().cloned());

    let mut cmd = tokio::process::Command::new(&binary);
    cmd.args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.kill_on_drop(true);

    let mut effective_cwd: Option<String> = None;
    if let Some(cwd) = req.cwd.as_ref() {
        let trimmed = cwd.trim();
        if !trimmed.is_empty() {
            cmd.current_dir(trimmed);
            effective_cwd = Some(trimmed.to_string());
        }
    }

    // Use the active Go toolchain instead of inheriting a stale global override.
    cmd.env_remove("GOROOT");

    eprintln!(
        "local_client: dispatch start client={} binary={} cwd={} timeout_ms={} args={:?} prompt_preview={}",
        preset.id,
        binary,
        effective_cwd.as_deref().unwrap_or("<process cwd>"),
        req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).max(1_000),
        summarize_args_for_log(&args, prompt),
        preview_text(prompt, 240),
    );

    let wait_timeout_ms = req.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).max(1_000);
    let started = Instant::now();
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => {
            let message = format!("failed to execute {}: {err}", preset.command);
            eprintln!(
                "local_client: dispatch failed client={} duration_ms={} error={}",
                preset.id,
                started.elapsed().as_millis(),
                message
            );
            return Err(message);
        }
    };
    let child_pid = child.id();
    let dispatch_id = register_active_dispatch(
        &client_id,
        &command_line,
        effective_cwd.as_deref(),
        prompt,
        child_pid,
    );

    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut chunk_sequence: u64 = 0;
    let (chunk_tx, mut chunk_rx) = mpsc::unbounded_channel::<RawDispatchChunk>();

    let mut stdout_reader = child.stdout.take().map(|stdout_stream| {
        let tx = chunk_tx.clone();
        tokio::spawn(async move { read_stream_chunks(stdout_stream, "stdout", tx).await })
    });
    let mut stderr_reader = child.stderr.take().map(|stderr_stream| {
        let tx = chunk_tx.clone();
        tokio::spawn(async move { read_stream_chunks(stderr_stream, "stderr", tx).await })
    });
    drop(chunk_tx);

    let process_result: Result<std::process::ExitStatus, String> =
        timeout(Duration::from_millis(wait_timeout_ms), async {
            let mut wait_fut = Box::pin(child.wait());
            let mut exit_status: Option<std::process::ExitStatus> = None;
            let mut drain_deadline: Option<TokioInstant> = None;

            loop {
                if let Some(deadline) = drain_deadline {
                    if TokioInstant::now() >= deadline {
                        break;
                    }
                }

                tokio::select! {
                    wait_outcome = &mut wait_fut, if exit_status.is_none() => {
                        let status = wait_outcome
                            .map_err(|err| format!("failed to execute {}: {err}", preset.command))?;
                        exit_status = Some(status);
                        // Keep draining pipes briefly after process exit.
                        drain_deadline = Some(TokioInstant::now() + Duration::from_millis(250));
                    }
                    maybe_chunk = chunk_rx.recv() => {
                        match maybe_chunk {
                            Some(raw) => {
                                append_output_chunk(
                                    if raw.stream == "stderr" {
                                        &mut stderr
                                    } else {
                                        &mut stdout
                                    },
                                    &raw.text,
                                    MAX_OUTPUT_BYTES,
                                );

                                chunk_sequence += 1;
                                if let Some(tx) = stream_tx.as_ref() {
                                    let _ = tx.send(LocalDispatchChunk {
                                        stream: raw.stream.to_string(),
                                        text: raw.text,
                                        sequence: chunk_sequence,
                                    });
                                }
                            }
                            None => {
                                if exit_status.is_some() {
                                    break;
                                }
                            }
                        }
                    }
                    _ = async {
                        if let Some(deadline) = drain_deadline {
                            tokio::time::sleep_until(deadline).await;
                        } else {
                            std::future::pending::<()>().await;
                        }
                    }, if drain_deadline.is_some() => {
                        break;
                    }
                }
            }

            exit_status.ok_or_else(|| "local client process exited without status".to_string())
        })
        .await
        .map_err(|_| {
            format!(
                "dispatch timed out after {}ms for local client {}",
                wait_timeout_ms, preset.id
            )
        })
        .and_then(|status| status);

    if let Some(handle) = stdout_reader.take() {
        handle.abort();
        let _ = handle.await;
    }
    if let Some(handle) = stderr_reader.take() {
        handle.abort();
        let _ = handle.await;
    }

    unregister_active_dispatch(&dispatch_id);

    let status = match process_result {
        Ok(status) => status,
        Err(message) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            eprintln!(
                "local_client: dispatch failed client={} duration_ms={} error={}",
                preset.id,
                started.elapsed().as_millis(),
                message
            );
            return Err(message);
        }
    };

    let elapsed_ms = started.elapsed().as_millis() as u64;
    let exit_code = status.code();
    let ok = status.success();

    eprintln!(
        "local_client: dispatch done client={} ok={} exit_code={:?} duration_ms={} stdout_preview={} stderr_preview={}",
        preset.id,
        ok,
        exit_code,
        elapsed_ms,
        preview_text(&stdout, 400),
        preview_text(&stderr, 400),
    );

    Ok(LocalDispatchResult {
        ok,
        client_id,
        command: command_line,
        exit_code,
        stdout: truncate_utf8(stdout.into_bytes()),
        stderr: truncate_utf8(stderr.into_bytes()),
        duration_ms: elapsed_ms,
        error: if ok {
            None
        } else {
            Some(format!(
                "local client process exited with status {}",
                exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "<signal>".to_string())
            ))
        },
    })
}

async fn read_stream_chunks<R>(
    reader: R,
    stream: &'static str,
    tx: mpsc::UnboundedSender<RawDispatchChunk>,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let _ = tx.send(RawDispatchChunk { stream, text: line });
            }
            Ok(None) => break,
            Err(err) => {
                let _ = tx.send(RawDispatchChunk {
                    stream,
                    text: format!("[{} stream read error] {err}", stream),
                });
                break;
            }
        }
    }
}

fn append_output_chunk(output: &mut String, chunk: &str, max_bytes: usize) {
    if chunk.is_empty() {
        return;
    }

    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(chunk);

    if output.len() <= max_bytes {
        return;
    }

    let mut keep_from = output.len().saturating_sub(max_bytes);
    while keep_from < output.len() && !output.is_char_boundary(keep_from) {
        keep_from += 1;
    }
    output.drain(..keep_from);
}

fn presets() -> Vec<LocalClientPreset> {
    vec![
        LocalClientPreset {
            id: "codex",
            name: "Codex CLI",
            command: "codex",
            process_names: &["codex"],
            supports_dispatch: true,
        },
        LocalClientPreset {
            id: "claude",
            name: "Claude Code",
            command: "claude",
            process_names: &["claude", "node"],
            supports_dispatch: true,
        },
        LocalClientPreset {
            id: "opencode",
            name: "OpenCode",
            command: "opencode",
            process_names: &["opencode", "node", "bun"],
            supports_dispatch: false,
        },
        LocalClientPreset {
            id: "cursor",
            name: "Cursor Agent",
            command: "cursor-agent",
            process_names: &["cursor-agent"],
            supports_dispatch: false,
        },
        LocalClientPreset {
            id: "gemini",
            name: "Gemini CLI",
            command: "gemini",
            process_names: &["gemini"],
            supports_dispatch: false,
        },
        LocalClientPreset {
            id: "amp",
            name: "Sourcegraph AMP",
            command: "amp",
            process_names: &["amp"],
            supports_dispatch: false,
        },
    ]
}

fn build_dispatch_args(client_id: &str, prompt: &str) -> Result<Vec<String>, String> {
    match client_id {
        // Codex official non-interactive mode.
        "codex" => Ok(vec![
            "--dangerously-bypass-approvals-and-sandbox".to_string(),
            "exec".to_string(),
            "--json".to_string(),
            "--skip-git-repo-check".to_string(),
            prompt.to_string(),
        ]),
        // Claude Code print mode with optional JSON output.
        "claude" => Ok(vec![
            "-p".to_string(),
            prompt.to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ]),
        _ => Err(format!(
            "dispatch is not implemented for local client {}",
            client_id
        )),
    }
}

fn resolve_path(command: &str) -> Option<String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.contains(std::path::MAIN_SEPARATOR) {
        let candidate = PathBuf::from(trimmed);
        if is_executable_file(&candidate) {
            return Some(candidate.to_string_lossy().to_string());
        }
        return None;
    }

    resolve_path_in_dirs(trimmed, &resolve_search_dirs())
}

fn resolve_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();

    if let Ok(raw_path) = std::env::var("PATH") {
        for path in std::env::split_paths(&raw_path) {
            if !path.as_os_str().is_empty() {
                push_unique_dir(&mut dirs, path);
            }
        }
    }

    // Finder/GUI-launched apps on macOS often miss shell PATH additions.
    for fallback in [
        "/opt/homebrew/bin",
        "/usr/local/bin",
        "/usr/bin",
        "/bin",
        "/opt/local/bin",
    ] {
        push_unique_dir(&mut dirs, PathBuf::from(fallback));
    }

    if let Ok(home) = std::env::var("HOME") {
        for rel in [".local/bin", "bin", ".cargo/bin"] {
            push_unique_dir(&mut dirs, Path::new(&home).join(rel));
        }
    }

    dirs
}

fn push_unique_dir(dirs: &mut Vec<PathBuf>, dir: PathBuf) {
    if dirs.iter().any(|existing| existing == &dir) {
        return;
    }
    dirs.push(dir);
}

fn resolve_path_in_dirs(command: &str, dirs: &[PathBuf]) -> Option<String> {
    for dir in dirs {
        let candidate = dir.join(command);
        if is_executable_file(&candidate) {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let metadata = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return metadata.permissions().mode() & 0o111 != 0;
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn detect_running(process_names: &[&str]) -> bool {
    process_names.iter().any(|name| {
        Command::new("pgrep")
            .arg("-x")
            .arg(name)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

fn collect_process_tree(root_pid: u32) -> Vec<LocalDispatchProcessInfo> {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    let mut rows = Vec::new();
    queue.push_back(root_pid);

    while let Some(pid) = queue.pop_front() {
        if !visited.insert(pid) {
            continue;
        }

        if let Some(mut row) = inspect_process(pid) {
            row.is_root = pid == root_pid;
            rows.push(row);
        }

        for child_pid in list_child_pids(pid) {
            if !visited.contains(&child_pid) {
                queue.push_back(child_pid);
            }
        }
    }

    rows.sort_by_key(|row| (if row.is_root { 0_u8 } else { 1_u8 }, row.pid));
    rows
}

fn list_child_pids(parent_pid: u32) -> Vec<u32> {
    let output = match Command::new("pgrep")
        .arg("-P")
        .arg(parent_pid.to_string())
        .output()
    {
        Ok(output) => output,
        Err(_) => return Vec::new(),
    };

    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

fn inspect_process(pid: u32) -> Option<LocalDispatchProcessInfo> {
    let output = Command::new("ps")
        .arg("-p")
        .arg(pid.to_string())
        .arg("-o")
        .arg("pid=,ppid=,pcpu=,pmem=,etime=,comm=,args=")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let line = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|row| !row.is_empty())?
        .to_string();
    let mut parts = line.split_whitespace();

    let pid_value = parts.next()?.parse::<u32>().ok()?;
    let parent_pid = parts.next().and_then(|value| value.parse::<u32>().ok());
    let cpu_percent = parts.next().and_then(|value| value.parse::<f64>().ok());
    let memory_percent = parts.next().and_then(|value| value.parse::<f64>().ok());
    let elapsed = parts.next().map(ToString::to_string);
    let command_name = parts.next().map(ToString::to_string);
    let command_line = {
        let remain = parts.collect::<Vec<_>>().join(" ");
        if remain.trim().is_empty() {
            None
        } else {
            Some(remain)
        }
    };

    Some(LocalDispatchProcessInfo {
        pid: pid_value,
        parent_pid,
        cpu_percent,
        memory_percent,
        elapsed,
        command_name,
        command_line,
        is_root: false,
    })
}

fn truncate_utf8(bytes: Vec<u8>) -> String {
    let text = String::from_utf8_lossy(&bytes).to_string();
    if text.len() <= MAX_OUTPUT_BYTES {
        return text;
    }

    let mut truncated = text
        .chars()
        .take(MAX_OUTPUT_BYTES.saturating_sub(32))
        .collect::<String>();
    truncated.push_str("\n...[output truncated]...");
    truncated
}

fn summarize_args_for_log(args: &[String], prompt: &str) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if arg == prompt {
                format!("<prompt:{} chars>", prompt.chars().count())
            } else {
                arg.clone()
            }
        })
        .collect()
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut preview = text
        .chars()
        .take(max_chars.saturating_sub(16))
        .collect::<String>()
        .replace('\n', "\\n");
    if text.chars().count() > max_chars {
        preview.push_str("...(truncated)");
    }
    preview
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{is_executable_file, resolve_path_in_dirs};
    use std::path::PathBuf;

    #[cfg(unix)]
    fn make_executable(path: &PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path).expect("metadata should be readable");
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod should succeed");
    }

    #[cfg(not(unix))]
    fn make_executable(_path: &PathBuf) {}

    #[test]
    fn resolve_path_in_dirs_finds_executable() {
        let tmp_root = std::env::temp_dir().join(format!(
            "spiral-organ-local-client-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp_root).expect("temp dir should be created");
        let binary = tmp_root.join("fake-client");
        std::fs::write(&binary, b"#!/bin/sh\necho ok\n").expect("binary should be written");
        make_executable(&binary);

        let resolved = resolve_path_in_dirs("fake-client", &[tmp_root.clone()]);
        assert_eq!(resolved, Some(binary.to_string_lossy().to_string()));
        assert!(is_executable_file(&binary));

        let _ = std::fs::remove_file(&binary);
        let _ = std::fs::remove_dir_all(&tmp_root);
    }

    #[test]
    fn resolve_path_in_dirs_skips_non_executable() {
        let tmp_root = std::env::temp_dir().join(format!(
            "spiral-organ-local-client-test-nonexec-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&tmp_root).expect("temp dir should be created");
        let binary = tmp_root.join("fake-client");
        std::fs::write(&binary, b"echo ok\n").expect("binary should be written");

        let resolved = resolve_path_in_dirs("fake-client", &[tmp_root.clone()]);
        assert_eq!(resolved, None);
        assert!(!is_executable_file(&binary));

        let _ = std::fs::remove_file(&binary);
        let _ = std::fs::remove_dir_all(&tmp_root);
    }
}
