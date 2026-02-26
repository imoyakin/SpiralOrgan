use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

use crate::local_client;
use crate::runtime_config;

use super::{
    AppState, EventSubscription, SandboxAction, SandboxApprovalMode, SandboxOperationKind,
    TaskOutputStreamContext,
};

const PROVIDER_AGENT_MAX_TURNS: usize = 18;
const SKILL_CONTENT_MAX_CHARS: usize = 18_000;
const STREAM_TEXT_MAX_CHARS: usize = 4_000;

#[derive(Debug, Clone)]
struct OpenAiToolCall {
    id: String,
    name: String,
    arguments: String,
}

pub(super) async fn dispatch_openai_like_provider_agent(
    context: Option<TaskOutputStreamContext>,
    provider: runtime_config::ProviderConfig,
    prompt: String,
    stream_tx: Option<mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
) -> Result<String, String> {
    let system_prompt = build_system_prompt(context.as_ref(), &provider)?;
    let tools = if context.is_some() {
        Some(build_openai_tools())
    } else {
        None
    };

    let mut messages = vec![
        json!({"role": "system", "content": system_prompt}),
        json!({"role": "user", "content": prompt}),
    ];

    let mut subscription = context.as_ref().map(|ctx| {
        ctx.state
            .subscribe_events(Some(ctx.session_id.clone()), Some(ctx.task_id.clone()))
    });

    let mut stream_sequence = 0_u64;
    for _turn in 0..PROVIDER_AGENT_MAX_TURNS {
        let (assistant_message, content, tool_calls) =
            openai_chat_completions(provider.clone(), messages.clone(), tools.clone()).await?;
        messages.push(assistant_message);

        if !content.trim().is_empty() {
            stream_sequence = stream_agent_message(stream_tx.as_ref(), stream_sequence, &content);
        }

        if tool_calls.is_empty() {
            return Ok(content.trim().to_string());
        }

        let Some(ctx) = context.as_ref() else {
            return Err("provider tool calls require task context".to_string());
        };
        let Some(sub) = subscription.as_mut() else {
            return Err("provider tool calls require event subscription".to_string());
        };

        for call in tool_calls {
            let tool_output =
                execute_tool_call(ctx, sub, &call, stream_tx.as_ref(), &mut stream_sequence)
                    .await?;
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "content": tool_output,
            }));
        }
    }

    Err(format!(
        "provider agent exceeded max turns ({PROVIDER_AGENT_MAX_TURNS})"
    ))
}

pub(super) async fn dispatch_anthropic_provider_agent(
    context: Option<TaskOutputStreamContext>,
    provider: runtime_config::ProviderConfig,
    prompt: String,
    stream_tx: Option<mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
) -> Result<String, String> {
    let system_prompt = build_system_prompt(context.as_ref(), &provider)?;
    let tools = if context.is_some() {
        Some(build_anthropic_tools())
    } else {
        None
    };

    let mut messages = vec![json!({
        "role": "user",
        "content": [{
            "type": "text",
            "text": prompt,
        }]
    })];

    let mut subscription = context.as_ref().map(|ctx| {
        ctx.state
            .subscribe_events(Some(ctx.session_id.clone()), Some(ctx.task_id.clone()))
    });

    let mut stream_sequence = 0_u64;
    for _turn in 0..PROVIDER_AGENT_MAX_TURNS {
        let (assistant_message, content, tool_calls) = anthropic_messages(
            provider.clone(),
            system_prompt.clone(),
            messages.clone(),
            tools.clone(),
        )
        .await?;
        messages.push(assistant_message);

        if !content.trim().is_empty() {
            stream_sequence = stream_agent_message(stream_tx.as_ref(), stream_sequence, &content);
        }

        if tool_calls.is_empty() {
            return Ok(content.trim().to_string());
        }

        let Some(ctx) = context.as_ref() else {
            return Err("provider tool calls require task context".to_string());
        };
        let Some(sub) = subscription.as_mut() else {
            return Err("provider tool calls require event subscription".to_string());
        };

        let mut blocks = Vec::with_capacity(tool_calls.len());
        for call in tool_calls {
            let tool_output =
                execute_tool_call(ctx, sub, &call, stream_tx.as_ref(), &mut stream_sequence)
                    .await?;
            let is_error = serde_json::from_str::<Value>(&tool_output)
                .ok()
                .and_then(|value| value.get("ok").and_then(Value::as_bool))
                .map(|ok| !ok)
                .unwrap_or(false);
            blocks.push(json!({
                "type": "tool_result",
                "tool_use_id": call.id,
                "content": tool_output,
                "is_error": is_error,
            }));
        }

        messages.push(json!({
            "role": "user",
            "content": blocks,
        }));
    }

    Err(format!(
        "provider agent exceeded max turns ({PROVIDER_AGENT_MAX_TURNS})"
    ))
}

fn build_system_prompt(
    context: Option<&TaskOutputStreamContext>,
    provider: &runtime_config::ProviderConfig,
) -> Result<String, String> {
    let mut sections = Vec::new();
    sections.push("You are SpiralOrgan's provider-backed coding agent.".to_string());
    sections.push("Work with the supplied tools to inspect and modify the workspace.".to_string());
    sections.push("Be concise; do not fabricate tool outputs.".to_string());

    if let Some(ctx) = context {
        sections.push(format!(
            "Session: {}  Task: {}",
            ctx.session_id, ctx.task_id
        ));
        if let Ok(session) = super::load_session_record(&ctx.state, &ctx.session_id) {
            sections.push(format!("Workspace root: {}", session.target));
            sections.push(
                "All filesystem paths must be relative to the workspace root; '..' traversal is forbidden."
                    .to_string(),
            );
        }

        if let Ok(skill_context) = build_skill_context(&ctx.state) {
            if !skill_context.trim().is_empty() {
                sections.push(String::new());
                sections.push(skill_context);
            }
        }
    }

    let active_agent_prompt = runtime_config::active_agent(&runtime_config::default_db_path())
        .ok()
        .flatten()
        .filter(|active| active.provider.id == provider.id)
        .map(|active| active.agent.system_prompt);
    if let Some(prompt) = active_agent_prompt {
        let trimmed = prompt.trim();
        if !trimmed.is_empty() {
            sections.push(String::new());
            sections.push("Active agent system prompt:".to_string());
            sections.push(trimmed.to_string());
        }
    }

    Ok(sections.join("\n"))
}

fn build_skill_context(state: &AppState) -> Result<String, String> {
    let skills = {
        let guard = state
            .inner
            .lock()
            .map_err(|_| "skills state mutex poisoned".to_string())?;
        let mut items = guard.skills.values().cloned().collect::<Vec<_>>();
        items.sort_by(|left, right| left.skill_id.cmp(&right.skill_id));
        items
    };

    if skills.is_empty() {
        return Ok(String::new());
    }

    let mut lines = Vec::new();
    lines.push("Installed skills:".to_string());
    for skill in &skills {
        let status = if skill.active { "active" } else { "inactive" };
        let mut summary = format!("- {}: {} ({status})", skill.skill_id, skill.name);
        if !skill.path.trim().is_empty() {
            summary.push_str(&format!(" path={}", skill.path));
        }
        let description = skill.description.trim();
        if !description.is_empty() {
            summary.push_str(&format!(" - {description}"));
        }
        lines.push(summary);
    }

    lines.push(String::new());
    lines.push("Skill activation protocol:".to_string());
    lines.push(
        "Append one directive per line in your final response if you want to change activation:"
            .to_string(),
    );
    lines.push("@skill activate <skill_id_or_skill_name>".to_string());
    lines.push("@skill deactivate <skill_id_or_skill_name>".to_string());

    let active = skills
        .iter()
        .filter(|skill| skill.active)
        .collect::<Vec<_>>();
    if !active.is_empty() {
        lines.push(String::new());
        lines.push("Active skill bodies (SKILL.md):".to_string());
        for skill in active {
            let path = skill.path.trim();
            if path.is_empty() {
                continue;
            }
            let content = match std::fs::read_to_string(path) {
                Ok(text) => text,
                Err(_) => continue,
            };
            let trimmed = content.trim();
            if trimmed.is_empty() {
                continue;
            }
            let body = if trimmed.len() > SKILL_CONTENT_MAX_CHARS {
                format!(
                    "{}\n\n...(truncated to {} chars)...",
                    &trimmed[..SKILL_CONTENT_MAX_CHARS],
                    SKILL_CONTENT_MAX_CHARS
                )
            } else {
                trimmed.to_string()
            };
            lines.push(String::new());
            lines.push(format!("--- {} ({}) ---", skill.name, path));
            lines.push(body);
        }
    }

    Ok(lines.join("\n"))
}

fn build_openai_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "sandbox_command_run",
                "description": "Run a command inside the workspace root. command must be a bare executable name (no paths). args is an array of string arguments.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string"},
                        "args": {"type": "array", "items": {"type": "string"}},
                        "timeout_ms": {"type": "integer"},
                    },
                    "required": ["command"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "sandbox_file_write",
                "description": "Write (create or overwrite) a file at path relative to workspace root.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                        "content": {"type": "string"},
                    },
                    "required": ["path", "content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "sandbox_file_delete",
                "description": "Delete a file at path relative to workspace root.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "sandbox_directory_create",
                "description": "Create a directory (and parents) at path relative to workspace root.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "sandbox_directory_delete",
                "description": "Delete a directory recursively at path relative to workspace root.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string"},
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "mcp_tools_list",
                "description": "List tools from an MCP stdio server. server_command must be allowed by policy.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "server_command": {"type": "string"},
                        "server_args": {"type": "array", "items": {"type": "string"}},
                        "timeout_ms": {"type": "integer"},
                    },
                    "required": ["server_command"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "mcp_tool_call",
                "description": "Invoke a tool on an MCP stdio server using tools/call. server_command must be allowed by policy.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "server_command": {"type": "string"},
                        "server_args": {"type": "array", "items": {"type": "string"}},
                        "tool": {"type": "string"},
                        "arguments": {"type": "object"},
                        "timeout_ms": {"type": "integer"},
                    },
                    "required": ["server_command", "tool", "arguments"]
                }
            }
        }),
    ]
}

fn build_anthropic_tools() -> Vec<Value> {
    vec![
        json!({
            "name": "sandbox_command_run",
            "description": "Run a command inside the workspace root. command must be a bare executable name (no paths). args is an array of string arguments.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "args": {"type": "array", "items": {"type": "string"}},
                    "timeout_ms": {"type": "integer"},
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "sandbox_file_write",
            "description": "Write (create or overwrite) a file at path relative to workspace root.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"},
                },
                "required": ["path", "content"]
            }
        }),
        json!({
            "name": "sandbox_file_delete",
            "description": "Delete a file at path relative to workspace root.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "sandbox_directory_create",
            "description": "Create a directory (and parents) at path relative to workspace root.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "sandbox_directory_delete",
            "description": "Delete a directory recursively at path relative to workspace root.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "path": {"type": "string"},
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "mcp_tools_list",
            "description": "List tools from an MCP stdio server. server_command must be allowed by policy.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "server_command": {"type": "string"},
                    "server_args": {"type": "array", "items": {"type": "string"}},
                    "timeout_ms": {"type": "integer"},
                },
                "required": ["server_command"]
            }
        }),
        json!({
            "name": "mcp_tool_call",
            "description": "Invoke a tool on an MCP stdio server using tools/call. server_command must be allowed by policy.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "server_command": {"type": "string"},
                    "server_args": {"type": "array", "items": {"type": "string"}},
                    "tool": {"type": "string"},
                    "arguments": {"type": "object"},
                    "timeout_ms": {"type": "integer"},
                },
                "required": ["server_command", "tool", "arguments"]
            }
        }),
    ]
}

async fn openai_chat_completions(
    provider: runtime_config::ProviderConfig,
    messages: Vec<Value>,
    tools: Option<Vec<Value>>,
) -> Result<(Value, String, Vec<OpenAiToolCall>), String> {
    let api_key = provider
        .api_key
        .clone()
        .ok_or_else(|| format!("provider api_key is required for {} kind", provider.kind))?;
    let default_base_url = match provider.kind.as_str() {
        "openrouter" => "https://openrouter.ai/api/v1",
        _ => "https://api.openai.com/v1",
    };
    let base_url = provider
        .base_url
        .clone()
        .unwrap_or_else(|| default_base_url.to_string());
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let payload = tokio::task::spawn_blocking(move || {
        let client = reqwest::blocking::Client::new();
        let mut body = json!({
            "model": provider.model,
            "messages": messages,
        });
        if let Some(tools) = tools {
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = Value::String("auto".to_string());
        }

        let response = client
            .post(endpoint)
            .bearer_auth(api_key)
            .json(&body)
            .send()
            .map_err(|err| format!("provider request failed: {err}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| "<failed to read error body>".to_string());
            return Err(format!("provider returned {}: {}", status, body));
        }

        response
            .json::<Value>()
            .map_err(|err| format!("invalid provider response json: {err}"))
    })
    .await
    .map_err(|err| format!("provider dispatch join error: {err}"))??;

    let message = payload
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .cloned()
        .ok_or_else(|| "provider response missing choices[0].message".to_string())?;

    let content = message
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let raw_tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tool_calls = raw_tool_calls
        .iter()
        .filter_map(parse_openai_tool_call)
        .collect::<Vec<_>>();

    let mut assistant_message = json!({
        "role": "assistant",
        "content": content,
    });
    if !raw_tool_calls.is_empty() {
        assistant_message["tool_calls"] = Value::Array(raw_tool_calls);
    }

    Ok((assistant_message, content, tool_calls))
}

fn parse_openai_tool_call(raw: &Value) -> Option<OpenAiToolCall> {
    let id = raw.get("id").and_then(Value::as_str)?.to_string();
    let function = raw.get("function").and_then(Value::as_object)?;
    let name = function.get("name").and_then(Value::as_str)?.to_string();
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}")
        .to_string();
    Some(OpenAiToolCall {
        id,
        name,
        arguments,
    })
}

async fn execute_tool_call(
    ctx: &TaskOutputStreamContext,
    subscription: &mut EventSubscription,
    call: &OpenAiToolCall,
    stream_tx: Option<&mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
    stream_sequence: &mut u64,
) -> Result<String, String> {
    let args = parse_tool_arguments(&call.arguments)?;
    let tool_name = call.name.as_str();

    let (action, approval_reason, request_summary) = match tool_name {
        "sandbox_command_run" => {
            let command = required_string(&args, "command")?;
            let tool_args = string_list(&args, "args");
            let timeout_ms = args.get("timeout_ms").and_then(Value::as_u64);
            let summary = if tool_args.is_empty() {
                format!("sandbox_command_run: {command}")
            } else {
                format!("sandbox_command_run: {} {}", command, tool_args.join(" "))
            };
            (
                SandboxAction {
                    operation: SandboxOperationKind::CommandRun,
                    path: None,
                    content: None,
                    command: Some(command),
                    args: tool_args,
                    timeout_ms,
                },
                "provider requested sandbox command execution".to_string(),
                Some(summary),
            )
        }
        "sandbox_file_write" => {
            let path = required_string(&args, "path")?;
            let content = required_string(&args, "content")?;
            let summary = format!("sandbox_file_write: {} ({} bytes)", path, content.len());
            (
                SandboxAction {
                    operation: SandboxOperationKind::FileWrite,
                    path: Some(path),
                    content: Some(content),
                    command: None,
                    args: Vec::new(),
                    timeout_ms: None,
                },
                "provider requested sandbox file write".to_string(),
                Some(summary),
            )
        }
        "sandbox_file_delete" => {
            let path = required_string(&args, "path")?;
            let summary = format!("sandbox_file_delete: {path}");
            (
                SandboxAction {
                    operation: SandboxOperationKind::FileDelete,
                    path: Some(path),
                    content: None,
                    command: None,
                    args: Vec::new(),
                    timeout_ms: None,
                },
                "provider requested sandbox file delete".to_string(),
                Some(summary),
            )
        }
        "sandbox_directory_create" => {
            let path = required_string(&args, "path")?;
            let summary = format!("sandbox_directory_create: {path}");
            (
                SandboxAction {
                    operation: SandboxOperationKind::DirectoryCreate,
                    path: Some(path),
                    content: None,
                    command: None,
                    args: Vec::new(),
                    timeout_ms: None,
                },
                "provider requested sandbox directory create".to_string(),
                Some(summary),
            )
        }
        "sandbox_directory_delete" => {
            let path = required_string(&args, "path")?;
            let summary = format!("sandbox_directory_delete: {path}");
            (
                SandboxAction {
                    operation: SandboxOperationKind::DirectoryDelete,
                    path: Some(path),
                    content: None,
                    command: None,
                    args: Vec::new(),
                    timeout_ms: None,
                },
                "provider requested sandbox directory delete".to_string(),
                Some(summary),
            )
        }
        "mcp_tools_list" => {
            let command = required_string(&args, "server_command")?;
            let server_args = string_list(&args, "server_args");
            let timeout_ms = args.get("timeout_ms").and_then(Value::as_u64);
            let summary = if server_args.is_empty() {
                format!("mcp_tools_list: {command}")
            } else {
                format!("mcp_tools_list: {} {}", command, server_args.join(" "))
            };
            (
                SandboxAction {
                    operation: SandboxOperationKind::McpCall,
                    path: Some("tools/list".to_string()),
                    content: Some("{}".to_string()),
                    command: Some(command),
                    args: server_args,
                    timeout_ms,
                },
                "provider requested mcp tools/list".to_string(),
                Some(summary),
            )
        }
        "mcp_tool_call" => {
            let command = required_string(&args, "server_command")?;
            let server_args = string_list(&args, "server_args");
            let tool = required_string(&args, "tool")?;
            let arguments = args
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            let timeout_ms = args.get("timeout_ms").and_then(Value::as_u64);
            let summary = if server_args.is_empty() {
                format!("mcp_tool_call: {command} tool={tool}")
            } else {
                format!(
                    "mcp_tool_call: {} {} tool={}",
                    command,
                    server_args.join(" "),
                    tool
                )
            };
            let params = json!({ "name": tool, "arguments": arguments });
            (
                SandboxAction {
                    operation: SandboxOperationKind::McpCall,
                    path: Some("tools/call".to_string()),
                    content: Some(
                        serde_json::to_string(&params)
                            .unwrap_or_else(|_| "{\"name\":\"\",\"arguments\":{}}".to_string()),
                    ),
                    command: Some(command),
                    args: server_args,
                    timeout_ms,
                },
                "provider requested mcp tools/call".to_string(),
                Some(summary),
            )
        }
        other => {
            return Ok(json!({
                "ok": false,
                "error": format!("unknown tool: {other}"),
            })
            .to_string());
        }
    };

    if let Some(summary) = request_summary.as_deref() {
        *stream_sequence = stream_agent_message(
            stream_tx,
            *stream_sequence,
            &truncate_for_stream(summary, STREAM_TEXT_MAX_CHARS),
        );
    }

    let session = super::load_session_record(&ctx.state, &ctx.session_id)
        .map_err(|err| format!("load session failed: {err:?}"))?;
    let policy = super::load_sandbox_runtime_policy();

    if matches!(policy.approval_mode, SandboxApprovalMode::AskFirst) {
        let approval_id = super::queue_sandbox_approval(
            &ctx.state,
            &ctx.session_id,
            Some(&ctx.task_id),
            action.clone(),
            approval_reason,
        )
        .map_err(|err| err.message)?;

        super::publish_kernel_event(
            &ctx.state,
            &ctx.session_id,
            Some(&ctx.task_id),
            "tool.approval_required",
            json!({
                "summary": "sandbox operation requires approval",
                "approval_id": approval_id,
                "reason": "ask-first policy requires approval before sandbox operation",
                "action": super::sandbox_action_preview(&action),
            }),
        );

        if let Some(tx) = stream_tx {
            *stream_sequence = stream_agent_message(
                Some(tx),
                *stream_sequence,
                &format!("Waiting for approval: {approval_id}"),
            );
        }

        let outcome = match wait_for_approval_result(subscription, approval_id.as_str()).await {
            Ok(result) => {
                stream_tool_result(stream_tx, stream_sequence, &action, &result);
                json!({ "ok": true, "result": result })
            }
            Err(err) => {
                *stream_sequence = stream_agent_message(
                    stream_tx,
                    *stream_sequence,
                    &truncate_for_stream(&format!("Tool failed: {err}"), STREAM_TEXT_MAX_CHARS),
                );
                json!({ "ok": false, "error": err })
            }
        };
        return Ok(outcome.to_string());
    }

    match super::execute_sandbox_action(&session, &action, &policy).await {
        Ok(output) => {
            super::publish_kernel_event(
                &ctx.state,
                &ctx.session_id,
                Some(&ctx.task_id),
                "tool.executed",
                json!({
                    "summary": "sandbox operation executed",
                    "action": super::sandbox_action_preview(&action),
                    "result": output,
                }),
            );
            stream_tool_result(stream_tx, stream_sequence, &action, &output);
            Ok(json!({ "ok": true, "result": output }).to_string())
        }
        Err(err) => {
            super::publish_kernel_event(
                &ctx.state,
                &ctx.session_id,
                Some(&ctx.task_id),
                "tool.execution_failed",
                json!({
                    "summary": "sandbox operation failed",
                    "action": super::sandbox_action_preview(&action),
                    "reason": err,
                }),
            );
            *stream_sequence = stream_agent_message(
                stream_tx,
                *stream_sequence,
                &truncate_for_stream(&format!("Tool failed: {err}"), STREAM_TEXT_MAX_CHARS),
            );
            Ok(json!({ "ok": false, "error": err }).to_string())
        }
    }
}

fn stream_tool_result(
    stream_tx: Option<&mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
    stream_sequence: &mut u64,
    action: &SandboxAction,
    output: &Value,
) {
    match action.operation {
        SandboxOperationKind::CommandRun => {
            let exit_code = output.get("exit_code").and_then(Value::as_i64);
            if let Some(code) = exit_code {
                *stream_sequence = stream_agent_message(
                    stream_tx,
                    *stream_sequence,
                    &format!("command exit_code={code}"),
                );
            }

            let stdout = output
                .get("stdout")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !stdout.is_empty() {
                *stream_sequence = stream_agent_message(
                    stream_tx,
                    *stream_sequence,
                    &truncate_for_stream(stdout, STREAM_TEXT_MAX_CHARS),
                );
            }

            let stderr = output
                .get("stderr")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if !stderr.is_empty() {
                *stream_sequence = stream_agent_message(
                    stream_tx,
                    *stream_sequence,
                    &truncate_for_stream(stderr, STREAM_TEXT_MAX_CHARS),
                );
            }
        }
        SandboxOperationKind::McpCall => {
            let result = output.get("result").cloned().unwrap_or(Value::Null);
            let rendered = truncate_for_stream(&result.to_string(), STREAM_TEXT_MAX_CHARS);
            if !rendered.trim().is_empty() {
                *stream_sequence =
                    stream_agent_message(stream_tx, *stream_sequence, rendered.as_str());
            }
        }
        _ => {}
    }
}

fn truncate_for_stream(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut chars = trimmed.chars();
    let prefix: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_none() {
        return prefix;
    }

    format!("{prefix}\n...(truncated)")
}

async fn wait_for_approval_result(
    subscription: &mut EventSubscription,
    approval_id: &str,
) -> Result<Value, String> {
    loop {
        let maybe_event = subscription.next_event(60_000).await?;
        let Some(event) = maybe_event else {
            continue;
        };
        let event_type = event
            .get("event_type")
            .and_then(Value::as_str)
            .unwrap_or("");
        let payload = event.get("payload").cloned().unwrap_or(Value::Null);
        let approval = payload
            .get("approval_id")
            .and_then(Value::as_str)
            .unwrap_or("");
        if approval != approval_id {
            continue;
        }

        match event_type {
            "tool.executed" => {
                return Ok(payload.get("result").cloned().unwrap_or(Value::Null));
            }
            "tool.execution_failed" => {
                let reason = payload
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("tool execution failed");
                return Err(reason.to_string());
            }
            "tool.approval_rejected" => return Err("tool approval rejected".to_string()),
            _ => continue,
        }
    }
}

fn parse_tool_arguments(raw: &str) -> Result<Map<String, Value>, String> {
    let decoded: Value =
        serde_json::from_str(raw).map_err(|err| format!("invalid tool arguments: {err}"))?;
    decoded
        .as_object()
        .cloned()
        .ok_or_else(|| "tool arguments must be a json object".to_string())
}

fn required_string(map: &Map<String, Value>, key: &str) -> Result<String, String> {
    let value = map
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("tool arguments missing required string: {key}"))?;
    Ok(value.to_string())
}

fn string_list(map: &Map<String, Value>, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn stream_agent_message(
    stream_tx: Option<&mpsc::UnboundedSender<local_client::LocalDispatchChunk>>,
    mut sequence: u64,
    text: &str,
) -> u64 {
    let Some(tx) = stream_tx else {
        return sequence;
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return sequence;
    }
    sequence += 1;
    let payload = json!({
        "item": {
            "type": "agent_message",
            "text": trimmed,
        }
    })
    .to_string();
    let _ = tx.send(local_client::LocalDispatchChunk {
        stream: "stdout".to_string(),
        text: payload,
        sequence,
    });
    sequence
}

async fn anthropic_messages(
    provider: runtime_config::ProviderConfig,
    system_prompt: String,
    messages: Vec<Value>,
    tools: Option<Vec<Value>>,
) -> Result<(Value, String, Vec<OpenAiToolCall>), String> {
    let api_key = provider
        .api_key
        .clone()
        .ok_or_else(|| "provider api_key is required for anthropic kind".to_string())?;
    let base_url = provider
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.anthropic.com/v1".to_string());
    let endpoint = format!("{}/messages", base_url.trim_end_matches('/'));

    let payload = tokio::task::spawn_blocking(move || {
        let client = reqwest::blocking::Client::new();
        let mut body = json!({
            "model": provider.model,
            "max_tokens": 2048,
            "system": system_prompt,
            "messages": messages,
        });
        if let Some(tools) = tools {
            body["tools"] = Value::Array(tools);
            body["tool_choice"] = json!({"type": "auto"});
        }

        let response = client
            .post(endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .map_err(|err| format!("provider request failed: {err}"))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .unwrap_or_else(|_| "<failed to read error body>".to_string());
            return Err(format!("provider returned {}: {}", status, body));
        }

        response
            .json::<Value>()
            .map_err(|err| format!("invalid provider response json: {err}"))
    })
    .await
    .map_err(|err| format!("provider dispatch join error: {err}"))??;

    let content_blocks = payload
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    for block in &content_blocks {
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "text" => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        text_parts.push(trimmed.to_string());
                    }
                }
            }
            "tool_use" => {
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "anthropic tool_use missing id".to_string())?
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "anthropic tool_use missing name".to_string())?
                    .to_string();
                let input = block
                    .get("input")
                    .cloned()
                    .unwrap_or_else(|| Value::Object(Map::new()));
                let arguments = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                tool_calls.push(OpenAiToolCall {
                    id,
                    name,
                    arguments,
                });
            }
            _ => {}
        }
    }

    let content = text_parts.join("\n");
    let assistant_message = json!({
        "role": "assistant",
        "content": content_blocks,
    });

    Ok((assistant_message, content, tool_calls))
}
