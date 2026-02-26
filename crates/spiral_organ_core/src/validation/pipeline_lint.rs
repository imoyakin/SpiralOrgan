use std::collections::{HashMap, HashSet, VecDeque};

use crate::domain::{ExecutionMode, NodeKind, PipelineSpec};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintIssue {
    pub code: &'static str,
    pub severity: LintSeverity,
    pub message: String,
}

impl LintIssue {
    fn err(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: LintSeverity::Error,
            message: message.into(),
        }
    }

    fn warn(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            severity: LintSeverity::Warning,
            message: message.into(),
        }
    }
}

pub fn lint_pipeline(spec: &PipelineSpec) -> Vec<LintIssue> {
    let mut issues = Vec::new();

    if spec.api_version != "spiralorgan.pipeline/v0.1" {
        issues.push(LintIssue::err(
            "E100",
            format!("unsupported api_version: {}", spec.api_version),
        ));
    }

    let start_nodes: Vec<_> = spec
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::Start)
        .collect();
    let end_nodes: Vec<_> = spec
        .nodes
        .iter()
        .filter(|n| n.kind == NodeKind::End)
        .collect();

    if start_nodes.is_empty() {
        issues.push(LintIssue::err("E101", "missing start node"));
    }
    if end_nodes.is_empty() {
        issues.push(LintIssue::err("E102", "missing end node"));
    }
    if start_nodes.len() > 1 {
        issues.push(LintIssue::err(
            "E103",
            format!("multiple start nodes: {}", start_nodes.len()),
        ));
    }

    let mut seen = HashSet::new();
    for node in &spec.nodes {
        if !seen.insert(node.id.clone()) {
            issues.push(LintIssue::err(
                "E104",
                format!("duplicate node id: {}", node.id),
            ));
        }
    }

    let node_ids: HashSet<_> = spec.nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in &spec.edges {
        if !node_ids.contains(edge.from.as_str()) || !node_ids.contains(edge.to.as_str()) {
            issues.push(LintIssue::err(
                "E105",
                format!("edge references missing node: {} -> {}", edge.from, edge.to),
            ));
        }
    }

    let mut degree: HashMap<&str, usize> = HashMap::new();
    for node in &spec.nodes {
        degree.insert(node.id.as_str(), 0);
    }
    for edge in &spec.edges {
        if let Some(v) = degree.get_mut(edge.from.as_str()) {
            *v += 1;
        }
        if let Some(v) = degree.get_mut(edge.to.as_str()) {
            *v += 1;
        }
    }
    for node in &spec.nodes {
        if degree.get(node.id.as_str()) == Some(&0) {
            issues.push(LintIssue::err(
                "E106",
                format!("isolated node: {}", node.id),
            ));
        }
    }

    if !start_nodes.is_empty() && !end_nodes.is_empty() {
        let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
        for edge in &spec.edges {
            adjacency
                .entry(edge.from.as_str())
                .or_default()
                .push(edge.to.as_str());
        }
        let start_id = start_nodes[0].id.as_str();
        let end_set: HashSet<_> = end_nodes.iter().map(|n| n.id.as_str()).collect();
        let mut q = VecDeque::from([start_id]);
        let mut visited = HashSet::new();
        while let Some(cur) = q.pop_front() {
            if !visited.insert(cur) {
                continue;
            }
            if end_set.contains(cur) {
                break;
            }
            if let Some(nexts) = adjacency.get(cur) {
                for next in nexts {
                    q.push_back(next);
                }
            }
        }
        let can_reach_end = visited.iter().any(|n| end_set.contains(n));
        if !can_reach_end {
            issues.push(LintIssue::err(
                "E108",
                "no path from start node to any end node",
            ));
        }
    }

    let parallel_count = spec
        .nodes
        .iter()
        .filter(|n| n.execution_mode == ExecutionMode::Parallel)
        .count();
    if parallel_count > spec.defaults.max_parallel_nodes as usize {
        issues.push(LintIssue::err(
            "E109",
            format!(
                "parallel nodes {} exceed max_parallel_nodes {}",
                parallel_count, spec.defaults.max_parallel_nodes
            ),
        ));
    }

    let risky_tools = [
        "shell_exec",
        "browser_remote_control",
        "file_write_outside_workspace",
    ];
    for node in &spec.nodes {
        if node.execution_mode == ExecutionMode::Parallel && !node.approval_required {
            let has_risky = node
                .allowed_tools
                .iter()
                .any(|tool| risky_tools.contains(&tool.as_str()));
            if has_risky {
                issues.push(LintIssue::err(
                    "E110",
                    format!(
                        "parallel node {} contains risky tools but approval_required=false",
                        node.id
                    ),
                ));
            }
        }
        if node.allowed_tools.iter().any(|t| t == "*") {
            issues.push(LintIssue::warn(
                "W202",
                format!("node {} uses wildcard tool allowlist", node.id),
            ));
        }
    }

    issues
}
