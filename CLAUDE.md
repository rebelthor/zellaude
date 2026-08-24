# zellaude

Zellij plugin (WASM) that replaces the tab bar with per-tab Claude Code activity
status. This clone is `rebelthor/zellaude`, a fork of `ishefi/zellaude` that adds
the opt-in "Tab titles" setting on branch `feat/tab-titles`.

The fork is standalone: all work stays on `rebelthor/zellaude` and is not
proposed upstream. `feat/tab-titles` is the working branch and ships the binary
in use.

## Always push

**Commit and push every change to `origin` in the same session you make it.**

This directory is `~/src/zellaude`, which is outside the Syncthing tree and has
no backup. It has already been lost once, taking the only reproducible source of
the deployed `.wasm` with it. An unpushed commit here is one `rm` from gone, so
`origin` is the only durable copy.

Local-only commits are not enough: check `git status -sb` for an `ahead` marker
before considering work finished.

## Live session safety

NEVER experiment in an attached work session. Treat every session containing active Claude or
work tabs as production state.

For plugin testing, permission prompts, pipe probes, reloads, focus changes, key injection, or
tab and pane cleanup:

- Work from a terminal outside Zellij.
- Create a dedicated test session, for example with `zellij attach --create zellaude-test`.
- Pass `--session zellaude-test` to every command that can affect that session. For actions, use
  `zellij --session zellaude-test action ...`.
- Inspect the explicitly targeted session before any side effect.
- Never infer the target from current focus, tab position, or the session inherited by the shell.
- If a separate session cannot be created and targeted explicitly, stop. Do not fall back to a
  live work session.
- Clean up only the named test session after verifying that no work tabs are in it.

## Hook portability

The hook selects GNU `timeout` on Fedora and Homebrew `gtimeout` on macOS. Keep that selection
portable when changing the hook.

## Build and test

`cargo` is not on `PATH`; it lives at
`~/.rustup/toolchains/stable-aarch64-apple-darwin/bin/cargo`.

```bash
cargo build --release   # produces target/wasm32-wasip1/release/zellaude.wasm
```

`.cargo/config.toml` already defaults `--target` to `wasm32-wasip1`, so a bare
build emits the WASM artifact. No need to pass `--target` by hand.

Tests need both the host target and `--lib`:

```bash
cargo test --release --target aarch64-apple-darwin --lib
```

The plugin bin links wasm-only zellij host imports and **cannot** link for the
host, so `cargo test` without `--lib` fails at the linker with undefined
`_host_run_plugin_command`. Anything worth unit-testing therefore belongs in the
lib target (`src/lib.rs`), not in the bin.

## Deploy locally

The running plugin is a synced copy, not this build directory:

```bash
cp target/wasm32-wasip1/release/zellaude.wasm \
   ~/sync/projects/zellij-config/plugins/zellaude.wasm
```

`~/.config/zellij/plugins/zellaude.wasm` is a symlink to that path. Back up the
previous `.wasm` alongside it before overwriting, and record the new sha256 in
`~/sync/projects/zellij-config/CLAUDE.md`, that file is the source of truth for
which build is deployed, and a stale hash there has caused real confusion.

A running zellij server keeps the old WASM in memory. To pick up a new build
without killing the session:

```bash
zellij --session <name> action start-or-reload-plugin \
  file:$HOME/.config/zellij/plugins/zellaude.wasm
```

Prefer that over a restart. Killing the server kills every pane's process,
including any Claude agent mid-task; tabs and layout come back from the
serialized layout, in-flight agent work does not.

## Tab renames are guarded on purpose

`apply_tab_titles()` in `src/main.rs` looks over-defensive. It is not. See the
doc comment there for the full account.

`rename_tab` addresses tabs by *position* and zellij exposes no stable per-tab
ID. Upstream [zellij#3535](https://github.com/zellij-org/zellij/issues/3535)
drifts positions after a tab closes, so a rename can land nowhere, leaving
`tab.name` unchanged. A naive "rename whenever the name differs" rule reads that
as still-needs-renaming and re-issues it; each attempt provokes another
`TabUpdate`, which is a feedback loop that pegs the server and ends in `SIGABRT`
or `EMFILE`.

Three brakes prevent that: a stale-position guard, a bounded retry budget, and a
rate limit between batches. The pure rules live in `src/rename_guard.rs` with
unit tests.

Do not remove or loosen these while #3535 is open. It is still open, and the
crash reproduced on zellij 0.44.3, which already contains the PR credited with
fixing it. If renames feel sluggish, `RENAME_COOLDOWN_MS` is the knob, not
removal.
