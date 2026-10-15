use crate::state::{
    unix_now, unix_now_ms, Activity, ClickRegion, FlashMode, MenuAction, MenuClickRegion,
    NotifyMode, SessionInfo, SettingKey, State, ViewMode,
};
use std::fmt::Write;
use std::io::Write as IoWrite;
use zellij_tile::prelude::{InputMode, TabInfo};

struct Style {
    symbol: &'static str,
    r: u8,
    g: u8,
    b: u8,
}

fn activity_priority(activity: &Activity) -> u8 {
    match activity {
        Activity::Waiting => 8,
        Activity::Tool(_) => 7,
        Activity::Thinking => 6,
        Activity::Prompting => 5,
        Activity::Notification => 4,
        Activity::Init => 3,
        Activity::Done => 2,
        Activity::AgentDone => 1,
        Activity::Idle => 0,
    }
}

fn activity_style(activity: &Activity) -> Style {
    match activity {
        Activity::Init => Style { symbol: "◆", r: 180, g: 175, b: 195 },
        Activity::Thinking => Style { symbol: "●", r: 180, g: 140, b: 255 },
        Activity::Tool(name) => {
            let symbol = match name.as_str() {
                "Bash" => "⚡",
                "Read" | "Glob" | "Grep" => "◉",
                "Edit" | "Write" => "✎",
                "Task" => "⊜",
                "WebSearch" | "WebFetch" => "◈",
                _ => "⚙",
            };
            Style { symbol, r: 255, g: 170, b: 50 }
        }
        Activity::Prompting => Style { symbol: "▶", r: 80, g: 200, b: 120 },
        Activity::Waiting => Style { symbol: "⚠", r: 255, g: 60, b: 60 },
        Activity::Notification => Style { symbol: "◇", r: 200, g: 200, b: 100 },
        Activity::Done => Style { symbol: "✓", r: 80, g: 200, b: 120 },
        Activity::AgentDone => Style { symbol: "✓", r: 80, g: 180, b: 100 },
        Activity::Idle => Style { symbol: "○", r: 180, g: 175, b: 195 },
    }
}

fn fg(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[38;2;{r};{g};{b}m")
}

fn bg(r: u8, g: u8, b: u8) -> String {
    format!("\x1b[48;2;{r};{g};{b}m")
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const ELAPSED_THRESHOLD: u64 = 30;
const SEPARATOR: &str = "\u{e0b0}";

type Color = (u8, u8, u8);
const BAR_BG: Color = (30, 30, 46);
const PREFIX_BG: Color = (60, 50, 80);
const PREFIX_BG_SETTINGS: Color = (100, 70, 140);
const TAB_BG_ACTIVE: Color = (140, 100, 200);
const TAB_BG_INACTIVE: Color = (80, 75, 110);
const FLASH_BG_BRIGHT: Color = (80, 80, 30);
/// How far apart two colors must be (sum of channel differences) for a status
/// symbol to stay readable on a colored tab.
const MIN_SYMBOL_CONTRAST: u16 = 160;

/// A status symbol keeps its color unless it would vanish into the tab
/// background (an orange tool symbol on an orange tab); then it turns white.
fn readable_on(sym: Color, tab: Color) -> Color {
    let d = |a: u8, b: u8| (a as i16 - b as i16).unsigned_abs();
    if d(sym.0, tab.0) + d(sym.1, tab.1) + d(sym.2, tab.2) < MIN_SYMBOL_CONTRAST {
        (255, 255, 255)
    } else {
        sym
    }
}

/// Tab background for a Claude Code `/color` name, dark enough for white text.
fn session_color(name: &str) -> Option<Color> {
    match name {
        "red" => Some((190, 55, 60)),
        "blue" => Some((55, 105, 215)),
        "green" => Some((40, 140, 75)),
        "yellow" => Some((165, 135, 20)),
        "purple" => Some((135, 85, 200)),
        "orange" => Some((205, 105, 35)),
        "pink" => Some((200, 75, 145)),
        "cyan" => Some((25, 140, 160)),
        _ => None,
    }
}

/// Inactive tabs keep their hue but sink toward the bar background.
fn dim(c: Color) -> Color {
    let mix = |a: u8, b: u8| ((a as u16 * 45 + b as u16 * 55) / 100) as u8;
    (mix(c.0, BAR_BG.0), mix(c.1, BAR_BG.1), mix(c.2, BAR_BG.2))
}

/// Write a powerline arrow: fg=from_bg, bg=to_bg, then separator char.
fn arrow(buf: &mut String, col: &mut usize, from: Color, to: Color) {
    let _ = write!(
        buf,
        "{}{}{SEPARATOR}",
        fg(from.0, from.1, from.2),
        bg(to.0, to.1, to.2),
    );
    *col += 1;
}

fn format_elapsed(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

fn mode_style(mode: InputMode) -> (Color, &'static str) {
    match mode {
        InputMode::Normal => ((80, 200, 120), "NORMAL"),
        InputMode::Locked => ((255, 80, 80), "LOCKED"),
        InputMode::Pane => ((80, 180, 255), "PANE"),
        InputMode::Tab => ((180, 140, 255), "TAB"),
        InputMode::Resize => ((255, 170, 50), "RESIZE"),
        InputMode::Move => ((255, 170, 50), "MOVE"),
        InputMode::Scroll => ((200, 200, 100), "SCROLL"),
        InputMode::EnterSearch => ((200, 200, 100), "SEARCH"),
        InputMode::Search => ((200, 200, 100), "SEARCH"),
        InputMode::RenameTab => ((200, 200, 100), "RENAME"),
        InputMode::RenamePane => ((200, 200, 100), "RENAME"),
        InputMode::Session => ((180, 140, 255), "SESSION"),
        InputMode::Prompt => ((80, 200, 120), "PROMPT"),
        InputMode::Tmux => ((80, 200, 120), "TMUX"),
    }
}

pub fn render_status_bar(state: &mut State, _rows: usize, cols: usize) {
    state.click_regions.clear();
    state.menu_click_regions.clear();

    let mut buf = String::with_capacity(cols * 4);
    // Terminal setup for a 1-row status bar:
    //  \x1b[H     — cursor home (prevent scroll from cursor at end-of-line)
    //  \x1b[?7l   — disable auto-wrap (clip overflow instead of scroll)
    //  \x1b[?25l  — hide cursor
    buf.push_str("\x1b[H\x1b[?7l\x1b[?25l");
    let bar_bg_str = bg(BAR_BG.0, BAR_BG.1, BAR_BG.2);

    // Bail early if terminal is too narrow
    if cols < 5 {
        let _ = write!(buf, "{bar_bg_str}{:width$}{RESET}", "", width = cols);
        print!("{buf}");
        let _ = std::io::stdout().flush();
        return;
    }

    let prefix_bg = if state.view_mode == ViewMode::Settings {
        PREFIX_BG_SETTINGS
    } else {
        PREFIX_BG
    };

    // Build prefix: " Zellaude (session) MODE "
    let (mode_bg, mode_text) = mode_style(state.input_mode);
    let show_mode = state.settings.mode_indicator;
    let session_part = match state.zellij_session_name.as_deref() {
        Some(name) => format!(" ({name})"),
        None => String::new(),
    };
    let prefix_text = format!(" Zellaude{session_part} ");
    let prefix_width = display_width(&prefix_text);
    let mode_pill_width = if show_mode { 1 + mode_text.len() + 1 } else { 0 };
    let total_prefix_width = prefix_width + mode_pill_width;

    // Render prefix segment (truncate if wider than cols)
    let mut col;
    if total_prefix_width <= cols {
        let _ = write!(
            buf,
            "{}{}{BOLD}{prefix_text}{RESET}",
            bg(prefix_bg.0, prefix_bg.1, prefix_bg.2),
            fg(255, 255, 255),
        );
        if show_mode {
            let _ = write!(
                buf,
                "{}{}{BOLD} {mode_text} {RESET}",
                bg(mode_bg.0, mode_bg.1, mode_bg.2),
                fg(30, 30, 46),
            );
        }
        col = total_prefix_width;
    } else if prefix_width <= cols {
        // Fit the name part but skip mode pill
        let _ = write!(
            buf,
            "{}{}{BOLD}{prefix_text}{RESET}",
            bg(prefix_bg.0, prefix_bg.1, prefix_bg.2),
            fg(255, 255, 255),
        );
        col = prefix_width;
    } else {
        // Even name doesn't fit — just show what we can
        let avail = cols.saturating_sub(2); // leave room for fill
        let short: String = prefix_text.chars().take(avail).collect();
        let _ = write!(
            buf,
            "{}{}{BOLD}{short}{RESET}",
            bg(prefix_bg.0, prefix_bg.1, prefix_bg.2),
            fg(255, 255, 255),
        );
        col = display_width(&short);
    }
    state.prefix_click_region = Some((0, col));

    let last_prefix_bg = if show_mode && total_prefix_width <= cols { mode_bg } else { prefix_bg };
    let prefix_used = col;

    if col < cols {
        match state.view_mode {
            ViewMode::Normal => {
                render_tabs(state, &mut buf, &mut col, cols, last_prefix_bg, prefix_used);
            }
            ViewMode::Settings => {
                arrow(&mut buf, &mut col, last_prefix_bg, BAR_BG);
                let _ = write!(buf, "{bar_bg_str}");
                render_settings_menu(state, &mut buf, &mut col);
            }
        }
    }

    // Fill remaining width with bar background — never exceed cols
    if col < cols {
        let remaining = cols - col;
        let _ = write!(buf, "{bar_bg_str}{:width$}", "", width = remaining);
    }
    let _ = write!(buf, "{RESET}");

    print!("{buf}");
    let _ = std::io::stdout().flush();
}

fn render_tabs(
    state: &mut State,
    buf: &mut String,
    col: &mut usize,
    cols: usize,
    prefix_bg: Color,
    prefix_width: usize,
) {
    let now_s = unix_now();
    let now_ms = unix_now_ms();

    // Sort tabs by position
    let mut tabs: Vec<&TabInfo> = state.tabs.iter().collect();
    tabs.sort_by_key(|t| t.position);

    let count = tabs.len();
    if count == 0 {
        arrow(buf, col, prefix_bg, BAR_BG);
        return;
    }

    // For each tab, find the best (highest-priority) Claude session
    let best_sessions: Vec<Option<&SessionInfo>> = tabs
        .iter()
        .map(|tab| {
            state
                .sessions
                .values()
                .filter(|s| s.tab_index == Some(tab.position))
                .max_by_key(|s| activity_priority(&s.activity))
        })
        .collect();

    // Aggregate every pane in a tab, not only the foreground-status winner.
    let background_badges: Vec<String> = tabs
        .iter()
        .map(|tab| {
            let mut total = crate::state::BackgroundActivity::default();
            for session in state.sessions.values().filter(|s| s.tab_index == Some(tab.position)) {
                total.add(session.background);
            }
            total.badge()
        })
        .collect();

    // Pre-compute elapsed strings (only for Claude tabs)
    let elapsed_strs: Vec<Option<String>> = best_sessions
        .iter()
        .map(|session: &Option<&SessionInfo>| {
            if !state.settings.elapsed_time {
                return None;
            }
            session.and_then(|s| {
                let elapsed = now_s.saturating_sub(s.last_event_ts);
                if elapsed >= ELAPSED_THRESHOLD {
                    Some(format_elapsed(elapsed))
                } else {
                    None
                }
            })
        })
        .collect();

    // Reserve space for arrows and the fixed content of each tab. When the
    // full titles fit, tabs keep their natural width. Otherwise, titles share
    // the smaller available area.
    let fixed_widths: Vec<usize> = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let session_width = if best_sessions[i].is_some() { 4 } else { 2 };
            let elapsed_width = elapsed_strs[i].as_ref().map_or(0, |s| 1 + display_width(s));
            let fullscreen_width = if tab.is_fullscreen_active { 2 } else { 0 };
            session_width + elapsed_width + fullscreen_width + background_badges[i].len()
        })
        .collect();
    let fixed_width = fixed_widths.iter().sum::<usize>();
    let arrows_width = 2 * count;
    let available_name_width = cols.saturating_sub(prefix_width + arrows_width + fixed_width);
    let natural_name_widths: Vec<usize> = tabs.iter().map(|tab| display_width(&tab.name)).collect();
    let natural_name_width = natural_name_widths.iter().sum::<usize>();
    let name_widths = if natural_name_width <= available_name_width {
        natural_name_widths
    } else {
        vec![available_name_width / count; count]
    };

    let mut prev_bg = prefix_bg;

    for (i, tab) in tabs.iter().enumerate() {
        // Stop if we'd overflow — need room for at least arrow + closing arrow
        let arrows_needed = if prev_bg == prefix_bg { 1 } else { 2 };
        if *col + arrows_needed + fixed_widths[i] > cols {
            break;
        }

        let session = best_sessions[i];
        let is_claude = session.is_some();
        let tab_name = &tab.name;

        // Truncate name
        let max_name_len = name_widths[i];
        let char_count = tab_name.chars().count();
        let truncated = if max_name_len == 0 {
            String::new()
        } else if char_count > max_name_len {
            let s: String = tab_name.chars().take(max_name_len.saturating_sub(1)).collect();
            format!("{s}…")
        } else {
            tab_name.to_string()
        };

        // Check flash for any session in this tab
        let is_flash_bright = state
            .sessions
            .values()
            .filter(|s| s.tab_index == Some(tab.position))
            .any(|s| {
                state
                    .flash_deadlines
                    .get(&s.pane_id)
                    .map(|&deadline| now_ms < deadline && (now_ms / 250) % 2 == 0)
                    .unwrap_or(false)
            });

        let is_active = tab.active;

        let color = session.and_then(|s| s.color.as_deref()).and_then(session_color);

        // Pick tab background color
        let tab_bg = match (is_flash_bright, color) {
            (true, _) => FLASH_BG_BRIGHT,
            (false, Some(c)) if is_active => c,
            (false, Some(c)) => dim(c),
            (false, None) if is_active => TAB_BG_ACTIVE,
            (false, None) => TAB_BG_INACTIVE,
        };

        // Arrow: close previous segment, then open this tab
        if prev_bg == prefix_bg {
            arrow(buf, col, prev_bg, tab_bg);
        } else {
            arrow(buf, col, prev_bg, BAR_BG);
            arrow(buf, col, BAR_BG, tab_bg);
        }

        let tab_bg_str = bg(tab_bg.0, tab_bg.1, tab_bg.2);
        let region_start = *col;

        if is_claude {
            let s = session.unwrap();
            let style = activity_style(&s.activity);

            let (sym_fg, name_fg, name_bold) = if is_flash_bright {
                (fg(255, 255, 80), fg(255, 255, 80), true)
            } else if color.is_some() {
                let (r, g, b) = readable_on((style.r, style.g, style.b), tab_bg);
                let name = if is_active { fg(255, 255, 255) } else { fg(225, 222, 235) };
                (fg(r, g, b), name, is_active)
            } else if is_active {
                (fg(style.r, style.g, style.b), fg(255, 255, 255), true)
            } else {
                (fg(style.r, style.g, style.b), fg(120, 220, 220), false)
            };

            // Leading space
            let _ = write!(buf, "{tab_bg_str} ");
            *col += 1;

            // Symbol
            let _ = write!(buf, "{sym_fg}{}", style.symbol);
            *col += display_width(style.symbol);

            // Adjacent ASCII badge adds one column for a single background run.
            let badge = &background_badges[i];
            let _ = write!(buf, "{}{badge}", fg(200, 190, 240));
            *col += badge.len();

            // Space + name
            if !truncated.is_empty() {
                let bold_str = if name_bold { BOLD } else { "" };
                let _ = write!(buf, " {bold_str}{name_fg}{truncated}{RESET}{tab_bg_str}");
                *col += 1 + display_width(&truncated);
            }
            // Elapsed suffix
            if let Some(ref es) = elapsed_strs[i] {
                if *col + 1 + es.len() + 1 < cols {
                    let _ = write!(buf, " {}{es}", fg(165, 160, 180));
                    *col += 1 + es.len();
                }
            }

            // Fullscreen indicator
            if tab.is_fullscreen_active && *col + 3 < cols {
                let _ = write!(buf, " {}F{RESET}{tab_bg_str}", fg(255, 200, 60));
                *col += 2;
            }

            // Trailing space
            let _ = write!(buf, " ");
            *col += 1;

            // Click region: if any session is waiting, use its pane_id for focus
            let waiting_session = state
                .sessions
                .values()
                .filter(|s| s.tab_index == Some(tab.position))
                .find(|s| matches!(s.activity, Activity::Waiting));

            state.click_regions.push(ClickRegion {
                start_col: region_start,
                end_col: *col,
                tab_index: tab.position,
                pane_id: waiting_session.map_or(0, |s| s.pane_id),
                is_waiting: waiting_session.is_some(),
            });
        } else {
            // Non-Claude tab: dimmer, no symbol
            let name_fg = if is_active {
                fg(220, 215, 230)
            } else {
                fg(170, 165, 185)
            };
            let name_bold = is_active;

            // Leading space
            let _ = write!(buf, "{tab_bg_str} ");
            *col += 1;

            // Name only (no symbol)
            if !truncated.is_empty() {
                let bold_str = if name_bold { BOLD } else { "" };
                let _ = write!(buf, "{bold_str}{name_fg}{truncated}{RESET}{tab_bg_str}");
                *col += display_width(&truncated);
            }
            // Fullscreen indicator
            if tab.is_fullscreen_active && *col + 3 < cols {
                let _ = write!(buf, " {}F{RESET}{tab_bg_str}", fg(255, 200, 60));
                *col += 2;
            }

            // Trailing space
            let _ = write!(buf, " ");
            *col += 1;

            state.click_regions.push(ClickRegion {
                start_col: region_start,
                end_col: *col,
                tab_index: tab.position,
                pane_id: 0,
                is_waiting: false,
            });
        }

        prev_bg = tab_bg;
    }

    // Arrow from last tab → bar background (only if we rendered any tabs)
    if prev_bg != prefix_bg || count > 0 {
        arrow(buf, col, prev_bg, BAR_BG);
    }
}

fn notify_mode_label(mode: NotifyMode) -> (&'static str, &'static str, String, String) {
    match mode {
        NotifyMode::Always => ("●", "Notify: always", fg(80, 200, 120), fg(255, 255, 255)),
        NotifyMode::Unfocused => ("◐", "Notify: unfocused", fg(255, 200, 60), fg(255, 200, 60)),
        NotifyMode::Never => ("○", "Notify: off", fg(100, 100, 100), fg(100, 100, 100)),
    }
}

fn flash_mode_label(mode: FlashMode) -> (&'static str, &'static str, String, String) {
    match mode {
        FlashMode::Persist => ("●", "Flash: persist", fg(80, 200, 120), fg(255, 255, 255)),
        FlashMode::Once => ("◐", "Flash: brief", fg(255, 200, 60), fg(255, 200, 60)),
        FlashMode::Off => ("○", "Flash: off", fg(100, 100, 100), fg(100, 100, 100)),
    }
}

/// Render a three-state toggle and register its click region.
/// Assumes the caller has already set the desired background color.
fn render_tristate(
    buf: &mut String,
    col: &mut usize,
    state_regions: &mut Vec<MenuClickRegion>,
    key: SettingKey,
    symbol: &str,
    label: &str,
    sym_color: &str,
    label_color: &str,
) {
    let region_start = *col;
    let width = display_width(symbol) + 1 + label.len();
    *col += width;

    state_regions.push(MenuClickRegion {
        start_col: region_start,
        end_col: *col,
        action: MenuAction::ToggleSetting(key),
    });

    let _ = write!(buf, "{sym_color}{symbol} {label_color}{label}");
}

fn render_settings_menu(state: &mut State, buf: &mut String, col: &mut usize) {
    // Leading space after arrow
    let _ = write!(buf, " ");
    *col += 1;

    // --- Notifications (three-state) ---
    {
        let (symbol, label, sym_color, label_color) =
            notify_mode_label(state.settings.notifications);
        render_tristate(
            buf, col, &mut state.menu_click_regions,
            SettingKey::Notifications, symbol, label, &sym_color, &label_color,
        );
    }

    // --- Flash (three-state) ---
    {
        let _ = write!(buf, "  ");
        *col += 2;
        let (symbol, label, sym_color, label_color) =
            flash_mode_label(state.settings.flash);
        render_tristate(
            buf, col, &mut state.menu_click_regions,
            SettingKey::Flash, symbol, label, &sym_color, &label_color,
        );
    }

    // --- Elapsed time (bool) ---
    {
        let _ = write!(buf, "  ");
        *col += 2;
        let enabled = state.settings.elapsed_time;
        let (symbol, sym_color, label_color) = if enabled {
            ("●", fg(80, 200, 120), fg(255, 255, 255))
        } else {
            ("○", fg(100, 100, 100), fg(100, 100, 100))
        };
        let label = if enabled { "Elapsed time: on" } else { "Elapsed time: off" };
        render_tristate(
            buf, col, &mut state.menu_click_regions,
            SettingKey::ElapsedTime, symbol, label, &sym_color, &label_color,
        );
    }

    // --- Mode indicator (bool) ---
    {
        let _ = write!(buf, "  ");
        *col += 2;
        let enabled = state.settings.mode_indicator;
        let (symbol, sym_color, label_color) = if enabled {
            ("●", fg(80, 200, 120), fg(255, 255, 255))
        } else {
            ("○", fg(100, 100, 100), fg(100, 100, 100))
        };
        let label = if enabled { "Mode indicator: on" } else { "Mode indicator: off" };
        render_tristate(
            buf, col, &mut state.menu_click_regions,
            SettingKey::ModeIndicator, symbol, label, &sym_color, &label_color,
        );
    }

    // --- Tab titles (bool) ---
    {
        let _ = write!(buf, "  ");
        *col += 2;
        let enabled = state.settings.tab_titles;
        let (symbol, sym_color, label_color) = if enabled {
            ("●", fg(80, 200, 120), fg(255, 255, 255))
        } else {
            ("○", fg(100, 100, 100), fg(100, 100, 100))
        };
        let label = if enabled { "Tab titles: on" } else { "Tab titles: off" };
        render_tristate(
            buf, col, &mut state.menu_click_regions,
            SettingKey::TabTitles, symbol, label, &sym_color, &label_color,
        );
    }

    // Close button
    let _ = write!(buf, "  ");
    *col += 2;
    let close_start = *col;
    let _ = write!(buf, "{}×", fg(255, 60, 60));
    *col += 1;

    state.menu_click_regions.push(MenuClickRegion {
        start_col: close_start,
        end_col: *col,
        action: MenuAction::CloseMenu,
    });
}

#[cfg(test)]
mod background_render_tests {
    use super::*;
    use crate::state::BackgroundActivity;

    #[test]
    fn badge_includes_other_panes_and_reserves_width() {
        let mut state = State::default();
        state.settings.elapsed_time = false;
        state.tabs.push(TabInfo { position: 0, name: "Long title".into(), ..Default::default() });
        for (pane, activity, background) in [
            (1, Activity::Waiting, BackgroundActivity::default()),
            (2, Activity::Done, BackgroundActivity { agents: 2, jobs: 3, watchers: 4 }),
        ] {
            state.sessions.insert(pane, SessionInfo {
                session_id: pane.to_string(), pane_id: pane, activity,
                tab_name: None, tab_index: Some(0), last_event_ts: 0,
                cwd: None, last_ts_ms: 0, background, background_ts_ms: 0,
                color: None,
            });
        }
        for width in 1..80 {
            state.click_regions.clear();
            let mut buf = String::new();
            let mut col = 0;
            render_tabs(&mut state, &mut buf, &mut col, width, PREFIX_BG, 0);
            assert!(col <= width, "width={width}, col={col}");
            if width >= 8 {
                assert!(buf.contains("a2"));
                assert!(!buf.contains("j3"));
                assert_eq!(state.click_regions[0].pane_id, 1);
                assert!(state.click_regions[0].is_waiting);
            }
        }
    }

    #[test]
    fn colored_session_paints_tab_and_keeps_symbol_readable() {
        let mut state = State::default();
        state.settings.elapsed_time = false;
        state.tabs.push(TabInfo { position: 0, name: "work".into(), active: true, ..Default::default() });
        state.sessions.insert(1, SessionInfo {
            session_id: "1".into(), pane_id: 1, activity: Activity::Done,
            tab_name: None, tab_index: Some(0), last_event_ts: 0,
            cwd: None, last_ts_ms: 0, background: Default::default(), background_ts_ms: 0,
            color: Some("green".into()),
        });
        let mut buf = String::new();
        let mut col = 0;
        render_tabs(&mut state, &mut buf, &mut col, 80, PREFIX_BG, 0);
        assert!(buf.contains(&bg(40, 140, 75)));
        assert!(!buf.contains(&bg(TAB_BG_ACTIVE.0, TAB_BG_ACTIVE.1, TAB_BG_ACTIVE.2)));
        // The green done check would vanish on a green tab, so it turns white.
        assert!(buf.contains(&format!("{}✓", fg(255, 255, 255))));
        assert_eq!(readable_on((180, 140, 255), (55, 105, 215)), (180, 140, 255));
    }
}
