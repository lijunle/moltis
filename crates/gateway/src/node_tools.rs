//! Node-advertised tool lifecycle: register/unregister custom tools from
//! connected remote nodes into the agent's [`ToolRegistry`].
//!
//! When a node connects and advertises tools in its handshake, the gateway
//! creates a [`NodeToolAdapter`] for each tool and registers it. When the
//! node disconnects, its tools are removed from the registry.

use std::{sync::Arc, time::Duration};

use {
    anyhow::Result,
    async_trait::async_trait,
    moltis_agents::tool_registry::{AgentTool, ToolRegistry},
    moltis_node_host::NodeToolSchema,
    tokio::sync::RwLock,
    tracing::info,
};

use crate::{nodes::NodeSession, state::GatewayState};

// ── Naming ──────────────────────────────────────────────────────────────────

/// Build the qualified tool name: `node__<node_id>__<tool_name>`.
///
/// The node_id is used as-is (UUIDs with hyphens are valid in tool names).
/// This avoids lossy sanitization that could cause collisions between
/// different node IDs.
fn qualified_name(node_id: &str, tool_name: &str) -> String {
    format!("node__{node_id}__{tool_name}")
}

// ── NodeToolAdapter ─────────────────────────────────────────────────────────

/// Adapts a node-advertised tool definition into an [`AgentTool`].
///
/// Execution is forwarded to the node via the existing `node.invoke.request`
/// mechanism. The tool's command name is the original tool name (not the
/// qualified name), since the node dispatches by its own tool names.
struct NodeToolAdapter {
    qname: String,
    node_id: String,
    def: NodeToolSchema,
    state: Arc<GatewayState>,
}

#[async_trait]
impl AgentTool for NodeToolAdapter {
    fn name(&self) -> &str {
        &self.qname
    }

    fn description(&self) -> &str {
        &self.def.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.def.parameters.clone()
    }

    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        exec_tool_on_node(&self.state, &self.node_id, &self.def.name, params).await
    }
}

// ── Invoke helper ───────────────────────────────────────────────────────────

/// Send a custom tool invocation to a node and wait for the result.
///
/// Reuses the same `node.invoke.request` → `node.invoke.result` mechanism
/// as `exec_on_node()`, but sends the tool's original name as `command`
/// and the agent-provided params as `args`.
async fn exec_tool_on_node(
    state: &Arc<GatewayState>,
    node_id: &str,
    tool_name: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value> {
    // Look up node connection.
    let conn_id = {
        let inner = state.inner.read().await;
        let node = inner
            .nodes
            .get(node_id)
            .ok_or_else(|| anyhow::anyhow!("Node '{node_id}' is offline. Tool unavailable."))?;
        node.conn_id.clone()
    };

    let invoke_id = uuid::Uuid::new_v4().to_string();
    let invoke_event = moltis_protocol::EventFrame::new(
        "node.invoke.request",
        serde_json::json!({
            "invokeId": invoke_id,
            "command": tool_name,
            "args": params,
        }),
        state.next_seq(),
    );
    let event_json = serde_json::to_string(&invoke_event)?;

    {
        let inner = state.inner.read().await;
        let client = inner
            .clients
            .get(&conn_id)
            .ok_or_else(|| anyhow::anyhow!("node connection lost"))?;
        if !client.send(&event_json) {
            anyhow::bail!("failed to send tool invoke to node");
        }
    }

    // Register pending invoke and wait.
    let (tx, rx) = tokio::sync::oneshot::channel();
    {
        let mut inner = state.inner.write().await;
        inner
            .pending_invokes
            .insert(invoke_id.clone(), crate::state::PendingInvoke {
                request_id: invoke_id.clone(),
                sender: tx,
                created_at: std::time::Instant::now(),
            });
    }

    // Node tools default to 5-minute timeout (handler-level timeout is on
    // the node side; this is a gateway-level safeguard).
    let timeout = Duration::from_secs(300);
    let result = match tokio::time::timeout(timeout, rx).await {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => anyhow::bail!("node tool invoke cancelled"),
        Err(_) => {
            state.inner.write().await.pending_invokes.remove(&invoke_id);
            anyhow::bail!("node tool invoke timeout after 300s");
        },
    };

    Ok(result)
}

// ── Lifecycle hooks ─────────────────────────────────────────────────────────

/// Register all tools from a newly connected node into the tool registry.
pub async fn on_node_connect(
    registry: &Arc<RwLock<ToolRegistry>>,
    node: &NodeSession,
    state: &Arc<GatewayState>,
) {
    if node.tool_defs.is_empty() {
        return;
    }

    let mut reg = registry.write().await;
    let mut count = 0;

    for def in &node.tool_defs {
        let qname = qualified_name(&node.node_id, &def.name);
        let adapter = NodeToolAdapter {
            qname,
            node_id: node.node_id.clone(),
            def: def.clone(),
            state: Arc::clone(state),
        };
        reg.register_node(Box::new(adapter), node.node_id.clone());
        count += 1;
    }

    info!(
        node_id = %node.node_id,
        tools = count,
        "registered node-advertised tools"
    );
}

/// Remove all tools from a disconnected node from the tool registry.
pub async fn on_node_disconnect(registry: &Arc<RwLock<ToolRegistry>>, node_id: &str) {
    let removed = registry.write().await.unregister_node(node_id);
    if removed > 0 {
        info!(node_id = %node_id, removed, "unregistered node-advertised tools");
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn qualified_name_formats_correctly() {
        assert_eq!(
            qualified_name("abc-123", "xcode_build"),
            "node__abc-123__xcode_build"
        );
    }

    #[test]
    fn qualified_name_preserves_uuid() {
        assert_eq!(
            qualified_name("aa75f3f0-21ba-41ed-9cb1-417165c2c1b2", "build"),
            "node__aa75f3f0-21ba-41ed-9cb1-417165c2c1b2__build"
        );
    }
}
