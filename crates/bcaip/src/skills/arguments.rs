use crate::utils::split_command_args;
use anyhow::Result;
use regex::{Captures, Regex};
use std::sync::LazyLock;
static PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\$ARGUMENTS\[(?P<idx>\d+)\]|\$ARGUMENTS\b|\$(?P<pos>\d+)|\$(?P<name>[A-Za-z_][A-Za-z0-9_-]*)",
    )
    .expect("skill argument regex should compile")
});

fn is_resolvable(caps: &Captures<'_>, names: &[String]) -> bool {
    caps.name("name")
        .map(|m| names.iter().any(|n| n == m.as_str()))
        .unwrap_or(true)
}

pub(crate) fn apply_skill_arguments(
    content: &str,
    raw_args: &str,
    argument_names: &[String],
) -> Result<String> {
    if !PLACEHOLDER_RE
        .captures_iter(content)
        .any(|caps| is_resolvable(&caps, argument_names))
    {
        return Ok(format!("{content}\n\nARGUMENTS: {raw_args}"));
    }

    let tokens = split_command_args(raw_args)?;
    let nth = |i: usize| tokens.get(i).cloned().unwrap_or_default();

    let rendered = PLACEHOLDER_RE.replace_all(content, |caps: &Captures<'_>| {
        if let Some(n) = caps.name("idx") {
            return nth(n.as_str().parse().unwrap_or(usize::MAX));
        }
        if let Some(n) = caps.name("pos") {
            let p: usize = n.as_str().parse().unwrap_or(0);
            return p.checked_sub(1).map_or_else(String::new, nth);
        }
        if let Some(name) = caps.name("name") {
            return argument_names
                .iter()
                .position(|n| n == name.as_str())
                .map_or_else(|| caps[0].to_string(), nth);
        }
        raw_args.to_string()
    });

    Ok(rendered.into_owned())
}
