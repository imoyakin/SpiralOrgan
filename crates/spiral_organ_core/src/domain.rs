#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Owner,
    Worker,
    Designer,
    Supervisor,
    Security,
    Ops,
    ChannelAdmin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Todo,
    InProgress,
    WaitingApproval,
    Blocked,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Running,
    Paused,
    WaitingUser,
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCard {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    pub assignee: Role,
    pub depends_on: Vec<String>,
    pub acceptance_criteria: String,
}

impl TaskCard {
    pub fn new(id: impl Into<String>, title: impl Into<String>, assignee: Role) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            status: TaskStatus::Todo,
            assignee,
            depends_on: Vec::new(),
            acceptance_criteria: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSession {
    pub id: String,
    pub project_id: String,
    pub target: String,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryEntry {
    pub id: String,
    pub title: String,
    pub tags: Vec<String>,
    pub score: f32,
    pub source: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostProfile {
    pub id: String,
    pub name: String,
    pub role: Role,
    pub ghost_path: String,
    pub memory_namespace: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRecord {
    pub id: String,
    pub task_id: String,
    pub risk_level: RiskLevel,
    pub approved: bool,
    pub approved_by: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEvent {
    pub id: String,
    pub event_type: String,
    pub session_id: String,
    pub task_id: Option<String>,
    pub timestamp_ms: u64,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelResponse {
    pub output: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolRequest {
    pub task_id: String,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub success: bool,
    pub output: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineDefaults {
    pub max_iterations: u16,
    pub max_parallel_nodes: u8,
    pub cancel_mode: String,
}

impl Default for PipelineDefaults {
    fn default() -> Self {
        Self {
            max_iterations: 20,
            max_parallel_nodes: 4,
            cancel_mode: "cooperative".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Start,
    Intake,
    Recall,
    Plan,
    Dispatch,
    Execute,
    Verify,
    Review,
    Commit,
    End,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Sequential,
    Parallel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpec {
    pub id: String,
    pub kind: NodeKind,
    pub enabled: bool,
    pub role_scope: Vec<Role>,
    pub execution_mode: ExecutionMode,
    pub approval_required: bool,
    pub allowed_tools: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
    pub priority: u16,
    pub when: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    BeforeNode,
    AfterNode,
    BeforePipeline,
    AfterPipeline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpec {
    pub id: String,
    pub phase: HookPhase,
    pub target: String,
    pub action: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineSpec {
    pub api_version: String,
    pub pipeline_id: String,
    pub defaults: PipelineDefaults,
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<EdgeSpec>,
    pub hooks: Vec<HookSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeView {
    Raw,
    Patch,
    Diff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeFile {
    pub path: String,
    pub status: ChangeStatus,
    pub additions: u32,
    pub deletions: u32,
    pub task_id: String,
    pub agent_id: String,
}
