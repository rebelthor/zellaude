use std::collections::HashMap;
use zellij_tile::prelude::*;

/// Build a mapping from terminal pane_id -> (tab_index, tab_name).
/// Uses PaneManifest (keyed by tab_index) cross-referenced with TabInfo list.
pub fn build_pane_to_tab_map(
    tabs: &[TabInfo],
    manifest: &PaneManifest,
) -> HashMap<u32, (usize, String)> {
    let tab_name_by_position: HashMap<usize, String> = tabs
        .iter()
        .map(|t| (t.position, t.name.clone()))
        .collect();

    let mut map = HashMap::new();
    for (&tab_index, panes) in &manifest.panes {
        let tab_name = tab_name_by_position
            .get(&tab_index)
            .cloned()
            .unwrap_or_default();
        for pane in panes {
            if !pane.is_plugin {
                map.insert(pane.id, (tab_index, tab_name.clone()));
            }
        }
    }
    map
}

/// Choose, per tab index, which terminal pane's title the tab should mirror.
///
/// Driven by the pane manifest so it works even when no Claude Code hook has
/// fired yet (`sessions` empty), which is the normal state right after a zellij
/// server restart resurrects panes from the serialized layout. Session activity
/// is only a tiebreak for tabs holding more than one candidate pane.
///
/// Selection per tab, in order:
/// 1. Panes with a non-empty title are the only candidates — a pane with no OSC
///    title has nothing to contribute.
/// 2. If several qualify, prefer the one with the most recent Claude session
///    activity, so a tab running several agents follows the active one.
/// 3. Otherwise take the lowest pane id, so the choice is stable across updates
///    rather than flapping with `HashMap` iteration order.
pub fn pick_title_panes(
    manifest: &PaneManifest,
    sessions: &std::collections::BTreeMap<u32, crate::state::SessionInfo>,
) -> HashMap<usize, u32> {
    let mut chosen = HashMap::new();
    for (&tab_index, panes) in &manifest.panes {
        let candidates: Vec<(u32, String, u64)> = panes
            .iter()
            .filter(|pane| !pane.is_plugin)
            .map(|pane| {
                let activity = sessions.get(&pane.id).map_or(0, |s| s.last_event_ts);
                (pane.id, pane.title.clone(), activity)
            })
            .collect();
        if let Some(pane_id) = crate::rename_guard::pick_title_pane(&candidates) {
            chosen.insert(tab_index, pane_id);
        }
    }
    chosen
}

/// Build a mapping from terminal pane_id -> the pane's current title (the OSC
/// title set by the running program, as it appears in the UI).
pub fn build_pane_titles(manifest: &PaneManifest) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for panes in manifest.panes.values() {
        for pane in panes {
            if !pane.is_plugin {
                map.insert(pane.id, pane.title.clone());
            }
        }
    }
    map
}
