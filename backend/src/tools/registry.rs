use serde_json::{Value, json};

pub(crate) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "project.list_files",
                "description": "List files within the authorized project. Read-only.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative directory path, or '.' for the project root."
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "project.read_file",
                "description": "Read numbered lines of a UTF-8 project file. Read-only.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative file path."
                        },
                        "start_line": {
                            "type": "integer",
                            "minimum": 1
                        },
                        "max_lines": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 120
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "project.search_text",
                "description": "Search project text files for a literal, case-sensitive string. Read-only.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "minLength": 2
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }),
    ]
}

pub(crate) fn contains(name: &str) -> bool {
    matches!(
        name,
        "project.list_files"
            | "project.read_file"
            | "project.search_text"
    )
}
