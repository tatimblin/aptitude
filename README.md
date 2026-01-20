# Agent Execution Harness

A test harness for validating AI agent (Claude Code) behavior against steering guides. This tool executes Claude with prompts and asserts on the tool calls made.

## Installation

### From crates.io

```bash
cargo install agent-execution-harness
```

### From Homebrew (coming soon)

```bash
# Once the tap is set up
brew tap tristimb/tap
brew install agent-execution-harness
```

### From source

```bash
git clone https://github.com/tatimblin/agent-execution-harness
cd agent-execution-harness
cargo build --release
```

## Usage

The harness has two main commands:

### Run tests

Execute Claude with a test file and evaluate assertions:

```bash
# Run a single test
harness run test.yaml

# Run all tests in a directory
harness run tests/

# With verbose output and custom working directory
harness run test.yaml -v -w /path/to/workdir
```

### Analyze existing sessions

Analyze a pre-existing Claude session log against test assertions:

```bash
harness analyze test.yaml session.jsonl
```

## Test File Format

Tests are defined in YAML files:

```yaml
name: "Test name"
prompt: "The prompt to send to Claude"
assertions:
  - tool: Read
    called: true
    params:
      file_path: "*.txt"  # glob pattern
  - tool: Bash
    called: false
  - tool: Write
    called_after: Read
```

### Assertion Types

- `called: true/false` - Whether a tool was called
- `params` - Parameter matching (supports glob patterns, regex, or exact match)
- `called_after` - Ordering assertions (tool A must be called after tool B)

## Development

```bash
# Build the project
cargo build

# Run tests
cargo test

# Run a specific test
cargo test test_name
```

## Release Process

This project uses automated scripts for releasing to both crates.io and Homebrew:

### Create a new release

```bash
# Bump version and release
./release.sh patch  # 0.1.0 -> 0.1.1
./release.sh minor  # 0.1.0 -> 0.2.0
./release.sh major  # 0.1.0 -> 1.0.0
./release.sh 1.2.3  # specific version
```

This script will:
1. Update version in `Cargo.toml`
2. Create and push a git tag
3. Trigger GitHub Actions to build release binaries
4. Publish to crates.io

### Update Homebrew formula

After the GitHub release is created, update the SHA256 hashes:

```bash
./update-homebrew-shas.sh 0.1.0
```

This downloads the release binaries and updates the Homebrew formula with correct SHA256 hashes.

## Architecture

- **parser.rs** - Parses Claude Code JSONL session logs
- **assertions.rs** - Test structure and assertion evaluation
- **executor.rs** - Executes Claude and finds session logs
- **watcher.rs** - File watching for incremental parsing

## License

MIT