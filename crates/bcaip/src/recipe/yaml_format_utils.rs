use std::fmt::Write;
/// Normalizes how `yaml_serde` outputs multi-line strings.
/// It uses internal heuristics to decide between `|` and quoted text with escaped
/// `\n` and `\"`, and the quoted form breaks MiniJinja parsing.
/// Example before:
///   prompt: "Hello \\\"World\\\"\\n{% if user == \\\"admin\\\" %}Welcome{% endif %}"
/// After fix:
///   prompt: |
///     Hello "World"
///     {% if user == "admin" %}Welcome{% endif %}
pub fn reformat_fields_with_multiline_values(yaml: &str, multiline_fields: &[&str]) -> String {
    let mut result = String::new();

    for line in yaml.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            writeln!(result).unwrap();
            continue;
        }

        let indent = line.len() - trimmed.len();
        let indent_str = " ".repeat(indent);

        let matched_field = multiline_fields
            .iter()
            .find(|&f| trimmed.starts_with(&format!("{f}: ")));

        if let Some(field) = matched_field {
            if let Some((_, raw_val)) = trimmed.split_once(": ") {
                if raw_val.contains("\\n") {
                    // Clean escaped content and unescape quotes
                    let mut value = raw_val.trim_matches('"').to_string();

                    // Unescape quotes and double backslashes (MiniJinja + newlines)
                    value = value.replace("\\\"", "\"").replace("\\\\n", "\\n");

                    writeln!(result, "{indent_str}{field}: |").unwrap();
                    for l in value.split("\\n") {
                        writeln!(result, "{indent_str}  {l}").unwrap();
                    }
                    continue;
                }
            }
        }

        writeln!(result, "{line}").unwrap();
    }

    let mut output = result.trim_end_matches('\n').to_string();
    output.push('\n');
    output
}
