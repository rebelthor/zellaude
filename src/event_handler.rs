use crate::state::{Activity, FlashMode, HookPayload, SessionInfo, State};

pub fn handle_hook_event(state: &mut State, payload: HookPayload) {
    // Capture env info for use in notifications
    if let Some(ref name) = payload.zellij_session {
        state.zellij_session_name = Some(name.clone());
    }
    if let Some(ref tp) = payload.term_program {
        state.term_program = Some(tp.clone());
    }

    let event = payload.hook_event.as_str();

    // SessionEnd → remove session (never drop: terminal cleanup)
    if event == "SessionEnd" {
        state.sessions.remove(&payload.pane_id);
        return;
    }

    // Background snapshots have their own clock and never change foreground
    // activity, attention, or elapsed time. Ignore snapshots for an old session
    // and snapshots arriving before SessionStart; the bridge refreshes them.
    if event == "BackgroundStatus" {
        if let (Some(background), Some(ts_ms), Some(session)) = (
            payload.background,
            payload.ts_ms,
            state.sessions.get_mut(&payload.pane_id),
        ) {
            if payload.session_id.as_deref() == Some(session.session_id.as_str())
                && ts_ms >= session.background_ts_ms
            {
                session.background = background;
                session.background_ts_ms = ts_ms;
                // The snapshot also carries /color, which fires no hook of its own.
                if let Some(color) = payload.color {
                    session.color = (color != "default").then_some(color);
                }
            }
        }
        return;
    }

    // Drop events that arrive out of order (async hooks can race through
    // parallel subprocesses). Only enforced when the hook supplied ts_ms —
    // an absent field means an old hook script and is treated as fresh.
    if let Some(ts_ms) = payload.ts_ms {
        if let Some(session) = state.sessions.get(&payload.pane_id) {
            if ts_ms < session.last_ts_ms {
                return;
            }
        }
    }

    let activity = match event {
        "SessionStart" => Activity::Init,
        // A pending AskUserQuestion blocks on the user, like a permission prompt.
        "PreToolUse" if payload.tool_name.as_deref() == Some("AskUserQuestion") => Activity::Waiting,
        "PreToolUse" => {
            Activity::Tool(payload.tool_name.clone().unwrap_or_default())
        }
        "PostToolUse" | "PostToolUseFailure" => Activity::Thinking,
        "UserPromptSubmit" => Activity::Thinking,
        "PermissionRequest" => Activity::Waiting,
        // Notification is informational — just refresh the timestamp, keep current activity.
        "Notification" => {
            if let Some(session) = state.sessions.get_mut(&payload.pane_id) {
                session.last_event_ts = crate::state::unix_now();
                if let Some(ts_ms) = payload.ts_ms {
                    session.last_ts_ms = ts_ms;
                }
            }
            return;
        }
        "Stop" => Activity::Done,
        "SubagentStop" => Activity::AgentDone,
        _ => Activity::Idle,
    };

    let (tab_index, tab_name) = state
        .pane_to_tab
        .get(&payload.pane_id)
        .cloned()
        .unzip();

    let session = state
        .sessions
        .entry(payload.pane_id)
        .or_insert_with(|| SessionInfo {
            session_id: payload.session_id.clone().unwrap_or_default(),
            pane_id: payload.pane_id,
            activity: Activity::Init,
            tab_name: None,
            tab_index: None,
            last_event_ts: 0,
            cwd: None,
            last_ts_ms: 0,
            background: Default::default(),
            background_ts_ms: 0,
            color: None,
        });

    if matches!(activity, Activity::Waiting) {
        match state.settings.flash {
            FlashMode::Once => {
                state.flash_deadlines.insert(
                    payload.pane_id,
                    crate::state::unix_now_ms() + crate::state::FLASH_DURATION_MS,
                );
            }
            FlashMode::Persist => {
                state.flash_deadlines.insert(payload.pane_id, u64::MAX);
            }
            FlashMode::Off => {}
        }
        // Desktop notification is handled by the hook script to avoid
        // duplicates from multiple plugin instances.
    } else {
        state.flash_deadlines.remove(&payload.pane_id);
    }

    if event == "SessionStart" {
        session.background = Default::default();
        session.background_ts_ms = 0;
    }
    session.activity = activity;
    session.last_event_ts = crate::state::unix_now();
    if let Some(ts_ms) = payload.ts_ms {
        session.last_ts_ms = ts_ms;
    }
    if let Some(sid) = &payload.session_id {
        session.session_id = sid.clone();
    }
    if let Some(cwd) = payload.cwd {
        session.cwd = Some(cwd);
    }
    if let Some(color) = payload.color {
        session.color = (color != "default").then_some(color);
    }
    if let Some((idx, name)) = tab_index.zip(tab_name) {
        session.tab_index = Some(idx);
        session.tab_name = Some(name);
    }
}
