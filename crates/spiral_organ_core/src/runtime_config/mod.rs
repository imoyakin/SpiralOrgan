use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: i64,
    pub name: String,
    pub provider_id: i64,
    pub system_prompt: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveAgent {
    pub agent: AgentConfig,
    pub provider: ProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProvider {
    pub name: String,
    pub kind: String,
    pub model: String,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewAgent {
    pub name: String,
    pub provider_id: i64,
    pub system_prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDispatcherConfig {
    pub project_id: String,
    pub target_kind: String,
    pub target_ref: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewProjectDispatcher {
    pub project_id: String,
    pub target_kind: String,
    pub target_ref: String,
}

pub fn default_db_path() -> PathBuf {
    std::env::var("SPIRAL_ORGAN_CORE_CONFIG_DB")
        .or_else(|_| std::env::var("SPIRAL_CORE_CONFIG_DB"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".runtime/spiral_organ_core/config.db"))
}

pub fn bootstrap(path: &Path) -> Result<Vec<String>, String> {
    let conn = open_and_init(path)?;
    let mut messages = Vec::new();

    let providers = list_providers_conn(&conn)?;
    if providers.is_empty() {
        messages.push("assistant> no provider configured".to_string());
        messages.push(
            "assistant> use /provider add <name> <kind> <model> <api_key|- > [base_url]"
                .to_string(),
        );
        messages.push(
            "assistant> kinds: openai | anthropic | openrouter | google | custom | noop"
                .to_string(),
        );
        return Ok(messages);
    }

    let mut agents = list_agents_conn(&conn)?;
    if agents.is_empty() {
        let default_provider = &providers[0];
        let created_id = add_agent_conn(
            &conn,
            &NewAgent {
                name: format!("{}-default-agent", default_provider.name),
                provider_id: default_provider.id,
                system_prompt: "You are the primary coding agent. Be concise and action-oriented."
                    .to_string(),
            },
        )?;
        set_active_agent_conn(&conn, created_id)?;
        messages.push(format!(
            "assistant> created default agent id={created_id} for provider {}",
            default_provider.name
        ));
        agents = list_agents_conn(&conn)?;
    }

    if !agents.iter().any(|a| a.is_active) {
        let first_agent = &agents[0];
        set_active_agent_conn(&conn, first_agent.id)?;
        messages.push(format!(
            "assistant> activated agent id={} ({})",
            first_agent.id, first_agent.name
        ));
    }

    if let Some(active) = active_agent_conn(&conn)? {
        messages.push(format!(
            "assistant> active agent: {} (provider: {})",
            active.agent.name, active.provider.name
        ));
    }

    Ok(messages)
}

pub fn list_providers(path: &Path) -> Result<Vec<ProviderConfig>, String> {
    let conn = open_and_init(path)?;
    list_providers_conn(&conn)
}

pub fn add_provider(path: &Path, provider: &NewProvider) -> Result<i64, String> {
    let conn = open_and_init(path)?;
    add_provider_conn(&conn, provider)
}

pub fn update_provider(
    path: &Path,
    provider_id: i64,
    provider: &NewProvider,
) -> Result<(), String> {
    let conn = open_and_init(path)?;
    update_provider_conn(&conn, provider_id, provider)
}

pub fn delete_provider(path: &Path, provider_id: i64) -> Result<(), String> {
    let conn = open_and_init(path)?;
    delete_provider_conn(&conn, provider_id)
}

pub fn list_agents(path: &Path) -> Result<Vec<AgentConfig>, String> {
    let conn = open_and_init(path)?;
    list_agents_conn(&conn)
}

pub fn add_agent(path: &Path, agent: &NewAgent) -> Result<i64, String> {
    let conn = open_and_init(path)?;
    add_agent_conn(&conn, agent)
}

pub fn set_active_agent(path: &Path, agent_id: i64) -> Result<(), String> {
    let conn = open_and_init(path)?;
    set_active_agent_conn(&conn, agent_id)
}

pub fn active_agent(path: &Path) -> Result<Option<ActiveAgent>, String> {
    let conn = open_and_init(path)?;
    active_agent_conn(&conn)
}

pub fn get_project_dispatcher(
    path: &Path,
    project_id: &str,
) -> Result<Option<ProjectDispatcherConfig>, String> {
    let conn = open_and_init(path)?;
    get_project_dispatcher_conn(&conn, project_id)
}

pub fn set_project_dispatcher(path: &Path, config: &NewProjectDispatcher) -> Result<(), String> {
    let conn = open_and_init(path)?;
    set_project_dispatcher_conn(&conn, config)
}

fn open_and_init(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create db dir {}: {e}", parent.display()))?;
    }
    let conn = Connection::open(path)
        .map_err(|e| format!("failed to open sqlite {}: {e}", path.display()))?;
    init_schema(&conn)?;
    Ok(conn)
}

fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS providers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL,
  model TEXT NOT NULL,
  base_url TEXT,
  api_key TEXT,
  created_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agents (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  provider_id INTEGER NOT NULL,
  system_prompt TEXT NOT NULL,
  is_active INTEGER NOT NULL DEFAULT 0,
  created_at_ms INTEGER NOT NULL,
  FOREIGN KEY(provider_id) REFERENCES providers(id)
);

CREATE INDEX IF NOT EXISTS idx_agents_active ON agents(is_active);

CREATE TABLE IF NOT EXISTS project_dispatchers (
  project_id TEXT PRIMARY KEY,
  target_kind TEXT NOT NULL,
  target_ref TEXT NOT NULL,
  updated_at_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_project_dispatchers_kind ON project_dispatchers(target_kind);
"#,
    )
    .map_err(|e| format!("failed to initialize sqlite schema: {e}"))
}

fn list_providers_conn(conn: &Connection) -> Result<Vec<ProviderConfig>, String> {
    let mut stmt = conn
        .prepare("SELECT id, name, kind, model, base_url, api_key FROM providers ORDER BY id ASC")
        .map_err(|e| format!("failed to prepare providers query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(ProviderConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                kind: row.get(2)?,
                model: row.get(3)?,
                base_url: row.get(4)?,
                api_key: row.get(5)?,
            })
        })
        .map_err(|e| format!("failed to query providers: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read providers: {e}"))
}

fn add_provider_conn(conn: &Connection, provider: &NewProvider) -> Result<i64, String> {
    let kind = normalize_kind(&provider.kind)?;
    let name = normalize_required(&provider.name, "provider name")?;
    let model = normalize_required(&provider.model, "provider model")?;
    let base_url = normalize_optional(&provider.base_url);
    let api_key = normalize_optional(&provider.api_key);

    conn.execute(
        "INSERT INTO providers (name, kind, model, base_url, api_key, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![name, kind, model, base_url, api_key, now_ms() as i64],
    )
    .map_err(|e| format!("failed to insert provider: {e}"))?;
    Ok(conn.last_insert_rowid())
}

fn update_provider_conn(
    conn: &Connection,
    provider_id: i64,
    provider: &NewProvider,
) -> Result<(), String> {
    let kind = normalize_kind(&provider.kind)?;
    let name = normalize_required(&provider.name, "provider name")?;
    let model = normalize_required(&provider.model, "provider model")?;
    let base_url = normalize_optional(&provider.base_url);
    let api_key = normalize_optional(&provider.api_key);

    let affected = conn
        .execute(
            r#"
UPDATE providers
SET name = ?1,
    kind = ?2,
    model = ?3,
    base_url = ?4,
    api_key = ?5
WHERE id = ?6
"#,
            params![name, kind, model, base_url, api_key, provider_id],
        )
        .map_err(|e| format!("failed to update provider: {e}"))?;

    if affected == 0 {
        return Err(format!("provider id {} not found", provider_id));
    }
    Ok(())
}

fn delete_provider_conn(conn: &Connection, provider_id: i64) -> Result<(), String> {
    let linked_agents: i64 = conn
        .query_row(
            "SELECT COUNT(1) FROM agents WHERE provider_id = ?1",
            params![provider_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("failed to validate provider dependencies: {e}"))?;
    if linked_agents > 0 {
        return Err(format!(
            "provider id {} is in use by {} agent(s)",
            provider_id, linked_agents
        ));
    }

    let affected = conn
        .execute("DELETE FROM providers WHERE id = ?1", params![provider_id])
        .map_err(|e| format!("failed to delete provider: {e}"))?;
    if affected == 0 {
        return Err(format!("provider id {} not found", provider_id));
    }
    Ok(())
}

fn list_agents_conn(conn: &Connection) -> Result<Vec<AgentConfig>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, name, provider_id, system_prompt, is_active
             FROM agents ORDER BY id ASC",
        )
        .map_err(|e| format!("failed to prepare agents query: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok(AgentConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                provider_id: row.get(2)?,
                system_prompt: row.get(3)?,
                is_active: row.get::<_, i64>(4)? == 1,
            })
        })
        .map_err(|e| format!("failed to query agents: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("failed to read agents: {e}"))
}

fn add_agent_conn(conn: &Connection, agent: &NewAgent) -> Result<i64, String> {
    let provider_exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM providers WHERE id = ?1",
            params![agent.provider_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("failed to validate provider: {e}"))?;
    if provider_exists.is_none() {
        return Err(format!("provider id {} not found", agent.provider_id));
    }

    conn.execute(
        "INSERT INTO agents (name, provider_id, system_prompt, is_active, created_at_ms)
         VALUES (?1, ?2, ?3, 0, ?4)",
        params![
            agent.name,
            agent.provider_id,
            agent.system_prompt,
            now_ms() as i64
        ],
    )
    .map_err(|e| format!("failed to insert agent: {e}"))?;
    Ok(conn.last_insert_rowid())
}

fn set_active_agent_conn(conn: &Connection, agent_id: i64) -> Result<(), String> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM agents WHERE id = ?1",
            params![agent_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("failed to validate agent: {e}"))?;
    if exists.is_none() {
        return Err(format!("agent id {} not found", agent_id));
    }

    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("failed to begin transaction: {e}"))?;
    tx.execute("UPDATE agents SET is_active = 0", [])
        .map_err(|e| format!("failed to clear active agent: {e}"))?;
    tx.execute(
        "UPDATE agents SET is_active = 1 WHERE id = ?1",
        params![agent_id],
    )
    .map_err(|e| format!("failed to set active agent: {e}"))?;
    tx.commit()
        .map_err(|e| format!("failed to commit active agent update: {e}"))
}

fn active_agent_conn(conn: &Connection) -> Result<Option<ActiveAgent>, String> {
    conn.query_row(
        r#"
SELECT
  a.id,
  a.name,
  a.provider_id,
  a.system_prompt,
  a.is_active,
  p.id,
  p.name,
  p.kind,
  p.model,
  p.base_url,
  p.api_key
FROM agents a
JOIN providers p ON p.id = a.provider_id
WHERE a.is_active = 1
LIMIT 1
"#,
        [],
        |row| {
            Ok(ActiveAgent {
                agent: AgentConfig {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    provider_id: row.get(2)?,
                    system_prompt: row.get(3)?,
                    is_active: row.get::<_, i64>(4)? == 1,
                },
                provider: ProviderConfig {
                    id: row.get(5)?,
                    name: row.get(6)?,
                    kind: row.get(7)?,
                    model: row.get(8)?,
                    base_url: row.get(9)?,
                    api_key: row.get(10)?,
                },
            })
        },
    )
    .optional()
    .map_err(|e| format!("failed to load active agent: {e}"))
}

fn get_project_dispatcher_conn(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<ProjectDispatcherConfig>, String> {
    conn.query_row(
        r#"
SELECT project_id, target_kind, target_ref, updated_at_ms
FROM project_dispatchers
WHERE project_id = ?1
LIMIT 1
"#,
        params![project_id],
        |row| {
            Ok(ProjectDispatcherConfig {
                project_id: row.get(0)?,
                target_kind: row.get(1)?,
                target_ref: row.get(2)?,
                updated_at_ms: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(|e| format!("failed to read project dispatcher: {e}"))
}

fn set_project_dispatcher_conn(
    conn: &Connection,
    config: &NewProjectDispatcher,
) -> Result<(), String> {
    let project_id = normalize_required(&config.project_id, "project_id")?;
    let target_kind = normalize_dispatcher_kind(&config.target_kind)?;
    let target_ref = normalize_required(&config.target_ref, "target_ref")?;
    conn.execute(
        r#"
INSERT INTO project_dispatchers (project_id, target_kind, target_ref, updated_at_ms)
VALUES (?1, ?2, ?3, ?4)
ON CONFLICT(project_id) DO UPDATE SET
  target_kind = excluded.target_kind,
  target_ref = excluded.target_ref,
  updated_at_ms = excluded.updated_at_ms
"#,
        params![project_id, target_kind, target_ref, now_ms() as i64],
    )
    .map_err(|e| format!("failed to upsert project dispatcher: {e}"))?;
    Ok(())
}

fn normalize_required(value: &str, field: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(trimmed.to_string())
}

fn normalize_optional(value: &Option<String>) -> Option<String> {
    value.as_deref().map(str::trim).and_then(|v| {
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    })
}

fn normalize_kind(kind: &str) -> Result<String, String> {
    let normalized = kind.trim().to_lowercase();
    if normalized.is_empty() {
        return Err("provider kind must not be empty".to_string());
    }
    Ok(normalized)
}

fn normalize_dispatcher_kind(kind: &str) -> Result<String, String> {
    let normalized = kind.trim().to_lowercase();
    if normalized != "provider" && normalized != "local_client" && normalized != "ssh" {
        return Err(format!(
            "target_kind must be provider, local_client, or ssh (got: {kind})"
        ));
    }
    Ok(normalized)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
