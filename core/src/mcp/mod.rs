//! The diffpilot MCP server. A run reaches it through `--mcp-config`, as `diffpilot-mcp` over stdio. That
//! binary forwards each tool call over a unix socket to the `McpBridge` in the app, which runs it against
//! the review database. The split follows diffity's `diffity-mcp` and `agents/bridge.rs` (MIT, see
//! THIRD_PARTY_NOTICES), without tokio or rmcp: the bridge uses threads, and the stdio server speaks the four
//! JSON-RPC methods a client needs.

pub mod bridge;
mod guide;
pub mod launch;
pub mod protocol;
pub mod tools;

pub use bridge::{Binding, McpBridge, Notice, PermissionRequest, socket_path_for};
pub use tools::{ToolDef, ToolError};
