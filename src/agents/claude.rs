//! Claude Code agent adapter.
//!
//! This adapter integrates with Claude Code CLI (`claude --print`).

use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crate::parser::{parse_jsonl_file, ToolCall};
use super::{Agent, ExecutionConfig, RawExecutionResult, ToolNameMapping};

/// Claude Code agent adapter.
pub struct ClaudeAdapter {
    mapping: ToolNameMapping,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        // Claude's tool names are already canonical — no mapping needed.
        // The ToolNameMapping fallback returns the original name unchanged.
        Self {
            mapping: ToolNameMapping::new(),
        }
    }
}

impl Default for ClaudeAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Agent for ClaudeAdapter {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn execute(&self, prompt: &str, config: &ExecutionConfig) -> Result<RawExecutionResult> {
        let (mut cmd, locator) = build_claude_command(prompt, config)?;

        let output = cmd.output().context("Failed to execute claude command")?;

        // Capture stdout
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stdout = if stdout.is_empty() { None } else { Some(stdout) };

        // Find the session log by its exact ID; never guess.
        let session_log_path = locator
            .wait_for(SESSION_FILE_GRACE)
            .ok_or_else(|| locator.missing_error(&output.status, &output.stderr))?;

        Ok(RawExecutionResult {
            session_log_path: Some(session_log_path),
            stdout,
            agent_context: None,
        })
    }

    fn parse_session(&self, result: &RawExecutionResult) -> Result<Vec<ToolCall>> {
        let path = result
            .session_log_path
            .as_ref()
            .context("Claude requires session log path")?;
        parse_jsonl_file(path)
    }

    fn tool_mapping(&self) -> &ToolNameMapping {
        &self.mapping
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn is_available(&self) -> bool {
        Command::new("claude")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn grade(&self, prompt: &str, model: Option<&str>) -> Result<String> {
        let mut cmd = Command::new("claude");
        cmd.arg("--print").arg(prompt).stdin(Stdio::null());

        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }

        let output = cmd.output().context("Failed to execute claude command for grading")?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();

        if stdout.trim().is_empty() {
            anyhow::bail!("Grading agent returned empty response");
        }

        Ok(stdout)
    }

}

/// How long to keep polling for the session file after Claude exits.
pub(crate) const SESSION_FILE_GRACE: Duration = Duration::from_millis(1500);

/// Get the Claude projects directory (`~/.claude/projects`).
///
/// The directory may not exist yet (e.g. Claude has never run); the session
/// locator handles that by reporting a missing session after the run.
pub(crate) fn get_claude_projects_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(".claude").join("projects"))
}

/// Resolve the working directory Claude will run in, as Claude sees it.
fn resolve_workdir(working_dir: &Option<PathBuf>) -> Result<PathBuf> {
    match working_dir {
        Some(dir) => dir
            .canonicalize()
            .context("Failed to canonicalize working directory"),
        None => std::env::current_dir().context("Failed to get current directory"),
    }
}

/// Encode a working directory the way Claude Code names its project directories:
/// every character that is not ASCII alphanumeric becomes `-`.
/// E.g. `/Users/foo/my_app.v2` becomes `-Users-foo-my-app-v2`.
pub(crate) fn encode_project_dir_name(workdir: &Path) -> String {
    workdir
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Determine the session ID for a run from the caller's extra args.
///
/// Returns `(session_id, args_to_append)`: if `extra_args` already contains
/// `--session-id <id>` (or `--session-id=<id>`) that ID is used and nothing is
/// appended; otherwise a fresh UUID v4 is generated and `--session-id <uuid>`
/// is returned for appending. Resume/continue modes are rejected because the
/// resulting session cannot be attributed reliably to this run.
pub(crate) fn resolve_session_id(extra_args: &[String]) -> Result<(String, Vec<String>)> {
    let mut existing: Option<String> = None;
    let mut iter = extra_args.iter();
    while let Some(arg) = iter.next() {
        let a = arg.as_str();
        if matches!(a, "--resume" | "-r" | "--continue" | "-c")
            || a.starts_with("--resume=")
            || a.starts_with("--continue=")
        {
            anyhow::bail!(
                "`{}` is not supported: aptitude must start a new Claude session so it \
                 can attribute the session log to this run (use --session-id instead)",
                a
            );
        }
        if a == "--session-id" {
            let id = iter
                .next()
                .context("`--session-id` in extra args requires a value")?;
            existing = Some(id.clone());
        } else if let Some(id) = a.strip_prefix("--session-id=") {
            existing = Some(id.to_string());
        }
    }

    match existing {
        Some(id) if id.is_empty() => anyhow::bail!("`--session-id` value must not be empty"),
        Some(id) => Ok((id, Vec::new())),
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            Ok((id.clone(), vec!["--session-id".to_string(), id]))
        }
    }
}

/// Build the `claude --print` command for a run, pinned to a known session ID.
///
/// Shared by the blocking adapter and the streaming orchestrator.
pub(crate) fn build_claude_command(
    prompt: &str,
    config: &ExecutionConfig,
) -> Result<(Command, SessionLocator)> {
    let (session_id, session_args) = resolve_session_id(&config.extra_args)?;
    let locator = SessionLocator {
        projects_dir: get_claude_projects_dir()?,
        workdir: resolve_workdir(&config.working_dir)?,
        session_id,
    };

    let mut cmd = Command::new("claude");
    cmd.arg("--print").arg(prompt).stdin(Stdio::null());
    if let Some(dir) = &config.working_dir {
        cmd.current_dir(dir);
    }
    cmd.args(&config.extra_args).args(&session_args);

    Ok((cmd, locator))
}

/// Locates the session log for one specific Claude session ID.
#[derive(Debug, Clone)]
pub(crate) struct SessionLocator {
    pub projects_dir: PathBuf,
    pub workdir: PathBuf,
    pub session_id: String,
}

impl SessionLocator {
    /// Look for `<session_id>.jsonl`, never any other file.
    pub fn locate(&self) -> Option<PathBuf> {
        locate_session_log(&self.projects_dir, &self.workdir, &self.session_id)
    }

    /// Poll for the session file for up to `grace`.
    pub fn wait_for(&self, grace: Duration) -> Option<PathBuf> {
        let deadline = Instant::now() + grace;
        loop {
            if let Some(path) = self.locate() {
                return Some(path);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Error for a run whose session file never appeared.
    pub fn missing_error(&self, status: &ExitStatus, stderr: &[u8]) -> anyhow::Error {
        let status = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "terminated by signal".to_string());
        let stderr = String::from_utf8_lossy(stderr);
        let stderr = stderr.trim();
        let excerpt: String = if stderr.is_empty() {
            "<no stderr>".to_string()
        } else {
            let tail: Vec<char> = stderr.chars().rev().take(500).collect();
            tail.into_iter().rev().collect()
        };
        anyhow::anyhow!(
            "Claude exited without writing session {} (exit status {}): {}",
            self.session_id,
            status,
            excerpt
        )
    }
}

/// Find the session log `<session_id>.jsonl` under `projects_dir`.
///
/// Checks `<projects_dir>/<encoded workdir>/<id>.jsonl` first, then scans each
/// project directory one level deep for `<id>.jsonl`. Subagent logs (which live
/// under `<project>/<session>/subagents/`) are never considered.
pub(crate) fn locate_session_log(
    projects_dir: &Path,
    workdir: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    let file_name = format!("{}.jsonl", session_id);

    let direct = projects_dir
        .join(encode_project_dir_name(workdir))
        .join(&file_name);
    if direct.is_file() {
        return Some(direct);
    }

    std::fs::read_dir(projects_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join(&file_name))
        .find(|p| p.is_file() && !p.to_string_lossy().contains("/subagents/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const ID: &str = "0b7c1f4e-2d5a-4c8e-9f1a-3b6d8e2c4a10";

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn encode_replaces_all_non_alphanumerics() {
        assert_eq!(
            encode_project_dir_name(Path::new("/Users/foo/my_app.v2/a-b c")),
            "-Users-foo-my-app-v2-a-b-c"
        );
    }

    #[test]
    fn found_when_project_dir_did_not_exist_before_run() {
        let projects = TempDir::new().unwrap();
        let workdir = Path::new("/tmp/fresh/workdir");
        assert!(locate_session_log(projects.path(), workdir, ID).is_none());

        // Claude creates the project dir and session file during the run.
        let proj = projects.path().join(encode_project_dir_name(workdir));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join(format!("{}.jsonl", ID)), "{}\n").unwrap();

        assert_eq!(
            locate_session_log(projects.path(), workdir, ID),
            Some(proj.join(format!("{}.jsonl", ID)))
        );
    }

    #[test]
    fn newer_decoys_are_ignored() {
        let projects = TempDir::new().unwrap();
        let workdir = Path::new("/w/proj");
        let proj = projects.path().join(encode_project_dir_name(workdir));
        let other = projects.path().join("-some-other-project");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        let target = proj.join(format!("{}.jsonl", ID));
        std::fs::write(&target, "{}\n").unwrap();
        std::thread::sleep(Duration::from_millis(20));
        // Newer, unrelated logs in the same project and in another project.
        std::fs::write(proj.join("decoy-same.jsonl"), "{}\n").unwrap();
        std::fs::write(other.join("decoy-other.jsonl"), "{}\n").unwrap();

        assert_eq!(locate_session_log(projects.path(), workdir, ID), Some(target));
    }

    #[test]
    fn found_with_full_encoding_for_underscore_and_dot() {
        let projects = TempDir::new().unwrap();
        let workdir = Path::new("/Users/me/my_project.v2");
        let proj = projects.path().join("-Users-me-my-project-v2");
        std::fs::create_dir_all(&proj).unwrap();
        let target = proj.join(format!("{}.jsonl", ID));
        std::fs::write(&target, "{}\n").unwrap();

        assert_eq!(locate_session_log(projects.path(), workdir, ID), Some(target));
    }

    #[test]
    fn found_by_scan_when_dir_name_differs() {
        let projects = TempDir::new().unwrap();
        let proj = projects.path().join("-unexpected-name");
        std::fs::create_dir_all(&proj).unwrap();
        let target = proj.join(format!("{}.jsonl", ID));
        std::fs::write(&target, "{}\n").unwrap();

        assert_eq!(
            locate_session_log(projects.path(), Path::new("/w/x"), ID),
            Some(target)
        );
    }

    #[test]
    fn subagent_logs_are_ignored() {
        let projects = TempDir::new().unwrap();
        let sub = projects.path().join("-w-x").join("parent").join("subagents");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(format!("{}.jsonl", ID)), "{}\n").unwrap();

        assert!(locate_session_log(projects.path(), Path::new("/w/x"), ID).is_none());
    }

    #[test]
    fn missing_file_is_an_error_not_a_fallback() {
        let projects = TempDir::new().unwrap();
        let workdir = Path::new("/w/proj");
        let proj = projects.path().join(encode_project_dir_name(workdir));
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(proj.join("previous-run.jsonl"), "{}\n").unwrap();

        let locator = SessionLocator {
            projects_dir: projects.path().to_path_buf(),
            workdir: workdir.to_path_buf(),
            session_id: ID.to_string(),
        };
        assert!(locator.wait_for(Duration::from_millis(200)).is_none());

        let status = Command::new("sh").arg("-c").arg("exit 3").status().unwrap();
        let msg = locator.missing_error(&status, b"boom").to_string();
        assert!(msg.contains(ID), "{msg}");
        assert!(msg.contains("exit status 3"), "{msg}");
        assert!(msg.contains("boom"), "{msg}");
    }

    #[test]
    fn generates_session_id_when_absent() {
        let (id, extra) = resolve_session_id(&args(&["--model", "sonnet"])).unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        assert_eq!(extra, args(&["--session-id", &id]));

        let (id2, _) = resolve_session_id(&[]).unwrap();
        assert_ne!(id, id2);
    }

    #[test]
    fn uses_existing_session_id() {
        let (id, extra) = resolve_session_id(&args(&["--session-id", ID])).unwrap();
        assert_eq!(id, ID);
        assert!(extra.is_empty());

        let (id, extra) =
            resolve_session_id(&args(&[&format!("--session-id={}", ID)])).unwrap();
        assert_eq!(id, ID);
        assert!(extra.is_empty());

        assert!(resolve_session_id(&args(&["--session-id"])).is_err());
    }

    #[test]
    fn rejects_resume_and_continue() {
        for flag in [&["--resume"][..], &["-r", ID], &["--resume", ID], &["--continue"], &["-c"]] {
            let err = resolve_session_id(&args(flag)).unwrap_err().to_string();
            assert!(err.contains("not supported"), "{flag:?}: {err}");
        }
    }
}
