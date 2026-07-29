/// Pick which pane, of the candidates on one tab, should supply the tab title.
///
/// Each candidate is `(pane_id, title, last_event_ts)`, where `last_event_ts` is
/// 0 when no Claude Code hook activity is known for that pane. Returns `None`
/// when no candidate has a usable title.
///
/// The rule that matters: **a candidate with a title is eligible even when its
/// activity timestamp is 0.** `self.sessions` is populated only by hook events,
/// so it is empty until a hook fires — the normal state right after a zellij
/// server restart resurrects panes from the serialized layout. An earlier
/// version keyed title selection off the session map and so renamed nothing at
/// all in that state, leaving every tab at `Tab #N` while the titles sat unused
/// in the pane manifest.
///
/// Ties broken by lowest pane id so the choice is stable across updates rather
/// than flapping with map iteration order.
pub fn pick_title_pane(candidates: &[(u32, String, u64)]) -> Option<u32> {
    let mut best: Option<(u32, u64)> = None;
    for (pane_id, title, activity) in candidates {
        if title.trim().is_empty() {
            continue;
        }
        let better = match best {
            None => true,
            Some((best_id, best_activity)) => {
                *activity > best_activity || (*activity == best_activity && *pane_id < best_id)
            }
        };
        if better {
            best = Some((*pane_id, *activity));
        }
    }
    best.map(|(id, _)| id)
}

/// Whether a tab's reported position can be trusted enough to address a
/// `rename_tab` call at it.
///
/// `TabInfo::position` arrives in a snapshot that may predate a tab close, and
/// upstream `zellij-org/zellij#3535` lets the server's positions drift from the
/// plugin's. When `position` disagrees with the tab's actual index in the list
/// we just received, positions are mid-drift and a rename could land on the
/// wrong tab. Extracted as a pure function so the rule is unit-testable
/// without a live zellij.
pub fn position_is_addressable(reported_position: usize, index_in_list: usize) -> bool {
    reported_position == index_in_list
}

/// Whether a desired rename has exhausted its retry budget.
///
/// Returns true when the same desired name has already been issued
/// `max_attempts` times without landing, meaning it should be abandoned rather
/// than re-issued forever (which is what turns `#3535` into a server-pegging
/// feedback loop).
pub fn rename_budget_exhausted(
    prior_desired: Option<&str>,
    desired: &str,
    attempts: u8,
    max_attempts: u8,
) -> bool {
    match prior_desired {
        // A different target resets the budget.
        Some(prior) if prior != desired => false,
        Some(_) => attempts >= max_attempts,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_work_with_no_session_activity_at_all() {
        // The regression this fix exists for: after a server restart the
        // sessions map is empty (no hook has fired), so every activity stamp is
        // 0. Titles must still be applied.
        let candidates = vec![(7, "Assess OpenBao status".to_string(), 0)];
        assert_eq!(pick_title_pane(&candidates), Some(7));
    }

    #[test]
    fn panes_without_a_title_are_not_candidates() {
        let candidates = vec![(1, "".to_string(), 99), (2, "   ".to_string(), 50)];
        assert_eq!(pick_title_pane(&candidates), None);
    }

    #[test]
    fn most_recently_active_pane_wins_when_several_have_titles() {
        let candidates = vec![
            (1, "older task".to_string(), 100),
            (2, "newer task".to_string(), 200),
        ];
        assert_eq!(pick_title_pane(&candidates), Some(2));
    }

    #[test]
    fn ties_resolve_to_lowest_pane_id_for_stability() {
        // Equal activity (e.g. both 0) must not depend on iteration order.
        let candidates = vec![(9, "a".to_string(), 0), (4, "b".to_string(), 0)];
        assert_eq!(pick_title_pane(&candidates), Some(4));
    }

    #[test]
    fn an_untitled_pane_never_beats_a_titled_one_on_activity() {
        // A busy pane with no OSC title must not suppress a quiet titled pane.
        let candidates = vec![(1, "".to_string(), 999), (2, "real title".to_string(), 0)];
        assert_eq!(pick_title_pane(&candidates), Some(2));
    }

    #[test]
    fn position_trusted_when_snapshot_agrees_with_list_order() {
        assert!(position_is_addressable(0, 0));
        assert!(position_is_addressable(7, 7));
    }

    #[test]
    fn position_rejected_when_positions_have_drifted() {
        // The #3535 signature: a close happened, so the reported position no
        // longer matches where the tab actually sits in the fresh list.
        assert!(!position_is_addressable(5, 4));
        assert!(!position_is_addressable(0, 3));
    }

    #[test]
    fn first_attempt_at_a_name_is_always_allowed() {
        assert!(!rename_budget_exhausted(None, "build", 0, 3));
    }

    #[test]
    fn retries_allowed_up_to_the_budget_then_abandoned() {
        assert!(!rename_budget_exhausted(Some("build"), "build", 1, 3));
        assert!(!rename_budget_exhausted(Some("build"), "build", 2, 3));
        // Budget reached: stop re-issuing. This is the brake that prevents the
        // rename/TabUpdate feedback loop.
        assert!(rename_budget_exhausted(Some("build"), "build", 3, 3));
        assert!(rename_budget_exhausted(Some("build"), "build", 9, 3));
    }

    #[test]
    fn a_new_target_resets_the_budget() {
        // Claude changed its OSC title; that is a fresh intent, not a retry, so
        // an exhausted budget for the old name must not suppress it.
        assert!(!rename_budget_exhausted(Some("build"), "deploy", 9, 3));
    }
}
