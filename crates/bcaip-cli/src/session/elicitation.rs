use console::style;
use rmcp::model::ElicitationAction;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead, IsTerminal, Write};
use tokio_util::sync::CancellationToken;
pub struct ElicitationInput {
    pub action: ElicitationAction,
    pub user_data: HashMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SelectChoice {
    Value(String),
    Skip,
}

struct SingleSelect<'a> {
    field_name: &'a str,
    description: Option<&'a str>,
    options: Vec<(SelectChoice, String)>,
    initial_value: Option<SelectChoice>,
}

pub fn collect_elicitation_input(
    message: &str,
    schema: &Value,
    cancel_token: &CancellationToken,
) -> io::Result<ElicitationInput> {
    if cancel_token.is_cancelled() {
        return Ok(cancelled_input());
    }
    // Piped stdin may already contain the next --text conversation turn.
    // CLI elicitation has no separate answer channel for extension forms,
    // so leave queued input to the session and require a terminal here.
    if !io::stdin().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "elicitation requires an interactive terminal",
        ));
    }
    let input = collect_elicitation_input_inner(message, schema, cancel_token)?;
    if cancel_token.is_cancelled() {
        return Ok(cancelled_input());
    }
    Ok(input)
}

fn cancelled_input() -> ElicitationInput {
    ElicitationInput {
        action: ElicitationAction::Cancel,
        user_data: HashMap::new(),
    }
}

fn collect_elicitation_input_inner(
    message: &str,
    schema: &Value,
    cancel_token: &CancellationToken,
) -> io::Result<ElicitationInput> {
    if !message.is_empty() {
        println!("\n{}", style(message).cyan());
    }

    let properties = schema.get("properties").and_then(|p| p.as_object());

    if io::stdin().is_terminal() && io::stderr().is_terminal() {
        if let Some(select) = single_select(schema) {
            return prompt_single_select(select);
        }
    }

    let properties = match properties {
        Some(props) if !props.is_empty() => props,
        _ => {
            let prompt = if message.is_empty() {
                "Approve this action?"
            } else {
                "Approve?"
            };
            return match cliclack::confirm(prompt).initial_value(true).interact() {
                Ok(true) => Ok(ElicitationInput {
                    action: ElicitationAction::Accept,
                    user_data: HashMap::new(),
                }),
                Ok(false) => Ok(ElicitationInput {
                    action: ElicitationAction::Decline,
                    user_data: HashMap::new(),
                }),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(ElicitationInput {
                    action: ElicitationAction::Cancel,
                    user_data: HashMap::new(),
                }),
                Err(e) => Err(e),
            };
        }
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut data: HashMap<String, Value> = HashMap::new();

    for (name, field_schema) in properties {
        if cancel_token.is_cancelled() {
            return Ok(ElicitationInput {
                action: ElicitationAction::Cancel,
                user_data: HashMap::new(),
            });
        }
        let is_required = required.contains(&name.as_str());
        let field_type = field_schema
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("string");
        let description = field_schema.get("description").and_then(|d| d.as_str());
        let default = field_schema.get("default");
        let enum_values = field_schema.get("enum").and_then(|e| e.as_array());

        if field_type == "boolean" {
            let label = match description {
                Some(desc) => format!("{} ({})", name, desc),
                None => name.clone(),
            };
            let default_bool = default.and_then(|v| v.as_bool()).unwrap_or(false);

            match cliclack::confirm(&label)
                .initial_value(default_bool)
                .interact()
            {
                Ok(v) => {
                    data.insert(name.clone(), Value::Bool(v));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    return Ok(ElicitationInput {
                        action: ElicitationAction::Cancel,
                        user_data: HashMap::new(),
                    });
                }
                Err(e) => return Err(e),
            }
            continue;
        }

        if let Some(options) = enum_values {
            let opts: Vec<&str> = options.iter().filter_map(|v| v.as_str()).collect();
            println!("  {}: {}", style("Options").dim(), opts.join(", "));
        }

        print!("{}", style(name).yellow());
        if let Some(desc) = description {
            print!(" {}", style(format!("({})", desc)).dim());
        }
        if is_required {
            print!("{}", style("*").red());
        }
        if let Some(def) = default {
            print!(" {}", style(format!("[{}]", format_default(def))).dim());
        }
        print!(": ");
        io::stdout().flush()?;

        let input = read_line(cancel_token)?;

        if input.is_none() {
            return Ok(ElicitationInput {
                action: ElicitationAction::Cancel,
                user_data: HashMap::new(),
            });
        }
        let input = input.unwrap();

        let value = if input.is_empty() {
            default.cloned()
        } else {
            Some(parse_value(&input, field_type, enum_values))
        };

        if let Some(v) = value {
            if !v.is_null() {
                data.insert(name.clone(), v);
            }
        }

        if is_required && !data.contains_key(name) {
            println!(
                "{}",
                style(format!("Required field '{}' is missing", name)).red()
            );
            return Ok(ElicitationInput {
                action: ElicitationAction::Decline,
                user_data: HashMap::new(),
            });
        }
    }

    println!();
    Ok(ElicitationInput {
        action: ElicitationAction::Accept,
        user_data: data,
    })
}

fn single_select(schema: &Value) -> Option<SingleSelect<'_>> {
    let properties = schema.get("properties")?.as_object()?;
    if properties.len() != 1 {
        return None;
    }

    let (field_name, field_schema) = properties.iter().next()?;
    let description = field_schema.get("description").and_then(Value::as_str);
    let mut options: Vec<(SelectChoice, String)> =
        if let Some(one_of) = field_schema.get("oneOf").and_then(Value::as_array) {
            one_of
                .iter()
                .map(|option| {
                    let value = option.get("const")?.as_str()?;
                    let label = option.get("title").and_then(Value::as_str).unwrap_or(value);
                    Some((SelectChoice::Value(value.to_string()), label.to_string()))
                })
                .collect::<Option<_>>()?
        } else {
            field_schema
                .get("enum")?
                .as_array()?
                .iter()
                .map(|value| {
                    let value = value.as_str()?;
                    Some((SelectChoice::Value(value.to_string()), value.to_string()))
                })
                .collect::<Option<_>>()?
        };

    if options.is_empty() {
        return None;
    }

    let is_required = schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| {
            required
                .iter()
                .any(|value| value.as_str() == Some(field_name))
        });
    let default_value = field_schema
        .get("default")
        .and_then(Value::as_str)
        .map(|value| SelectChoice::Value(value.to_string()))
        .filter(|value| options.iter().any(|(option, _)| option == value));

    let initial_value = if !is_required && default_value.is_none() {
        options.push((SelectChoice::Skip, "Skip".to_string()));
        Some(SelectChoice::Skip)
    } else {
        default_value
    };

    Some(SingleSelect {
        field_name,
        description,
        options,
        initial_value,
    })
}

fn prompt_single_select(select: SingleSelect<'_>) -> io::Result<ElicitationInput> {
    let items: Vec<_> = select
        .options
        .iter()
        .map(|(value, label)| (value.clone(), label, ""))
        .collect();
    let label = match select.description {
        Some(desc) => format!("{} ({})", select.field_name, desc),
        None => select.field_name.to_string(),
    };
    let mut prompt = cliclack::select(label).items(&items);
    if let Some(initial_value) = select.initial_value {
        prompt = prompt.initial_value(initial_value);
    }

    match prompt.interact() {
        Ok(SelectChoice::Value(value)) => Ok(ElicitationInput {
            action: ElicitationAction::Accept,
            user_data: HashMap::from([(select.field_name.to_string(), Value::String(value))]),
        }),
        Ok(SelectChoice::Skip) => Ok(ElicitationInput {
            action: ElicitationAction::Accept,
            user_data: HashMap::new(),
        }),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(ElicitationInput {
            action: ElicitationAction::Cancel,
            user_data: HashMap::new(),
        }),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn read_line(cancel_token: &CancellationToken) -> io::Result<Option<String>> {
    let mut reader = cancellable_stdin::Input::new(io::stdin().lock(), cancel_token)?;
    let result = read_line_from(&mut reader);
    if cancel_token.is_cancelled() || matches!(result, Ok(None)) {
        if matches!(result, Ok(None)) {
            reader.discard_terminal_input()?;
        }
        return Ok(None);
    }
    result
}

#[cfg(not(unix))]
fn read_line(cancel_token: &CancellationToken) -> io::Result<Option<String>> {
    if cancel_token.is_cancelled() {
        return Ok(None);
    }
    let result = read_line_from(&mut io::stdin().lock());
    if cancel_token.is_cancelled() {
        return Ok(None);
    }
    result
}

#[cfg(unix)]
mod cancellable_stdin {
    use super::*;
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    use rustix::termios::{QueueSelector, tcflush};
    use std::io::Read;
    pub(super) struct Input<'a> {
        stdin: io::StdinLock<'a>,
        original_flags: OFlags,
        cancel_token: &'a CancellationToken,
    }

    impl<'a> Input<'a> {
        pub(super) fn new(
            stdin: io::StdinLock<'a>,
            cancel_token: &'a CancellationToken,
        ) -> io::Result<Self> {
            // Nonblocking reads close the race between readiness and a signal flushing stdin.
            let original_flags = fcntl_getfl(&stdin)?;
            fcntl_setfl(&stdin, original_flags | OFlags::NONBLOCK)?;
            Ok(Self {
                stdin,
                original_flags,
                cancel_token,
            })
        }

        pub(super) fn discard_terminal_input(&self) -> io::Result<()> {
            // A programmatic SIGINT need not flush the terminal's unfinished line.
            if self.stdin.is_terminal() {
                tcflush(&self.stdin, QueueSelector::IFlush)?;
            }
            Ok(())
        }
    }

    impl Read for Input<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let available = self.fill_buf()?;
            let count = buffer.len().min(available.len());
            buffer[..count].copy_from_slice(&available[..count]);
            self.consume(count);
            Ok(count)
        }
    }

    impl BufRead for Input<'_> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            loop {
                if self.cancel_token.is_cancelled() {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                // Rustyline's nonterminal input may already have buffered the next answer.
                match self.stdin.fill_buf() {
                    Ok(_) => return self.stdin.fill_buf(),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
                let mut descriptors = [PollFd::new(&self.stdin, PollFlags::IN)];
                let timeout = Timespec {
                    tv_sec: 0,
                    tv_nsec: 50_000_000,
                };
                poll(&mut descriptors, Some(&timeout))?;
            }
        }

        fn consume(&mut self, amount: usize) {
            self.stdin.consume(amount);
        }
    }

    impl Drop for Input<'_> {
        fn drop(&mut self) {
            // Restore the shared stdin description before handing input back to the CLI.
            let _ = fcntl_setfl(&self.stdin, self.original_flags);
        }
    }
}

fn read_line_from(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        // BufRead::read_line retries Interrupted instead of allowing cancellation.
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(None),
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        line.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }

    let line = String::from_utf8(line).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })?;
    Ok(line.ends_with('\n').then(|| line.trim().to_string()))
}

fn format_default(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => value.to_string(),
    }
}

fn parse_value(input: &str, field_type: &str, enum_values: Option<&Vec<Value>>) -> Value {
    if let Some(options) = enum_values {
        let valid: Vec<&str> = options.iter().filter_map(|v| v.as_str()).collect();
        if valid.contains(&input) {
            return Value::String(input.to_string());
        }
        if let Ok(idx) = input.parse::<usize>() {
            if idx > 0 && idx <= valid.len() {
                return Value::String(valid[idx - 1].to_string());
            }
        }
    }

    match field_type {
        "boolean" => {
            let lower = input.to_lowercase();
            Value::Bool(matches!(lower.as_str(), "true" | "yes" | "y" | "1"))
        }
        "integer" => input
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .unwrap_or(Value::Null),
        "number" => input
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        _ => Value::String(input.to_string()),
    }
}
