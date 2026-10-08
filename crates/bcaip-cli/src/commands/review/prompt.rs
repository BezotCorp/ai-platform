use bcaip::checks::{Check, DiscoveredReview};
use std::fmt::Write;

/// The default review prompt embedded in the binary.
pub const DEFAULT_REVIEW_PROMPT: &str = include_str!("default_review_prompt.md");

/// Build the full prompt sent to the main review agent.
///
/// Layout:
///
/// ```text
/// <base prompt>
///
/// ## Checks
/// <check table + per-check bodies>
///
/// ## Diff
/// ```
///
/// `base_prompt` is either the embedded [`DEFAULT_REVIEW_PROMPT`] or a
/// caller-supplied prompt loaded from `--prompt`. Findings derived from a
/// `**/.agents/REVIEW.md` file appear here as virtual `repo-rules`-prefixed
/// checks so the agent can attribute them via the `check` field on each
/// JSON finding.
pub fn build_review_prompt(
    base_prompt: &str,
    discovered: &DiscoveredReview,
    diff: &str,
    default_model: Option<&str>,
    override_model: Option<&str>,
    default_turn_limit: Option<usize>,
) -> String {
    let mut out = String::new();
    out.push_str(base_prompt.trim_end());
    out.push_str("\n\n");

    if !discovered.checks.is_empty() {
        out.push_str("## Checks\n\n");
        out.push_str("Dispatch one subagent per check below. ");
        out.push_str(
            "Use the `model`, `turn_limit`, and `tools` columns when invoking each subagent. ",
        );
        out.push_str("`tools = *` means the subagent inherits the agent's full toolset. ");
        out.push_str(
            "Set the `check` field on each finding to the check's `name` so the originating \
             rule can be identified in the output.\n\n",
        );
        out.push_str(
            "| name | scope | model | turn_limit | tools | severity_default | description |\n",
        );
        out.push_str(
            "|------|-------|-------|------------|-------|------------------|-------------|\n",
        );
        for check in &discovered.checks {
            let scope = if check.scope_dir.is_empty() {
                "<root>".to_string()
            } else {
                check.scope_dir.clone()
            };
            let model = check
                .resolved_model(default_model, override_model)
                .unwrap_or("<agent default>");
            let turn_limit = check.resolved_turn_limit(default_turn_limit);
            let tools = match check.tools.as_ref() {
                Some(t) if !t.is_empty() => t.join(", "),
                _ => "*".to_string(),
            };
            let severity = check.severity_default.as_deref().unwrap_or("");
            let description = check.description.as_deref().unwrap_or("");
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {} |",
                escape_pipe(&check.name),
                escape_pipe(&scope),
                escape_pipe(model),
                turn_limit,
                escape_pipe(&tools),
                escape_pipe(severity),
                escape_pipe(description),
            );
        }
        out.push('\n');
        for check in &discovered.checks {
            append_check_body(&mut out, check);
        }
    }

    out.push_str("## Diff\n\n");
    out.push_str("```diff\n");
    out.push_str(diff.trim_end_matches('\n'));
    out.push_str("\n```\n");
    out
}

fn append_check_body(out: &mut String, check: &Check) {
    let scope = if check.scope_dir.is_empty() {
        "<root>".to_string()
    } else {
        check.scope_dir.clone()
    };
    let _ = writeln!(out, "### Check: {} (scope: {})", check.name, scope);
    out.push_str(check.body.trim());
    out.push_str("\n\n");
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}
