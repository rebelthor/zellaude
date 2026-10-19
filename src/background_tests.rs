use crate::event_handler::handle_hook_event;
use crate::state::{Activity, BackgroundActivity, HookPayload, State};
use serde_json::json;

fn event(state: &mut State, name: &str, ts: u64, session: &str, counts: Option<BackgroundActivity>) {
    let payload: HookPayload = serde_json::from_value(json!({
        "pane_id": 1, "session_id": session, "hook_event": name,
        "ts_ms": ts, "background": counts
    })).unwrap();
    handle_hook_event(state, payload);
}

#[test]
fn badges_prioritize_and_bound_counts() {
    assert_eq!(BackgroundActivity::default().badge(), "");
    let mut counts = BackgroundActivity { agents: 0, jobs: 0, watchers: 3 };
    assert_eq!(counts.badge(), "w3");
    counts.jobs = 1;
    assert_eq!(counts.badge(), "j");
    counts.agents = 2;
    assert_eq!(counts.badge(), "a2");
    counts.agents = 10;
    assert_eq!(counts.badge(), "a9+");
    counts.add(BackgroundActivity { agents: u32::MAX, jobs: 0, watchers: 0 });
    assert_eq!(counts.badge(), "a9+");
}

#[test]
fn background_and_foreground_have_independent_clocks() {
    let mut state = State::default();
    event(&mut state, "SessionStart", 100, "one", None);
    event(&mut state, "PermissionRequest", 200, "one", None);
    let elapsed = state.sessions[&1].last_event_ts;
    let flash = state.flash_deadlines[&1];
    let counts = BackgroundActivity { agents: 2, jobs: 1, watchers: 3 };
    event(&mut state, "BackgroundStatus", 300, "one", Some(counts));
    assert_eq!(state.sessions[&1].activity, Activity::Waiting);
    assert_eq!(state.sessions[&1].last_event_ts, elapsed);
    assert_eq!(state.flash_deadlines[&1], flash);
    event(&mut state, "Stop", 250, "one", None);
    assert_eq!(state.sessions[&1].activity, Activity::Done);
    assert_eq!(state.sessions[&1].background, counts);
    event(&mut state, "BackgroundStatus", 290, "one", Some(BackgroundActivity::default()));
    assert_eq!(state.sessions[&1].background, counts);
    event(&mut state, "BackgroundStatus", 310, "one", Some(BackgroundActivity::default()));
    assert_eq!(state.sessions[&1].background.badge(), "");
    assert_eq!(state.sessions[&1].activity, Activity::Done);
}

#[test]
fn late_background_cannot_resurrect_or_cross_sessions() {
    let mut state = State::default();
    let counts = Some(BackgroundActivity { agents: 1, jobs: 0, watchers: 0 });
    event(&mut state, "BackgroundStatus", 90, "one", counts);
    assert!(state.sessions.is_empty());
    event(&mut state, "SessionStart", 100, "one", None);
    event(&mut state, "BackgroundStatus", 110, "one", counts);
    event(&mut state, "SessionEnd", 120, "one", None);
    event(&mut state, "BackgroundStatus", 130, "one", counts);
    assert!(state.sessions.is_empty());
    event(&mut state, "SessionStart", 140, "two", None);
    event(&mut state, "BackgroundStatus", 150, "one", counts);
    assert_eq!(state.sessions[&1].background.badge(), "");
    event(&mut state, "BackgroundStatus", 160, "two", counts);
    assert_eq!(state.sessions[&1].background.badge(), "a");
}

#[test]
fn syncing_foreground_preserves_newer_background_and_session_identity() {
    let mut state = State::default();
    event(&mut state, "SessionStart", 100, "one", None);
    let mut older = state.sessions[&1].clone();
    let counts = BackgroundActivity { agents: 2, jobs: 0, watchers: 0 };
    event(&mut state, "BackgroundStatus", 200, "one", Some(counts));
    let mut newer = state.sessions[&1].clone();
    newer.merge_background(&older);
    assert_eq!(newer.background, counts);
    older.merge_background(&newer);
    assert_eq!(older.background, counts);
    assert_eq!(older.background_ts_ms, 200);
    older.session_id = "another".into();
    older.background_ts_ms = 0;
    older.background = BackgroundActivity::default();
    older.merge_background(&newer);
    assert_eq!(older.background, BackgroundActivity::default());
}

#[test]
fn old_hook_payloads_remain_valid() {
    let mut state = State::default();
    let payload = serde_json::from_value(json!({"pane_id": 1, "hook_event": "Stop"})).unwrap();
    handle_hook_event(&mut state, payload);
    assert_eq!(state.sessions[&1].activity, Activity::Done);
    assert_eq!(state.sessions[&1].background.badge(), "");
}

#[test]
fn session_color_sets_keeps_and_resets() {
    let mut state = State::default();
    fn send(state: &mut State, name: &str, ts: u64, color: Option<&str>) {
        let payload: HookPayload = serde_json::from_value(json!({
            "pane_id": 1, "session_id": "one", "hook_event": name, "ts_ms": ts, "color": color
        })).unwrap();
        handle_hook_event(state, payload);
    }
    send(&mut state, "SessionStart", 100, Some("purple"));
    send(&mut state, "PreToolUse", 200, None);
    send(&mut state, "UserPromptSubmit", 300, Some("default"));
    assert_eq!(state.sessions[&1].color, None);
    send(&mut state, "Stop", 400, Some("cyan"));
    assert_eq!(state.sessions[&1].color.as_deref(), Some("cyan"));
}

#[test]
fn background_snapshot_carries_session_color() {
    let mut state = State::default();
    event(&mut state, "SessionStart", 100, "one", None);
    let payload: HookPayload = serde_json::from_value(json!({
        "pane_id": 1, "session_id": "one", "hook_event": "BackgroundStatus", "ts_ms": 200,
        "background": BackgroundActivity::default(), "color": "orange"
    })).unwrap();
    handle_hook_event(&mut state, payload);
    assert_eq!(state.sessions[&1].color.as_deref(), Some("orange"));
    assert_eq!(state.sessions[&1].activity, Activity::Init);
}

#[test]
fn closed_mark_holds_only_from_stop_until_the_next_event() {
    let mut state = State::default();
    let closed = |state: &mut State, name: &str, ts: u64, flag: bool| {
        let payload: HookPayload = serde_json::from_value(json!({
            "pane_id": 1, "session_id": "one", "hook_event": name, "ts_ms": ts, "closed": flag
        })).unwrap();
        handle_hook_event(state, payload);
    };
    event(&mut state, "SessionStart", 100, "one", None);
    assert!(!state.sessions[&1].closed);
    closed(&mut state, "Stop", 200, true);
    assert!(state.sessions[&1].closed);
    event(&mut state, "BackgroundStatus", 250, "one", Some(BackgroundActivity::default()));
    assert!(state.sessions[&1].closed);
    closed(&mut state, "UserPromptSubmit", 300, true);
    assert!(!state.sessions[&1].closed);
}

#[test]
fn snapshot_sets_closed_only_after_the_last_foreground_event() {
    let mut state = State::default();
    let snap = |state: &mut State, ts: u64, flag: bool| {
        let payload: HookPayload = serde_json::from_value(json!({
            "pane_id": 1, "session_id": "one", "hook_event": "BackgroundStatus", "ts_ms": ts,
            "background": BackgroundActivity::default(), "closed": flag
        })).unwrap();
        handle_hook_event(state, payload);
    };
    event(&mut state, "Stop", 200, "one", None);
    snap(&mut state, 150, true);
    assert!(!state.sessions[&1].closed);
    snap(&mut state, 300, true);
    assert!(state.sessions[&1].closed);
    snap(&mut state, 400, false);
    assert!(!state.sessions[&1].closed);
}

fn prompt(state: &mut State, session: &str, ts: u64, command: Option<&str>) {
    let payload: HookPayload = serde_json::from_value(json!({
        "pane_id": 1, "session_id": session, "hook_event": "UserPromptSubmit",
        "ts_ms": ts, "command": command
    })).unwrap();
    handle_hook_event(state, payload);
}

#[test]
fn slash_command_is_stored_and_survives_typed_prompts() {
    let mut state = State::default();
    prompt(&mut state, "s1", 100, Some("review-requested-prs"));
    assert_eq!(state.sessions[&1].command.as_deref(), Some("review-requested-prs"));
    // A typed prompt (command null) keeps it; the AI title takes over via the pane title.
    prompt(&mut state, "s1", 200, None);
    assert_eq!(state.sessions[&1].command.as_deref(), Some("review-requested-prs"));
    prompt(&mut state, "s1", 300, Some("other"));
    assert_eq!(state.sessions[&1].command.as_deref(), Some("other"));
}

#[test]
fn command_resets_for_a_new_session_in_the_same_pane() {
    let mut state = State::default();
    prompt(&mut state, "s1", 100, Some("review-requested-prs"));
    event(&mut state, "SessionStart", 200, "s1", None);
    assert_eq!(state.sessions[&1].command, None);
    prompt(&mut state, "s1", 300, Some("x"));
    event(&mut state, "PreToolUse", 400, "s2", None);
    assert_eq!(state.sessions[&1].command, None);
}

#[test]
fn older_payloads_without_command_still_deserialize() {
    let payload: HookPayload = serde_json::from_value(json!({
        "pane_id": 1, "session_id": "s", "hook_event": "UserPromptSubmit"
    })).unwrap();
    assert_eq!(payload.command, None);
}
