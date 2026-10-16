use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};
use zellij_tile::prelude::*;

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub const FLASH_DURATION_MS: u64 = 2000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Activity {
    Init,
    Thinking,
    Tool(String),
    Prompting,
    Waiting,
    Notification,
    Done,
    AgentDone,
    Idle,
}

impl Activity {
    /// Thinking or running a tool: the tab symbol breathes while this holds.
    pub fn is_working(&self) -> bool {
        matches!(self, Activity::Thinking | Activity::Tool(_))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub pane_id: u32,
    pub activity: Activity,
    pub tab_name: Option<String>,
    pub tab_index: Option<usize>,
    pub last_event_ts: u64,
    pub cwd: Option<String>,
    #[serde(default)]
    pub last_ts_ms: u64,
    #[serde(default)]
    pub background: BackgroundActivity,
    #[serde(default)]
    pub background_ts_ms: u64,
    /// Session color set with Claude Code's `/color`; None means default.
    #[serde(default)]
    pub color: Option<String>,
    /// The session was wrapped up as safe to close; cleared by its next event.
    #[serde(default)]
    pub closed: bool,
}

impl SessionInfo {
    pub fn merge_background(&mut self, other: &Self) {
        if self.session_id == other.session_id && other.background_ts_ms > self.background_ts_ms {
            self.background = other.background;
            self.background_ts_ms = other.background_ts_ms;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BackgroundActivity {
    pub agents: u32,
    pub jobs: u32,
    pub watchers: u32,
}

impl BackgroundActivity {
    pub fn add(&mut self, other: Self) {
        self.agents = self.agents.saturating_add(other.agents);
        self.jobs = self.jobs.saturating_add(other.jobs);
        self.watchers = self.watchers.saturating_add(other.watchers);
    }

    pub fn badge(self) -> String {
        let (kind, count) = if self.agents > 0 {
            ('a', self.agents)
        } else if self.jobs > 0 {
            ('j', self.jobs)
        } else {
            ('w', self.watchers)
        };
        match count {
            0 => String::new(),
            1 => kind.to_string(),
            2..=9 => format!("{kind}{count}"),
            _ => format!("{kind}9+"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct HookPayload {
    pub session_id: Option<String>,
    pub pane_id: u32,
    pub hook_event: String,
    pub tool_name: Option<String>,
    pub cwd: Option<String>,
    pub zellij_session: Option<String>,
    pub term_program: Option<String>,
    pub ts_ms: Option<u64>,
    pub background: Option<BackgroundActivity>,
    pub color: Option<String>,
    /// Safe to close, from a Stop hook or a background snapshot; absent from older senders.
    #[serde(default)]
    pub closed: Option<bool>,
}

/// A tab rename zellaude has issued but not yet seen reflected in a TabUpdate.
///
/// Tracked so a rename that never lands (see the `#3535` note on
/// `apply_tab_titles`) is retried a bounded number of times and then abandoned,
/// instead of being re-issued on every event forever.
#[derive(Debug, Clone)]
pub struct PendingRename {
    pub desired: String,
    pub attempts: u8,
}

pub struct ClickRegion {
    pub start_col: usize,
    pub end_col: usize,
    pub tab_index: usize,
    pub pane_id: u32,
    pub is_waiting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum NotifyMode {
    Never,
    Unfocused,
    #[default]
    Always,
}

impl NotifyMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Always => Self::Unfocused,
            Self::Unfocused => Self::Never,
            Self::Never => Self::Always,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum FlashMode {
    Off,
    #[default]
    Once,
    Persist,
}

impl FlashMode {
    pub fn cycle(self) -> Self {
        match self {
            Self::Once => Self::Persist,
            Self::Persist => Self::Off,
            Self::Off => Self::Once,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub notifications: NotifyMode,
    pub flash: FlashMode,
    pub elapsed_time: bool,
    pub mode_indicator: bool,
    pub tab_titles: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            notifications: NotifyMode::Always,
            flash: FlashMode::Once,
            elapsed_time: true,
            mode_indicator: true,
            tab_titles: false,
        }
    }
}

#[derive(Default, PartialEq)]
pub enum ViewMode {
    #[default]
    Normal,
    Settings,
}

#[derive(Clone, Copy)]
pub enum SettingKey {
    Notifications,
    Flash,
    ElapsedTime,
    ModeIndicator,
    TabTitles,
}

#[derive(Clone, Copy)]
pub enum MenuAction {
    ToggleSetting(SettingKey),
    CloseMenu,
}

pub struct MenuClickRegion {
    pub start_col: usize,
    pub end_col: usize,
    pub action: MenuAction,
}

#[derive(Default)]
pub struct State {
    pub sessions: BTreeMap<u32, SessionInfo>,
    pub pane_to_tab: HashMap<u32, (usize, String)>,
    pub tabs: Vec<TabInfo>,
    /// True after the first TabUpdate; prevents a PaneUpdate arriving first
    /// from treating the not-yet-populated tab map as a mass pane deletion.
    pub tabs_loaded: bool,
    pub pane_manifest: Option<PaneManifest>,
    pub active_tab_index: Option<usize>,
    pub click_regions: Vec<ClickRegion>,
    /// pane_id -> flash deadline in ms (for waiting animation)
    pub flash_deadlines: HashMap<u32, u64>,
    pub zellij_session_name: Option<String>,
    pub term_program: Option<String>,
    pub input_mode: InputMode,
    pub settings: Settings,
    pub view_mode: ViewMode,
    pub prefix_click_region: Option<(usize, usize)>,
    pub menu_click_regions: Vec<MenuClickRegion>,
    pub config_loaded: bool,
    /// Bounded retry state for the config load; see the `Event::Timer` arm.
    pub config_load_attempts: u8,
    pub last_config_load_ts: u64,
    pub hooks_installed: bool,
    /// In-flight tab renames, keyed by the tab name observed when the rename was
    /// issued. See `PendingRename` and `apply_tab_titles`.
    pub pending_renames: HashMap<String, PendingRename>,
    /// Wall-clock ms of the last rename batch, used to rate-limit renames.
    pub last_rename_ms: u64,
}
