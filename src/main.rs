use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

use aptitude::agents::{AgentHarness, AgentType, ExecutionConfig};
use aptitude::config::Config;
use aptitude::discovery::discover_tests;
use aptitude::output::{OutputConfig, OutputFormatter};
use aptitude::parser::{parse_jsonl_file, ToolCall};
use aptitude::agents::ToolNameMapping;
use aptitude::streaming::{StreamEvent, StreamHandle};

#[cfg(feature = "yaml")]
use aptitude::yaml::{load_test, run_yaml_test, TestResult};

#[derive(Parser)]
#[command(name = "aptitude")]
#[command(about = "Test harness for AI agent steering guides", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a test file (executes an agent with the prompt and asserts on results)
    Run {
        /// Path to test YAML file or directory
        path: PathBuf,

        /// Verbose output (show tool calls as they happen)
        #[arg(short, long)]
        verbose: bool,

        /// Working directory for agent execution
        #[arg(short, long)]
        workdir: Option<PathBuf>,

        /// Agent to use (overrides test file setting)
        #[arg(short, long)]
        agent: Option<String>,

        /// Test file pattern (overrides config)
        #[arg(short, long)]
        pattern: Option<String>,

        /// Root directory for test discovery (overrides config)
        #[arg(short, long)]
        root: Option<PathBuf>,

        /// Disable recursive directory scanning
        #[arg(long)]
        no_recursive: bool,

        /// Path to config file (default: auto-discover)
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// List matched test files without running them
        #[arg(long)]
        list_tests: bool,
    },

    /// Analyze an existing session log file
    Analyze {
        /// Path to test YAML file
        test: PathBuf,

        /// Path to session JSONL file
        session: PathBuf,

        /// Agent that produced this session (for tool name normalization)
        #[arg(short, long)]
        agent: Option<String>,
    },

    /// List available agents
    Agents,

    /// Execute Claude with a prompt and display tool calls (no assertions)
    Log {
        /// The prompt to send to Claude
        prompt: String,

        /// Working directory for agent execution
        #[arg(short, long)]
        workdir: Option<PathBuf>,

        /// Agent to use (default: claude)
        #[arg(short, long)]
        agent: Option<String>,

        /// Model to use (passed to Claude via --model)
        #[arg(short, long)]
        model: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let harness = AgentHarness::new();

    match cli.command {
        Commands::Run {
            path,
            verbose,
            workdir,
            agent,
            pattern,
            root,
            no_recursive,
            config: config_path,
            list_tests,
        } => {
            let agent_type = parse_agent_type(agent.as_deref())?;

            if path.is_file() {
                // Single file mode - run directly
                run_single_test(&harness, &path, verbose, workdir.as_deref(), agent_type)?;
            } else {
                // Directory mode - use discovery
                let (config, config_dir) = load_or_discover_config(&path, config_path.as_deref());
                let config = config.with_overrides(pattern, root, no_recursive);
                let search_root = config.search_dir(&path, config_dir.as_deref());

                if list_tests {
                    list_discovered_tests(&search_root, &config)?;
                } else {
                    run_tests_in_directory(
                        &harness,
                        &search_root,
                        verbose,
                        workdir.as_deref(),
                        agent_type,
                        &config,
                    )?;
                }
            }
        }
        Commands::Analyze { test, session, agent } => {
            let agent_type = parse_agent_type(agent.as_deref())?;
            analyze_session(&harness, &test, &session, agent_type)?;
        }
        Commands::Agents => {
            list_agents(&harness);
        }
        Commands::Log {
            prompt,
            workdir,
            agent,
            model,
        } => {
            let agent_type = parse_agent_type(agent.as_deref())?;
            log_command(&harness, &prompt, workdir.as_deref(), agent_type, model.as_deref())?;
        }
    }

    Ok(())
}

fn parse_agent_type(agent: Option<&str>) -> Result<Option<AgentType>> {
    match agent {
        None => Ok(None),
        Some(name) => AgentType::from_str(name)
            .ok_or_else(|| anyhow::anyhow!("Unknown agent: '{}'. Use 'aptitude agents' to list available agents.", name))
            .map(Some),
    }
}

/// Load config from explicit path or discover from directory.
fn load_or_discover_config(
    start_dir: &Path,
    explicit_path: Option<&Path>,
) -> (Config, Option<PathBuf>) {
    match explicit_path {
        Some(path) => Config::load(path)
            .map(|(c, d)| (c, Some(d)))
            .unwrap_or_else(|_| (Config::default(), None)),
        None => Config::discover(start_dir)
            .map(|(c, d)| (c, Some(d)))
            .unwrap_or_else(|| (Config::default(), None)),
    }
}

/// List discovered test files without running them.
fn list_discovered_tests(dir: &Path, config: &Config) -> Result<()> {
    let tests = discover_tests(dir, config)?;

    println!();
    println!("Discovered {} test file(s):", tests.len());
    println!();

    for path in &tests {
        println!("  {}", path.display());
    }

    println!();
    Ok(())
}

fn list_agents(harness: &AgentHarness) {
    println!();
    println!("Registered agents:");
    for name in harness.registered_agents() {
        let available = harness
            .get_agent(AgentType::from_str(name).unwrap())
            .map(|a| a.is_available())
            .unwrap_or(false);
        let status = if available { "\x1b[32mavailable\x1b[0m" } else { "\x1b[31mnot found\x1b[0m" };
        println!("  - {} ({})", name, status);
    }
    println!();
}

/// Print test results and summary. Returns true if all passed.
fn print_results(results: &[(String, TestResult)]) -> bool {
    let mut passed = 0;
    let mut failed = 0;

    for (description, result) in results {
        match result {
            TestResult::Pass => {
                println!("  \x1b[32m✓\x1b[0m {}", description);
                passed += 1;
            }
            TestResult::Fail { reason } => {
                println!("  \x1b[31m✗\x1b[0m {}", description);
                println!("    └─ {}", reason);
                failed += 1;
            }
        }
    }

    let all_passed = failed == 0;
    println!();
    if all_passed {
        println!("\x1b[32mResults: {}/{} passed\x1b[0m", passed, passed + failed);
    } else {
        println!("\x1b[31mResults: {}/{} passed\x1b[0m", passed, passed + failed);
    }
    all_passed
}

/// Drain all events from a stream handle, normalizing tool names and printing live.
///
/// Returns the collected (normalized) tool calls.
fn drain_stream_events(
    handle: &StreamHandle,
    mapping: &ToolNameMapping,
    formatter: &OutputFormatter,
    verbose: bool,
) -> Vec<ToolCall> {
    let mut tool_calls = Vec::new();
    for event in &handle.receiver {
        match event {
            StreamEvent::ToolCall(tc) => {
                let normalized = ToolCall {
                    name: mapping.to_canonical(&tc.name),
                    params: tc.params.clone(),
                    timestamp: tc.timestamp.clone(),
                };
                println!("  {}", formatter.format_tool_call(&normalized));
                tool_calls.push(normalized);
            }
            StreamEvent::SessionDetected(path) => {
                let formatted = formatter.format_session_path(&path, verbose);
                println!("  \x1b[2m[session: {}]\x1b[0m", formatted);
            }
            StreamEvent::Error(msg) => {
                eprintln!("  \x1b[33m[stream error: {}]\x1b[0m", msg);
            }
        }
    }
    tool_calls
}

/// Resolve the tool name mapping for a given agent type.
fn get_mapping(harness: &AgentHarness, agent_type: Option<AgentType>) -> Result<ToolNameMapping> {
    let resolved = agent_type.unwrap_or(AgentType::Claude);
    let agent = harness.get_agent(resolved)
        .ok_or_else(|| anyhow::anyhow!("Agent not found: {:?}", resolved))?;
    Ok(agent.tool_mapping().clone())
}

fn run_single_test(
    harness: &AgentHarness,
    test_path: &Path,
    verbose: bool,
    workdir: Option<&Path>,
    cli_agent: Option<AgentType>,
) -> Result<bool> {
    let test = load_test(test_path).context("Failed to load test file")?;

    // Determine agent: CLI flag > test file > default (claude)
    let agent_type = match cli_agent {
        Some(a) => Some(a),
        None => test.agent.as_ref().and_then(|s| AgentType::from_str(s)),
    };
    let agent_name = agent_type
        .map(|a| a.as_str())
        .unwrap_or("claude");

    println!();
    println!("Running: \"{}\"", test.name);
    println!("Prompt: \"{}\"", test.prompt);
    println!("Agent: {}", agent_name);
    println!();
    println!("Executing {}...", agent_name);
    println!();

    // Build execution config
    let mut config = ExecutionConfig::new();
    if let Some(dir) = workdir {
        config = config.with_working_dir(dir.to_path_buf());
    }

    let mapping = get_mapping(harness, agent_type)?;
    let canonical_workdir = workdir.and_then(|d| d.canonicalize().ok());
    let formatter = OutputFormatter::new(OutputConfig::verbose())
        .with_workdir(canonical_workdir.clone());

    // Execute in streaming mode
    let handle = harness.execute_streaming(agent_type, &test.prompt, config)?;

    println!("Tool calls (live):");
    println!("{}", "─".repeat(40));

    let tool_calls = drain_stream_events(&handle, &mapping, &formatter, verbose);

    println!("{}", "─".repeat(40));

    // Wait for the process to finish
    let raw_result = handle.wait()?;

    println!();
    println!("{} finished. Evaluating assertions...", agent_name);
    if let Some(log_path) = &raw_result.session_log_path {
        println!("Session log: {}", formatter.format_session_path(log_path, verbose));
    }
    println!();

    // Evaluate assertions
    let grading_agent = harness.get_agent(agent_type.unwrap_or(AgentType::Claude));
    let results = run_yaml_test(&test, &tool_calls, &raw_result.stdout, grading_agent);
    let test_passed = print_results(&results);

    // Show response if verbose or failed
    let output_config = if verbose {
        OutputConfig::verbose()
    } else {
        OutputConfig::new()
    };
    let out_formatter = OutputFormatter::new(output_config)
        .with_workdir(canonical_workdir);
    out_formatter.print_response(raw_result.stdout.as_deref(), test_passed);

    Ok(test_passed)
}

fn run_tests_in_directory(
    harness: &AgentHarness,
    dir: &Path,
    verbose: bool,
    workdir: Option<&Path>,
    cli_agent: Option<AgentType>,
    config: &Config,
) -> Result<()> {
    let test_files = discover_tests(dir, config)?;

    if test_files.is_empty() {
        println!();
        println!(
            "No test files found matching pattern '{}' in {:?}",
            config.test_pattern, dir
        );
        return Ok(());
    }

    println!();
    println!(
        "Found {} test file(s) matching '{}'",
        test_files.len(),
        config.test_pattern
    );

    let mut total_passed = 0;
    let mut total_failed = 0;

    for path in test_files {
        match run_single_test(harness, &path, verbose, workdir, cli_agent) {
            Ok(passed) => {
                if passed {
                    total_passed += 1;
                } else {
                    total_failed += 1;
                }
            }
            Err(e) => {
                println!("\x1b[31mError running {:?}: {}\x1b[0m", path, e);
                total_failed += 1;
            }
        }
        println!();
        println!("{}", "─".repeat(60));
    }

    println!();
    println!("Total: {} passed, {} failed", total_passed, total_failed);

    if total_failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn analyze_session(
    harness: &AgentHarness,
    test_path: &Path,
    session_path: &Path,
    cli_agent: Option<AgentType>,
) -> Result<()> {
    let test = load_test(test_path).context("Failed to load test file")?;

    // Determine agent for tool name normalization
    let agent_type = cli_agent
        .or_else(|| test.agent.as_ref().and_then(|s| AgentType::from_str(s)))
        .unwrap_or(AgentType::Claude);

    let formatter = OutputFormatter::with_defaults();
    println!();
    println!("Analyzing: \"{}\"", test.name);
    println!("Session: {}", formatter.format_session_path(session_path, false));
    println!("Agent: {}", agent_type.as_str());
    println!();

    // Parse the session log
    let raw_tool_calls = parse_jsonl_file(session_path)?;

    // Normalize tool names using the agent's mapping
    let agent = harness.get_agent(agent_type)
        .ok_or_else(|| anyhow::anyhow!("Agent not found: {:?}", agent_type))?;
    let mapping = agent.tool_mapping();
    let tool_calls: Vec<_> = raw_tool_calls
        .iter()
        .map(|call| ToolCall {
            name: mapping.to_canonical(&call.name),
            params: call.params.clone(),
            timestamp: call.timestamp.clone(),
        })
        .collect();

    println!("Found {} tool calls", tool_calls.len());
    println!();

    for call in &tool_calls {
        let params_preview = call
            .params
            .get("file_path")
            .or_else(|| call.params.get("command"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let time = if call.timestamp.len() >= 19 {
            &call.timestamp[11..19]
        } else {
            "??:??:??"
        };
        println!(
            "[{}] {}: {}",
            time,
            call.name,
            params_preview
        );
    }

    println!();
    println!("Evaluating assertions...");
    println!();

    // Evaluate assertions (stdout not available in analyze mode)
    let grading_agent = harness.get_agent(agent_type);
    let results = run_yaml_test(&test, &tool_calls, &None, grading_agent);
    let all_passed = print_results(&results);

    if !all_passed {
        std::process::exit(1);
    }

    Ok(())
}

fn log_command(
    harness: &AgentHarness,
    prompt: &str,
    workdir: Option<&Path>,
    cli_agent: Option<AgentType>,
    model: Option<&str>,
) -> Result<()> {
    let agent_name = cli_agent
        .map(|a| a.as_str())
        .unwrap_or("claude");

    println!();
    println!("Executing Claude with prompt: \"{}\"", prompt);
    println!("Agent: {}", agent_name);
    println!();

    // Build execution config
    let mut config = ExecutionConfig::new();
    if let Some(dir) = workdir {
        config = config.with_working_dir(dir.to_path_buf());
    }
    if let Some(m) = model {
        config.extra_args.push("--model".to_string());
        config.extra_args.push(m.to_string());
    }

    let mapping = get_mapping(harness, cli_agent)?;
    let canonical_workdir = workdir.and_then(|d| d.canonicalize().ok());
    let formatter = OutputFormatter::new(OutputConfig::verbose())
        .with_workdir(canonical_workdir);

    println!("Tool calls (live):");
    println!("{}", "─".repeat(60));

    // Execute in streaming mode
    let handle = harness.execute_streaming(cli_agent, prompt, config)?;

    let tool_calls = drain_stream_events(&handle, &mapping, &formatter, false);

    println!("{}", "─".repeat(60));
    println!();
    println!("Total: {} tool call(s)", tool_calls.len());

    // Wait for the process to finish and get stdout
    let raw_result = handle.wait()?;

    if let Some(stdout) = &raw_result.stdout {
        if !stdout.trim().is_empty() {
            println!();
            println!("Response:");
            println!("{}", stdout);
        }
    }

    if let Some(log_path) = &raw_result.session_log_path {
        println!();
        println!("Session log: {}", formatter.format_session_path(log_path, false));
    }

    Ok(())
}
