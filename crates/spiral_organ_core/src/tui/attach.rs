use iocraft::prelude::*;
use spiral_organ_core::domain::{MemoryEntry, SessionStatus, TargetSession};
use spiral_organ_core::implementations::{
    AllowAllPolicyEngine, DemoPlanner, DemoSupervisor, DemoWorker, InMemoryMemoryStore,
    NoopNotificationSink, NoopPipelineEngine, StdoutChannelAdapter, default_demo_pipeline,
};
use spiral_organ_core::runtime_config::{self, ActiveAgent, NewAgent, NewProvider};
use spiral_organ_core::runtime_provider::RuntimeProvider;
use spiral_organ_core::services::{ExecutionService, GatewayService, ResearchAlignedOrchestrator};
use spiral_organ_core::traits::{MemoryStore, Orchestrator, PipelineEngine, SessionEngine};

const HISTORY_LIMIT: usize = 64;

pub fn run_once() -> Result<(), String> {
    let output = run_cycle(true, "execute current target")?;
    println!("run result: {output}");
    Ok(())
}

fn run_cycle(notify_console: bool, input: &str) -> Result<String, String> {
    let db_path = runtime_config::default_db_path();
    let _ = runtime_config::bootstrap(&db_path)?;
    let active = runtime_config::active_agent(&db_path)?.ok_or_else(|| {
        "no active agent configured; use /provider add then /agent add".to_string()
    })?;
    run_cycle_with_agent(notify_console, input, &active)
}

fn run_cycle_with_agent(
    notify_console: bool,
    input: &str,
    active: &ActiveAgent,
) -> Result<String, String> {
    let session = demo_session(&format!(
        "agent:{} provider:{}",
        active.agent.name, active.provider.name
    ));
    let orchestrator = ResearchAlignedOrchestrator::new(
        DemoPlanner,
        DemoWorker,
        DemoSupervisor,
        NoopPipelineEngine,
        AllowAllPolicyEngine,
    );
    orchestrator
        .tick(&session)
        .map_err(|e| format!("orchestrator tick failed: {e}"))?;

    let pipeline = default_demo_pipeline();
    NoopPipelineEngine
        .lint(&pipeline)
        .map_err(|e| format!("pipeline lint failed: {e}"))?;

    let memory = InMemoryMemoryStore::default();
    memory
        .save(MemoryEntry {
            id: format!("seed-{}", active.agent.id),
            title: format!("agent {} memory seed", active.agent.name),
            tags: vec!["seed".to_string(), active.agent.name.clone()],
            score: 1.0,
            source: "run_cycle_with_agent".to_string(),
            content: "Use concise and actionable coding responses.".to_string(),
        })
        .map_err(|e| format!("memory save failed: {e}"))?;

    let provider = RuntimeProvider::from_config(&active.provider)?;
    let execution = ExecutionService::new(provider, memory);
    let enriched_input = format!(
        "SYSTEM:\n{}\n\nUSER:\n{}",
        active.agent.system_prompt, input
    );
    let output = execution
        .run_turn(&session, &enriched_input)
        .map_err(|e| format!("run_turn failed: {e}"))?;

    if notify_console {
        let gateway = GatewayService::new(StdoutChannelAdapter, NoopNotificationSink);
        gateway
            .send_message("operator", &output)
            .map_err(|e| format!("send message failed: {e}"))?;
        gateway
            .notify_idle(&session.id)
            .map_err(|e| format!("notify idle failed: {e}"))?;

        println!(
            "run completed: session={} status={:?} agent={} provider={}",
            session.id, session.status, active.agent.name, active.provider.name
        );
    }
    Ok(output)
}

#[component]
fn SpiralCoreTui(mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut system = hooks.use_context_mut::<SystemContext>();
    let mut turns = hooks.use_state(|| 0_u64);
    let mut last_run = hooks.use_state(|| String::new());
    let mut last_output = hooks.use_state(|| String::new());
    let mut should_exit = hooks.use_state(|| false);
    let mut input_line = hooks.use_state(String::new);
    let mut history = hooks.use_state(initial_history);

    hooks.use_terminal_events(move |event| {
        if let TerminalEvent::Key(KeyEvent {
            code,
            kind,
            modifiers,
            ..
        }) = event
        {
            if kind == KeyEventKind::Release {
                return;
            }
            match code {
                KeyCode::Enter => {
                    let command = input_line.read().clone();
                    let trimmed = command.trim().to_string();
                    if trimmed.is_empty() {
                        return;
                    }
                    input_line.set(String::new());

                    let mut lines = history.read().clone();
                    lines = append_prefixed_lines(lines, "you", &trimmed);
                    trim_history(&mut lines);
                    history.set(lines);

                    if trimmed == "/quit" {
                        should_exit.set(true);
                        return;
                    }
                    if trimmed == "/clear" {
                        history.set(vec!["assistant> history cleared".to_string()]);
                        return;
                    }

                    match process_attach_input(&trimmed) {
                        Ok(AttachOutcome::RanTurn(output)) => {
                            turns += 1;
                            last_run.set(format!("turn {} ok", turns.get()));
                            last_output.set(output.clone());
                            let mut out_lines = history.read().clone();
                            out_lines = append_prefixed_lines(out_lines, "assistant", &output);
                            trim_history(&mut out_lines);
                            history.set(out_lines);
                        }
                        Ok(AttachOutcome::Message(message)) => {
                            let mut out_lines = history.read().clone();
                            out_lines = append_prefixed_lines(out_lines, "assistant", &message);
                            trim_history(&mut out_lines);
                            history.set(out_lines);
                        }
                        Err(err) => {
                            last_run.set(format!("turn {} failed", turns.get()));
                            last_output.set(err.clone());
                            let mut out_lines = history.read().clone();
                            out_lines = append_prefixed_lines(
                                out_lines,
                                "assistant",
                                &format!("error: {err}"),
                            );
                            trim_history(&mut out_lines);
                            history.set(out_lines);
                        }
                    }
                }
                KeyCode::Backspace => {
                    let mut current = input_line.read().clone();
                    current.pop();
                    input_line.set(current);
                }
                KeyCode::Esc => input_line.set(String::new()),
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                    should_exit.set(true);
                }
                KeyCode::Char(c) if !c.is_control() => {
                    let mut current = input_line.read().clone();
                    current.push(c);
                    input_line.set(current);
                }
                _ => {}
            }
        }
    });

    if should_exit.get() {
        system.exit();
    }

    let history_view = {
        let lines = history.read().clone();
        lines
            .into_iter()
            .rev()
            .take(14)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n")
    };
    let prompt = input_line.read().clone();

    element! {
        View(
            border_style: BorderStyle::Round,
            border_color: Color::Blue,
            padding: 1,
            flex_direction: FlexDirection::Column,
            gap: 1,
        ) {
            Text(weight: Weight::Bold, color: Color::Cyan, content: "SpiralOrgan Core TUI Attach")
            Text(content: "Mode: provider + agent interactive loop")
            Text(content: "Enter=send, Esc=clear input, Ctrl+C=exit")
            Text(content: "Commands: /help /status /provider ... /agent ... /run /clear /quit")
            Text(content: format!("turns: {}", turns.get()))
            Text(content: format!("last_run: {}", last_run.read().as_str()))
            Text(color: Color::Green, content: format!("output: {}", last_output.read().as_str()))
            Text(content: "----- conversation -----")
            Text(content: history_view)
            Text(color: Color::Yellow, content: format!("> {}", prompt))
        }
    }
}

pub fn run_tui() {
    smol::block_on(element!(SpiralCoreTui).fullscreen()).unwrap();
}

#[derive(Debug, Clone)]
enum AttachOutcome {
    Message(String),
    RanTurn(String),
}

fn initial_history() -> Vec<String> {
    let db_path = runtime_config::default_db_path();
    let mut lines = vec!["assistant> attach mode ready".to_string()];
    match runtime_config::bootstrap(&db_path) {
        Ok(mut boot) => lines.append(&mut boot),
        Err(err) => lines.push(format!("assistant> config bootstrap failed: {err}")),
    }
    lines.push("assistant> type /help for command reference".to_string());
    lines
}

fn process_attach_input(input: &str) -> Result<AttachOutcome, String> {
    if !input.starts_with('/') {
        return run_cycle(false, input).map(AttachOutcome::RanTurn);
    }
    process_meta_command(input)
}

fn process_meta_command(command: &str) -> Result<AttachOutcome, String> {
    if command == "/help" {
        return Ok(AttachOutcome::Message(attach_help_text()));
    }
    if command == "/run" {
        return run_cycle(false, "execute current target").map(AttachOutcome::RanTurn);
    }
    if command == "/status" {
        return status_report().map(AttachOutcome::Message);
    }

    if let Some(rest) = command.strip_prefix("/provider ") {
        return handle_provider_command(rest).map(AttachOutcome::Message);
    }
    if let Some(rest) = command.strip_prefix("/agent ") {
        return handle_agent_command(rest).map(AttachOutcome::Message);
    }

    Err("unknown command; use /help".to_string())
}

fn handle_provider_command(rest: &str) -> Result<String, String> {
    let db_path = runtime_config::default_db_path();
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.is_empty() {
        return Err("usage: /provider list | /provider add ...".to_string());
    }
    match parts[0] {
        "list" => {
            let providers = runtime_config::list_providers(&db_path)?;
            if providers.is_empty() {
                return Ok("no providers configured".to_string());
            }
            let lines = providers
                .iter()
                .map(|p| format!("[{}] {} kind={} model={}", p.id, p.name, p.kind, p.model))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(lines)
        }
        "add" => {
            if parts.len() < 5 {
                return Err(
                    "usage: /provider add <name> <kind> <model> <api_key|- > [base_url]"
                        .to_string(),
                );
            }
            let provider = NewProvider {
                name: parts[1].to_string(),
                kind: parts[2].to_string(),
                model: parts[3].to_string(),
                api_key: optional_arg(parts[4]),
                base_url: parts.get(5).and_then(|s| optional_arg(s)),
            };
            let id = runtime_config::add_provider(&db_path, &provider)?;
            let boot = runtime_config::bootstrap(&db_path)?;
            let mut lines = vec![format!("provider created id={id} name={}", provider.name)];
            lines.extend(boot);
            Ok(lines.join("\n"))
        }
        _ => Err("usage: /provider list | /provider add ...".to_string()),
    }
}

fn handle_agent_command(rest: &str) -> Result<String, String> {
    let db_path = runtime_config::default_db_path();
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.is_empty() {
        return Err("usage: /agent list | /agent add ... | /agent use <id>".to_string());
    }

    match parts[0] {
        "list" => {
            let agents = runtime_config::list_agents(&db_path)?;
            if agents.is_empty() {
                return Ok("no agents configured".to_string());
            }
            let lines = agents
                .iter()
                .map(|a| {
                    let marker = if a.is_active { "*" } else { " " };
                    format!(
                        "{}[{}] {} provider_id={} prompt_len={}",
                        marker,
                        a.id,
                        a.name,
                        a.provider_id,
                        a.system_prompt.len()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(lines)
        }
        "add" => {
            let fields: Vec<&str> = rest.splitn(4, ' ').collect();
            if fields.len() < 4 {
                return Err("usage: /agent add <name> <provider_id> <system_prompt...>".to_string());
            }
            let name = fields[1].to_string();
            let provider_id: i64 = fields[2]
                .parse()
                .map_err(|_| "provider_id must be integer".to_string())?;
            let prompt = fields[3].to_string();
            if prompt.trim().is_empty() {
                return Err("system_prompt must not be empty".to_string());
            }
            let id = runtime_config::add_agent(
                &db_path,
                &NewAgent {
                    name,
                    provider_id,
                    system_prompt: prompt,
                },
            )?;
            if runtime_config::active_agent(&db_path)?.is_none() {
                runtime_config::set_active_agent(&db_path, id)?;
            }
            Ok(format!("agent created id={id}"))
        }
        "use" => {
            if parts.len() < 2 {
                return Err("usage: /agent use <agent_id>".to_string());
            }
            let agent_id: i64 = parts[1]
                .parse()
                .map_err(|_| "agent_id must be integer".to_string())?;
            runtime_config::set_active_agent(&db_path, agent_id)?;
            Ok(format!("active agent set to id={agent_id}"))
        }
        _ => Err("usage: /agent list | /agent add ... | /agent use <id>".to_string()),
    }
}

fn status_report() -> Result<String, String> {
    let db_path = runtime_config::default_db_path();
    let providers = runtime_config::list_providers(&db_path)?;
    let agents = runtime_config::list_agents(&db_path)?;
    let active = runtime_config::active_agent(&db_path)?;

    let mut lines = vec![format!(
        "db={} providers={} agents={}",
        db_path.display(),
        providers.len(),
        agents.len()
    )];
    if let Some(active) = active {
        lines.push(format!(
            "active_agent=[{}] {} provider=[{}] {}",
            active.agent.id, active.agent.name, active.provider.id, active.provider.name
        ));
    } else {
        lines.push("active_agent=<none>".to_string());
    }
    Ok(lines.join("\n"))
}

fn attach_help_text() -> String {
    [
        "/help",
        "/status",
        "/provider list",
        "/provider add <name> <kind> <model> <api_key|- > [base_url]",
        "/agent list",
        "/agent add <name> <provider_id> <system_prompt...>",
        "/agent use <agent_id>",
        "/run",
        "/clear",
        "/quit",
        "note: provider api_key is currently stored in sqlite plaintext",
    ]
    .join("\n")
}

fn optional_arg(value: &str) -> Option<String> {
    if value.trim().is_empty() || value == "-" {
        None
    } else {
        Some(value.to_string())
    }
}

fn append_prefixed_lines(mut history: Vec<String>, role: &str, content: &str) -> Vec<String> {
    for line in content.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }
        history.push(format!("{role}> {line}"));
    }
    history
}

fn trim_history(history: &mut Vec<String>) {
    if history.len() > HISTORY_LIMIT {
        let excess = history.len() - HISTORY_LIMIT;
        history.drain(0..excess);
    }
}

fn demo_session(target: &str) -> TargetSession {
    TargetSession {
        id: "session-demo".to_string(),
        project_id: "project-demo".to_string(),
        target: target.to_string(),
        status: SessionStatus::Running,
    }
}
