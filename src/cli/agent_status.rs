//! Sidebar agent status published by provider hooks: running, needs input or idle.
//!
//! Each provider owns one status key in the workspace that contains the hook's terminal
//! surface, so `cmux list-status` and the sidebar show the same state.
use super::socket_client::SocketClient;
use serde_json::{json, Value};

/// Lifecycle phase derived from one hook event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// A session started and is waiting for its first prompt, or a turn finished.
    Idle,
    /// A prompt was submitted and the agent is working.
    Running,
    /// The agent asked for permission or input.
    NeedsInput,
    /// The session ended; its status is removed.
    Ended,
}

/// Status key for one provider, for example `agent.claude`.
pub fn key(kind: &str) -> String {
    format!("agent.{kind}")
}

/// The socket method and parameters that publish `phase` for `kind` on `surface`.
pub fn request(surface: &str, kind: &str, display: &str, phase: Phase) -> (&'static str, Value) {
    let (label, color, priority) = match phase {
        Phase::Idle => ("idle", None, 0),
        Phase::Running => ("running", Some("#3584e4"), 10),
        Phase::NeedsInput => ("needs input", Some("#e5a50a"), 20),
        Phase::Ended => {
            return (
                "sidebar.clear_status",
                json!({"surface_id": surface, "key": key(kind)}),
            );
        }
    };
    (
        "sidebar.set_status",
        json!({
            "surface_id": surface,
            "key": key(kind),
            "value": format!("{display}: {label}"),
            "color": color,
            "priority": priority,
        }),
    )
}

/// Publish best-effort: a status failure (for example an older app) never blocks the hook's
/// resume binding or notification.
pub fn publish(client: &mut SocketClient, surface: &str, kind: &str, display: &str, phase: Phase) {
    let (method, params) = request(surface, kind, display, phase);
    if let Err(error) = client.call(method, params) {
        eprintln!("cmux: agent status not updated: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURFACE: &str = "6f1c1d1e-2a3b-4c5d-8e9f-0a1b2c3d4e5f";

    #[test]
    fn phases_map_to_sidebar_status() {
        let (method, params) = request(SURFACE, "claude", "Claude", Phase::Running);
        assert_eq!(method, "sidebar.set_status");
        assert_eq!(params["surface_id"], SURFACE);
        assert_eq!(params["key"], "agent.claude");
        assert_eq!(params["value"], "Claude: running");
        assert_eq!(params["color"], "#3584e4");

        let (_, params) = request(SURFACE, "claude", "Claude", Phase::NeedsInput);
        assert_eq!(params["value"], "Claude: needs input");
        assert_eq!(params["priority"], 20);

        let (_, params) = request(SURFACE, "codex", "Codex", Phase::Idle);
        assert_eq!(params["value"], "Codex: idle");
        assert!(params["color"].is_null());

        let (method, params) = request(SURFACE, "claude", "Claude", Phase::Ended);
        assert_eq!(method, "sidebar.clear_status");
        assert_eq!(
            params,
            json!({"surface_id": SURFACE, "key": "agent.claude"})
        );
    }
}
