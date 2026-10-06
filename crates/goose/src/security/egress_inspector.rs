use crate::tool_inspection::{InspectionAction, InspectionResult, ToolInspector};
use anyhow::Result;
use async_trait::async_trait;
use bcaip_provider_types::conversations::{Message, ToolRequest};
use bcaip_provider_types::goose_mode::GooseMode;
use regex::Regex;
use std::{collections::HashSet, sync::OnceLock};

pub struct EgressInspector;

impl EgressInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EgressInspector {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum EgressDirection {
    Outbound,
    Inbound,
    Unknown,
}

impl EgressDirection {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Outbound => "outbound",
            Self::Inbound => "inbound",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
struct EgressDestination {
    kind: String,
    destination: String,
    domain: String,
}

fn extract_destinations(command: &str) -> Vec<EgressDestination> {
    let mut destinations = Vec::new();

    static URL_RE: OnceLock<Regex> = OnceLock::new();
    let url_re = URL_RE.get_or_init(|| Regex::new(r#"(?i)(https?|ftp)://[^\s'"<>|;&)]+"#).unwrap());
    for cap in url_re.find_iter(command) {
        let url = cap.as_str().to_string();
        let domain = extract_domain_from_url(&url).unwrap_or_default();
        if !domain.is_empty() {
            destinations.push(EgressDestination {
                kind: "url".to_string(),
                destination: url,
                domain,
            });
        }
    }

    static GIT_SSH_RE: OnceLock<Regex> = OnceLock::new();
    let git_ssh_re = GIT_SSH_RE.get_or_init(|| Regex::new(r#"git@([^:]+):([^\s'"]+)"#).unwrap());
    for cap in git_ssh_re.captures_iter(command) {
        let domain = cap[1].to_string();
        let path = cap[2].to_string();
        destinations.push(EgressDestination {
            kind: "git_remote".to_string(),
            destination: format!("git@{}:{}", domain, path),
            domain,
        });
    }

    static S3_RE: OnceLock<Regex> = OnceLock::new();
    let s3_re = S3_RE.get_or_init(|| Regex::new(r#"s3://([^/\s'"]+)(/[^\s'"]*)?"#).unwrap());
    for cap in s3_re.captures_iter(command) {
        let bucket = cap[1].to_string();
        let full = cap[0].to_string();
        destinations.push(EgressDestination {
            kind: "s3_bucket".to_string(),
            destination: full,
            domain: format!("{}.s3.amazonaws.com", bucket),
        });
    }

    static GCS_RE: OnceLock<Regex> = OnceLock::new();
    let gcs_re = GCS_RE.get_or_init(|| Regex::new(r#"gs://([^/\s'"]+)(/[^\s'"]*)?"#).unwrap());
    for cap in gcs_re.captures_iter(command) {
        let bucket = cap[1].to_string();
        let full = cap[0].to_string();
        destinations.push(EgressDestination {
            kind: "gcs_bucket".to_string(),
            destination: full,
            domain: format!("{}.storage.googleapis.com", bucket),
        });
    }

    static SCP_RE: OnceLock<Regex> = OnceLock::new();
    let scp_re = SCP_RE
        .get_or_init(|| Regex::new(r"(?:scp|rsync)\s+.*?(?:\S+@)?([a-zA-Z0-9][\w.-]+):").unwrap());
    for cap in scp_re.captures_iter(command) {
        let host = cap[1].to_string();
        destinations.push(EgressDestination {
            kind: "scp_target".to_string(),
            destination: cap[0].to_string(),
            domain: host,
        });
    }

    static SSH_RE: OnceLock<Regex> = OnceLock::new();
    let ssh_re = SSH_RE.get_or_init(|| {
        Regex::new(r"ssh\s+(?:-\w+\s+\S+\s+)*(?:\S+@)?([a-zA-Z0-9][\w.-]+)").unwrap()
    });
    for cap in ssh_re.captures_iter(command) {
        let host = cap[1].to_string();
        if !host.starts_with('-') {
            destinations.push(EgressDestination {
                kind: "ssh_target".to_string(),
                destination: cap[0].to_string(),
                domain: host,
            });
        }
    }

    static DOCKER_RE: OnceLock<Regex> = OnceLock::new();
    let docker_re = DOCKER_RE.get_or_init(|| {
        Regex::new(r#"docker\s+(?:push|login)\s+(?:--[^\s]+\s+)*([^\s'"]+)"#).unwrap()
    });
    for cap in docker_re.captures_iter(command) {
        let target = cap[1].to_string();
        let domain = target.split('/').next().unwrap_or(&target).to_string();
        destinations.push(EgressDestination {
            kind: "docker_registry".to_string(),
            destination: target,
            domain,
        });
    }

    static GENERIC_NET_CMD_RE: OnceLock<Regex> = OnceLock::new();
    let generic_net_cmd_re = GENERIC_NET_CMD_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(fetch|nc|ncat|netcat|ftp|sftp|socat|httpie|xh)\b[^\n]*?\b((?:[a-zA-Z0-9](?:[a-zA-Z0-9\-]*[a-zA-Z0-9])?\.)+[a-zA-Z]{2,})\b"
        ).unwrap()
    });
    let already_seen: HashSet<String> = destinations
        .iter()
        .map(|d| d.domain.to_lowercase())
        .collect();
    for cap in generic_net_cmd_re.captures_iter(command) {
        let domain = cap[2].to_string();
        if !already_seen.contains(&domain) {
            destinations.push(EgressDestination {
                kind: "generic_network".to_string(),
                destination: cap[0].to_string(),
                domain,
            });
        }
    }

    static NPM_PUBLISH_RE: OnceLock<Regex> = OnceLock::new();
    let npm_publish_re = NPM_PUBLISH_RE
        .get_or_init(|| Regex::new(r"(?:^|[;&|]\s*|\n)\s*npm\s+publish(?:\s|$)").unwrap());
    if npm_publish_re.is_match(command) {
        destinations.push(EgressDestination {
            kind: "package_publish".to_string(),
            destination: "npm publish".to_string(),
            domain: "registry.npmjs.org".to_string(),
        });
    }

    static CARGO_PUBLISH_RE: OnceLock<Regex> = OnceLock::new();
    let cargo_publish_re = CARGO_PUBLISH_RE
        .get_or_init(|| Regex::new(r"(?:^|[;&|]\s*|\n)\s*cargo\s+publish(?:\s|$)").unwrap());
    if cargo_publish_re.is_match(command) {
        destinations.push(EgressDestination {
            kind: "package_publish".to_string(),
            destination: "cargo publish".to_string(),
            domain: "crates.io".to_string(),
        });
    }

    destinations
}

fn extract_domain_from_url(url: &str) -> Option<String> {
    let after_scheme = url
        .find("://")
        .and_then(|i| url.get(i + 3..))
        .unwrap_or(url);
    let authority = after_scheme.split('/').next()?;
    let host_port = authority.split('@').next_back()?;
    let host = if host_port.contains('[') {
        host_port
            .split(']')
            .next()
            .map(|s| s.trim_start_matches('['))?
    } else {
        host_port.split(':').next()?
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

fn detect_direction(command: &str) -> EgressDirection {
    let lower = command.to_lowercase();

    if lower.contains("git push") || lower.contains("git remote add") {
        return EgressDirection::Outbound;
    }
    if lower.contains("git clone") || lower.contains("git pull") || lower.contains("git fetch") {
        return EgressDirection::Inbound;
    }

    if lower.contains("gh repo create") || lower.contains("gh repo fork") {
        return EgressDirection::Outbound;
    }

    static CURL_UPLOAD_RE: OnceLock<Regex> = OnceLock::new();
    let curl_upload_re = CURL_UPLOAD_RE.get_or_init(|| {
        Regex::new(r"(?i)\bcurl\b.*(-X\s*(POST|PUT|PATCH)|--data|--data-raw|--data-binary|-d\s|-F\s|--form|--upload-file|-T\s)").unwrap()
    });
    if curl_upload_re.is_match(command) {
        return EgressDirection::Outbound;
    }

    static WGET_UPLOAD_RE: OnceLock<Regex> = OnceLock::new();
    let wget_upload_re = WGET_UPLOAD_RE.get_or_init(|| {
        Regex::new(r"(?i)\bwget\b.*(--post-data|--post-file|--body-data|--body-file)").unwrap()
    });
    if wget_upload_re.is_match(command) {
        return EgressDirection::Outbound;
    }

    if lower.contains("npm publish")
        || lower.contains("cargo publish")
        || lower.contains("pip upload")
        || lower.contains("twine upload")
        || lower.contains("gem push")
    {
        return EgressDirection::Outbound;
    }

    if lower.contains("docker push") {
        return EgressDirection::Outbound;
    }
    if lower.contains("docker pull") {
        return EgressDirection::Inbound;
    }

    if lower.contains("scp ") || lower.contains("rsync ") {
        let args: Vec<&str> = command.split_whitespace().collect();
        if let Some(last) = args.last() {
            if last.contains(':') {
                return EgressDirection::Outbound; // local → remote dest
            } else {
                return EgressDirection::Inbound; // remote src → local
            }
        }
    }

    if lower.contains("curl ") || lower.contains("wget ") {
        return EgressDirection::Inbound;
    }

    EgressDirection::Unknown
}

fn is_shell_tool(name: &str) -> bool {
    matches!(
        name,
        "shell" | "bash" | "execute_command" | "run_command" | "terminal"
    ) || name.ends_with("__shell")
        || name.ends_with("__bash")
        || name.ends_with("__terminal")
}

fn is_web_tool(name: &str) -> bool {
    matches!(
        name,
        "web_fetch" | "fetch" | "browser_navigate" | "http_request"
    ) || name.ends_with("__web_fetch")
        || name.ends_with("__fetch")
        || name.ends_with("__browser_navigate")
}

fn extract_text_for_inspection(
    tool_call: &rmcp::model::CallToolRequestParams,
    is_web: bool,
) -> Option<String> {
    let args = tool_call.arguments.as_ref()?;
    let keys: &[&str] = if is_web {
        &["url", "uri", "endpoint"]
    } else {
        &["command", "cmd", "script", "input"]
    };
    keys.iter()
        .find_map(|k| args.get(*k).and_then(|v| v.as_str()).map(|s| s.to_string()))
}

#[async_trait]
impl ToolInspector for EgressInspector {
    fn name(&self) -> &'static str {
        "egress"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn inspect(
        &self,
        _session_id: &str,
        tool_requests: &[ToolRequest],
        _messages: &[Message],
        _goose_mode: GooseMode,
    ) -> Result<Vec<InspectionResult>> {
        let mut results = Vec::new();
        let mut seen_destinations: HashSet<String> = HashSet::new();

        for tool_request in tool_requests {
            let tool_call = match &tool_request.tool_call {
                Ok(tc) => tc,
                Err(_) => continue,
            };

            let name = tool_call.name.as_ref();
            let is_web = is_web_tool(name);
            if !is_shell_tool(name) && !is_web {
                continue;
            }

            let text = match extract_text_for_inspection(tool_call, is_web) {
                Some(t) => t,
                None => continue,
            };

            let destinations: Vec<_> = extract_destinations(&text)
                .into_iter()
                .filter(|d| seen_destinations.insert(d.destination.clone()))
                .collect();

            if destinations.is_empty() {
                continue;
            }

            let direction = detect_direction(&text);

            for dest in &destinations {
                tracing::info!(
                    security.event_type = "egress",
                    security.action = "LOG",
                    security.threat_type = "data_exfiltration",
                    network.destination = dest.destination.as_str(),
                    network.domain = dest.domain.as_str(),
                    network.egress_kind = dest.kind.as_str(),
                    network.direction = direction.as_str(),
                    tool.name = name,
                    "network egress detected"
                );
            }

            results.push(InspectionResult {
                tool_request_id: tool_request.id.clone(),
                action: InspectionAction::Allow,
                reason: format!(
                    "Egress destinations detected: {}",
                    destinations
                        .iter()
                        .map(|d| d.destination.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                confidence: 0.0,
                inspector_name: self.name().to_string(),
                finding_id: None,
            });
        }

        Ok(results)
    }
}
