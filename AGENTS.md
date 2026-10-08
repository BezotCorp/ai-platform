# AGENTS Instructions

BCAIP is an AI agent framework written in Rust with CLI and Electron desktop interfaces.

## Contribution Workflow

The issue is the source of truth for work intended for a pull request.

- Read the agreed design, constraints, non-goals, and verification plan before changing code.
- Keep the implementation within the issue's agreed scope.
- If implementation reveals a material design change, return to the issue before continuing.
- Every external pull request must link the issue it implements and explain how the verification plan was performed.
- Structure new issues using the matching template in `.github/ISSUE_TEMPLATE/` and set the appropriate issue type.

Maintainer-directed work, urgent security fixes, release automation, and local or exploratory changes do not require an issue.

## MCP Registry

BCAIP uses the [official MCP Registry](https://github.com/modelcontextprotocol/registry) and its `server.json` format for third-party MCP server discovery.

- Do not maintain a project-specific third-party MCP server directory.
- Direct server authors to publish their servers to the official MCP Registry.
- MCP discovery and installation should use registry-backed mechanisms.

## GitHub Communication

Write issue and pull request comments for humans, not as exhaustive work logs.

- Lead with the conclusion or action needed.
- Keep comments concise; do not repeat context already present in the thread.
- Use short paragraphs or bullets.
- Include implementation details only when they affect a decision or review.
- Prefer one clear summary over multiple incremental comments.

## Agent Loop

Agent execution is implemented through the state machine in:

```text
crates/bcaip/src/agents/state_machine/
```

Changes to agent-loop behavior must be implemented in the state-machine path.

## Setup

```bash
source bin/activate-hermit
cargo build
```

## Commands

### Build

```bash
cargo build
cargo build --release
just release-binary
```

### Test

```bash
cargo test
cargo test -p bcaip
cargo test --package bcaip --test mcp_integration_test
just record-mcp-tests
```

### Lint/Format

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
```

### UI

```bash
just run-ui
cd ui/desktop && pnpm run typecheck
cd ui/desktop && pnpm test
```

## Structure

```text
crates/       # Rust workspace members — see root Cargo.toml
ui/desktop/   # Electron desktop application
```

Some workspace crates, including those that make up the BDK, are published to crates.io and expose public APIs.

The authoritative list of BDK crates is the `release = true`, `version_group = "bdk"` package set in `release-plz.toml`, which drives the BDK release.

Run:

```bash
python3 crates/bcaip-sdk/scripts/bdk-release.py crates
```

to print the current BDK crate list.

Other crates, such as `bcaip` and `bcaip-cli`, do not provide stable public APIs. Their `pub` items are internal implementation details and may change without notice.

## Development Loop

```bash
# 1. source bin/activate-hermit
# 2. Make changes
# 3. cargo fmt
```

### Run these only if the user has asked you to build/test your changes

```text
# 1. cargo build
# 2. cargo test -p <crate>
# 3. cargo clippy --all-targets -- -D warnings
```

## Rules

- Test: Prefer the `tests/` folder, for example `crates/bcaip/tests/`.
- Error: Use `anyhow::Result`.
- Provider: Implement the `Provider` trait; see `providers/base.rs`.
- MCP: Extensions live in `crates/bcaip-mcp/`.
- UI Desktop: Use ACP SDK types or local `src/types/*` types. Do not import generated OpenAPI types/client code from `ui/desktop/src/api`.

## Code Quality

- Comments: Write self-documenting code; prefer clear names over comments.
- Comments: Never add comments that restate what code does.
- Comments: Only comment complex algorithms, non-obvious business logic, or why something is done rather than what it does.
- Simplicity: Do not make things optional that do not need to be; let the compiler enforce invariants.
- Simplicity: Booleans should default to `false`, not be optional when absence has no distinct meaning.
- Errors: Do not add error context that provides no useful information.
- Simplicity: Avoid unnecessary defensive code; trust Rust's type system.
- Logging: Clean up obsolete logs and only add logging that has a clear operational, diagnostic, error-reporting, or security purpose.

## Never

- Never recreate `ui/desktop/src/api` manually.
- Never add `@hey-api/openapi-ts` to `ui/desktop`.
- Cargo.toml: For human-authored dependency changes, use `cargo add` instead of manually editing dependency entries unless there is a specific reason not to.
- Cargo.toml: Automated dependency bump pull requests are exempt; when manual edits are necessary, keep `Cargo.lock` consistent.
- Never skip `cargo fmt`.
- Never merge without running Clippy.
- Never comment self-evident operations, getters, setters, constructors, or standard Rust idioms.
- Never overwrite a live executable in place. Unlink or atomically rename the destination first, otherwise macOS may terminate running processes with `Code Signature Invalid`.

## Entry Points

- CLI: `crates/bcaip-cli/src/main.rs`
- UI: `ui/desktop/src/main.ts`
- Agent: `crates/bcaip/src/agents/state_machine/`
