mod tui;

use agent_role_pipeline::{
    LintSeverity as RoleLintSeverity, RolePipelineRunner, ensure_default_pipeline_file,
};
use spiral_organ_core::validation::{PolicySeverity, load_and_validate_policy};
use std::env;
use std::path::PathBuf;

fn main() {
    let mut args = env::args();
    let _ = args.next();
    let command = args.next().unwrap_or_else(|| "run".to_string());
    let arg1 = args.next();

    let result = match command.as_str() {
        "run" => tui::attach::run_once(),
        "lint" => run_lint(),
        "policy-validate" => run_policy_validate(arg1.as_deref()),
        "serve" => run_serve(arg1.as_deref()),
        "tui" => {
            tui::attach::run_tui();
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command: {other}")),
    };

    if let Err(err) = result {
        eprintln!("spiral_organ_core error: {err}");
        std::process::exit(1);
    }
}

fn run_lint() -> Result<(), String> {
    let path = role_pipeline_path();
    ensure_default_pipeline_file(&path)?;
    let runner = RolePipelineRunner::from_yaml_path(&path)?;
    let issues = runner.lint();
    if issues.is_empty() {
        println!("lint: no issues ({})", path.display());
        return Ok(());
    }

    let mut has_error = false;
    for issue in issues {
        let level = match issue.severity {
            RoleLintSeverity::Error => {
                has_error = true;
                "ERROR"
            }
            RoleLintSeverity::Warning => "WARN",
        };
        println!("[{level}] {} {}", issue.code, issue.message);
    }

    if has_error {
        Err("lint failed with errors".to_string())
    } else {
        Ok(())
    }
}

fn role_pipeline_path() -> PathBuf {
    std::env::var("SPIRAL_ORGAN_ROLE_PIPELINE_PATH")
        .or_else(|_| std::env::var("SPIRAL_ROLE_PIPELINE_PATH"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(".design/specs/pipeline.role.default.yaml"))
}

fn run_policy_validate(path: Option<&str>) -> Result<(), String> {
    let policy_path = path.unwrap_or(".design/specs/policy.default.toml");
    let issues = load_and_validate_policy(policy_path)?;
    if issues.is_empty() {
        println!("policy-validate: no issues");
        return Ok(());
    }

    let mut has_error = false;
    for issue in issues {
        let level = match issue.severity {
            PolicySeverity::Error => {
                has_error = true;
                "ERROR"
            }
            PolicySeverity::Warning => "WARN",
        };
        println!("[{level}] {} {}", issue.code, issue.message);
    }

    if has_error {
        Err("policy validation failed with errors".to_string())
    } else {
        Ok(())
    }
}

fn run_serve(addr: Option<&str>) -> Result<(), String> {
    let bind = addr.unwrap_or("127.0.0.1:8787");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to create tokio runtime: {e}"))?;
    runtime.block_on(spiral_organ_core::server::serve(bind))
}

fn print_help() {
    println!("spiral_organ_core commands:");
    println!("  run   - execute one active-agent cycle");
    println!("  lint  - run role-pipeline lint checks on default YAML");
    println!(
        "  policy-validate [path] - validate policy TOML (default: .design/specs/policy.default.toml)"
    );
    println!("  serve [addr] - start kernel/change API server (default: 127.0.0.1:8787)");
    println!("  tui   - start attach-style interactive terminal UI");
    println!("  help  - show this message");
}
