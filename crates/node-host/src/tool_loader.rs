//! Load tool definitions from `~/.moltis/node-tools/*.json`.

use {
    crate::tool_def::NodeToolDef,
    std::{collections::HashMap, path::Path},
    tracing::{debug, info, warn},
};

/// Load all tool definitions from the given directory.
///
/// Each `.json` file in the directory is expected to contain a single
/// [`NodeToolDef`]. Files that fail to parse are logged and skipped.
pub fn load_tools(dir: &Path) -> HashMap<String, NodeToolDef> {
    let mut tools = HashMap::new();

    if !dir.is_dir() {
        debug!(path = %dir.display(), "node-tools directory does not exist, skipping");
        return tools;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            warn!(path = %dir.display(), error = %e, "failed to read node-tools directory");
            return tools;
        },
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        match load_tool_file(&path) {
            Ok(def) => {
                if tools.contains_key(&def.name) {
                    warn!(
                        name = %def.name,
                        path = %path.display(),
                        "duplicate tool name, skipping"
                    );
                    continue;
                }
                info!(name = %def.name, path = %path.display(), "loaded node tool");
                tools.insert(def.name.clone(), def);
            },
            Err(e) => {
                warn!(path = %path.display(), error = %e, "failed to load tool definition");
            },
        }
    }

    if !tools.is_empty() {
        info!(count = tools.len(), "node tools loaded");
    }

    tools
}

fn load_tool_file(path: &Path) -> anyhow::Result<NodeToolDef> {
    let content = std::fs::read_to_string(path)?;
    let def: NodeToolDef = serde_json::from_str(&content)?;

    // Basic validation.
    if def.name.is_empty() {
        anyhow::bail!("tool name must not be empty");
    }
    if def.name.contains("__") {
        anyhow::bail!("tool name must not contain '__' (reserved for namespacing)");
    }
    if def.description.is_empty() {
        anyhow::bail!("tool description must not be empty");
    }

    Ok(def)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn load_tools_from_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let tools = load_tools(dir.path());
        assert!(tools.is_empty());
    }

    #[test]
    fn load_tools_from_nonexistent_dir() {
        let tools = load_tools(Path::new("/tmp/definitely-does-not-exist-node-tools"));
        assert!(tools.is_empty());
    }

    #[test]
    fn load_tools_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let tool_json = r#"{
            "name": "test_tool",
            "description": "A test tool",
            "parameters": { "type": "object", "properties": {} },
            "handler": { "type": "exec", "program": "echo", "args": ["hello"] }
        }"#;
        std::fs::write(dir.path().join("test_tool.json"), tool_json).unwrap();
        let tools = load_tools(dir.path());
        assert_eq!(tools.len(), 1);
        assert!(tools.contains_key("test_tool"));
    }

    #[test]
    fn load_tools_skips_non_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not a tool").unwrap();
        let tools = load_tools(dir.path());
        assert!(tools.is_empty());
    }

    #[test]
    fn load_tools_skips_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("bad.json"), "not valid json").unwrap();
        let tools = load_tools(dir.path());
        assert!(tools.is_empty());
    }

    #[test]
    fn load_tools_rejects_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let tool_json = r#"{
            "name": "",
            "description": "empty name",
            "parameters": { "type": "object", "properties": {} },
            "handler": { "type": "exec", "program": "echo" }
        }"#;
        std::fs::write(dir.path().join("bad.json"), tool_json).unwrap();
        let tools = load_tools(dir.path());
        assert!(tools.is_empty());
    }

    #[test]
    fn load_tools_rejects_double_underscore_name() {
        let dir = tempfile::tempdir().unwrap();
        let tool_json = r#"{
            "name": "bad__name",
            "description": "has double underscore",
            "parameters": { "type": "object", "properties": {} },
            "handler": { "type": "exec", "program": "echo" }
        }"#;
        std::fs::write(dir.path().join("bad.json"), tool_json).unwrap();
        let tools = load_tools(dir.path());
        assert!(tools.is_empty());
    }

    #[test]
    fn load_tools_rejects_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let tool_json = r#"{
            "name": "dupe",
            "description": "first",
            "parameters": { "type": "object", "properties": {} },
            "handler": { "type": "exec", "program": "echo" }
        }"#;
        std::fs::write(dir.path().join("a_dupe.json"), tool_json).unwrap();
        std::fs::write(dir.path().join("b_dupe.json"), tool_json).unwrap();
        let tools = load_tools(dir.path());
        assert_eq!(tools.len(), 1);
    }
}
