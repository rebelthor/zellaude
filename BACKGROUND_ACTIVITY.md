# Background activity badges

A tab keeps its foreground icon and appends one ASCII badge: `a` for agents, `j` for jobs, or `w` for passive watchers, in that priority order. Counts appear above one and cap at `9+`. Counts aggregate across every tracked pane in the tab. The sidebar remains the detailed view.

The Pi bridge sends this optional snapshot through the existing `zellaude` pipe:

```json
{
  "pane_id": 1,
  "session_id": "current-session-id",
  "hook_event": "BackgroundStatus",
  "ts_ms": 1780000000000,
  "background": { "agents": 2, "jobs": 1, "watchers": 3 }
}
```

Send `SessionStart` first. The snapshot must match the pane's current session. Its timestamp is ordered independently of foreground events. Foreground completion preserves background counts; a zero snapshot clears the badge. Snapshots do not change attention, notifications, or foreground elapsed time. Older clients need no changes.

The Pi bridge uses the public current-session subagent fleet API for recovery and the task sidebar's existing background-job and watcher feed. It sends changed counts and repeats unchanged counts every ten seconds. These are last-reported counts, not a liveness guarantee if the bridge disappears without `SessionEnd`.

## Verification

```sh
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo test --release --target aarch64-apple-darwin --lib
PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH" cargo build --release --target wasm32-wasip1
```

Do not use `install.sh` for a trial: it overwrites the installed plugin. Test the artifact at `target/wasm32-wasip1/release/zellaude.wasm` in a dedicated Zellij session, started from a terminal outside Zellij. Never reload or replace plugins in attached work sessions. See `CLAUDE.md` for the session-safety rules.
