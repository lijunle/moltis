//! Tool definition types for node-advertised tools.
//!
//! Users place JSON files in `~/.moltis/node-tools/` on the node machine.
//! Each file defines one tool with a JSON Schema for parameters and a handler
//! that describes how to execute it.

use serde::{Deserialize, Serialize};

/// A complete tool definition loaded from a JSON file on the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeToolDef {
    /// Tool name (e.g. "xcode_build"). Must be unique per node.
    pub name: String,
    /// Human-readable description shown to the agent.
    pub description: String,
    /// Optional MCP-standard annotations (hints about behavior).
    #[serde(default)]
    pub annotations: Option<ToolAnnotations>,
    /// JSON Schema describing the tool's parameters.
    pub parameters: serde_json::Value,
    /// How the tool is executed (stays on the node, never sent to gateway).
    pub handler: ToolHandler,
}

/// MCP-standard tool annotations (all optional, conservative defaults).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// Human-readable display name.
    #[serde(default)]
    pub title: Option<String>,
    /// Tool only reads data, does not modify anything.
    #[serde(default)]
    pub read_only_hint: Option<bool>,
    /// Tool may perform destructive/irreversible operations.
    #[serde(default)]
    pub destructive_hint: Option<bool>,
    /// Repeated calls with same params have no additional effect.
    #[serde(default)]
    pub idempotent_hint: Option<bool>,
    /// Tool interacts with external entities beyond its host.
    #[serde(default)]
    pub open_world_hint: Option<bool>,
}

/// How the tool is executed on the node.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolHandler {
    /// Direct process execution (no shell).
    Exec(ExecHandler),
    /// Forward parameters as JSON to a local HTTP endpoint.
    Http(HttpHandler),
}

/// Execute a program directly via `execve` — no shell involved.
///
/// Parameters are substituted into the `args` array as literal string values.
/// Each `{{param}}` is replaced with the parameter's value and passed as a
/// discrete argv element. The value is **never interpreted by a shell**.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecHandler {
    /// Program to execute (e.g. "xcodebuild").
    pub program: String,
    /// Argument templates. `{{param_name}}` placeholders are substituted.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory template (optional, supports `{{param}}` substitution).
    #[serde(default)]
    pub cwd: Option<String>,
    /// Kill the process after this many seconds.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Return only the last N lines of stdout (all if `None`).
    #[serde(default)]
    pub max_output_lines: Option<usize>,
    /// How to handle stderr: `"separate"` (default) or `"merge"` into stdout.
    #[serde(default = "default_stderr_mode")]
    pub stderr: StderrMode,
    /// Optional environment variables to set.
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

/// Forward JSON parameters to a local HTTP endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpHandler {
    /// URL to POST to (e.g. "http://localhost:8080/api/build").
    pub url: String,
    /// HTTP method (defaults to POST).
    #[serde(default = "default_http_method")]
    pub method: String,
    /// Kill the request after this many seconds.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Optional extra headers.
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
}

/// How to handle stderr output.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StderrMode {
    #[default]
    Separate,
    Merge,
}

fn default_timeout_secs() -> u64 {
    60
}

fn default_stderr_mode() -> StderrMode {
    StderrMode::Separate
}

fn default_http_method() -> String {
    "POST".into()
}

/// The subset of `NodeToolDef` sent to the gateway in the handshake.
/// Excludes the `handler` block for security.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeToolSchema {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    pub parameters: serde_json::Value,
}

impl From<&NodeToolDef> for NodeToolSchema {
    fn from(def: &NodeToolDef) -> Self {
        Self {
            name: def.name.clone(),
            description: def.description.clone(),
            annotations: def.annotations.clone(),
            parameters: def.parameters.clone(),
        }
    }
}

/// Substitute `{{param_name}}` placeholders in a template string with values
/// from the provided JSON object. Unknown placeholders are left as-is.
pub fn substitute_params(template: &str, params: &serde_json::Value) -> String {
    let mut result = template.to_string();
    if let Some(obj) = params.as_object() {
        for (key, value) in obj {
            let placeholder = format!("{{{{{key}}}}}");
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }
    result
}

/// Truncate text to the last `n` lines.
pub fn tail_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= n {
        return text.to_string();
    }
    lines[lines.len() - n..].join("\n")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn substitute_params_replaces_placeholders() {
        let params = serde_json::json!({
            "scheme": "MyApp",
            "configuration": "Debug",
        });
        let result = substitute_params("-scheme {{scheme}} -config {{configuration}}", &params);
        assert_eq!(result, "-scheme MyApp -config Debug");
    }

    #[test]
    fn substitute_params_leaves_unknown_placeholders() {
        let params = serde_json::json!({"a": "1"});
        let result = substitute_params("{{a}} {{b}}", &params);
        assert_eq!(result, "1 {{b}}");
    }

    #[test]
    fn substitute_params_handles_non_string_values() {
        let params = serde_json::json!({"count": 42, "flag": true});
        let result = substitute_params("--count={{count}} --flag={{flag}}", &params);
        assert_eq!(result, "--count=42 --flag=true");
    }

    #[test]
    fn substitute_params_injection_safe() {
        let params = serde_json::json!({"scheme": "; rm -rf /"});
        let result = substitute_params("{{scheme}}", &params);
        assert_eq!(result, "; rm -rf /");
    }

    #[test]
    fn tail_lines_truncates() {
        let text = "line1\nline2\nline3\nline4\nline5";
        assert_eq!(tail_lines(text, 3), "line3\nline4\nline5");
    }

    #[test]
    fn tail_lines_returns_all_when_fewer() {
        let text = "line1\nline2";
        assert_eq!(tail_lines(text, 5), "line1\nline2");
    }

    #[test]
    fn deserialize_exec_handler() {
        let json = r#"{
            "name": "xcode_build",
            "description": "Build an Xcode project",
            "parameters": { "type": "object", "properties": {} },
            "handler": {
                "type": "exec",
                "program": "xcodebuild",
                "args": ["-scheme", "{{scheme}}"],
                "timeout_secs": 300
            }
        }"#;
        let def: NodeToolDef = serde_json::from_str(json).unwrap();
        assert_eq!(def.name, "xcode_build");
        assert!(matches!(def.handler, ToolHandler::Exec(_)));
        if let ToolHandler::Exec(h) = &def.handler {
            assert_eq!(h.program, "xcodebuild");
            assert_eq!(h.timeout_secs, 300);
        }
    }

    #[test]
    fn deserialize_http_handler() {
        let json = r#"{
            "name": "api_call",
            "description": "Call a local API",
            "parameters": { "type": "object", "properties": {} },
            "handler": {
                "type": "http",
                "url": "http://localhost:8080/api",
                "timeout_secs": 30
            }
        }"#;
        let def: NodeToolDef = serde_json::from_str(json).unwrap();
        assert!(matches!(def.handler, ToolHandler::Http(_)));
    }

    #[test]
    fn deserialize_annotations() {
        let json = r#"{
            "name": "test",
            "description": "test",
            "annotations": {
                "title": "Test Tool",
                "readOnlyHint": true,
                "destructiveHint": false
            },
            "parameters": { "type": "object", "properties": {} },
            "handler": { "type": "exec", "program": "echo" }
        }"#;
        let def: NodeToolDef = serde_json::from_str(json).unwrap();
        let ann = def.annotations.unwrap();
        assert_eq!(ann.title.as_deref(), Some("Test Tool"));
        assert_eq!(ann.read_only_hint, Some(true));
        assert_eq!(ann.destructive_hint, Some(false));
    }

    #[test]
    fn node_tool_schema_excludes_handler() {
        let def = NodeToolDef {
            name: "test".into(),
            description: "desc".into(),
            annotations: None,
            parameters: serde_json::json!({}),
            handler: ToolHandler::Exec(ExecHandler {
                program: "secret_binary".into(),
                args: vec![],
                cwd: None,
                timeout_secs: 60,
                max_output_lines: None,
                stderr: StderrMode::Separate,
                env: Default::default(),
            }),
        };
        let schema = NodeToolSchema::from(&def);
        let json = serde_json::to_string(&schema).unwrap();
        assert!(!json.contains("secret_binary"));
        assert!(!json.contains("handler"));
    }
}
