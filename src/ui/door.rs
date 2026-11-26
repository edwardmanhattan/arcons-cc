//! **The door.** Every input changes the wall here, or not at all.
//!
//! A pointer handler, a menu row, a socket job and a capture's script all arrive at [`State::apply`].
//! The body is `apply_by`: it settles the intent, and nothing below it is an input.

use super::pairing as pair;
use super::*;
use crate::control::{
    Applied, Changed, LAST_PANE, NOT_NEIGHBOURS, RATIO_WORDS, Refused, SELF_DROP, wire,
};
use crate::verb::{Intent, Origin, RenameTarget};

impl State {
    /// **The door.** Apply one intent, or say why not. A handler ignores the answer: rune re-runs the
    /// view after it, and a change from `poll` sets `layout_dirty` through the same call.
    pub fn apply(&mut self, intent: Intent) -> Result<Applied, Refused> {
        self.apply_as(intent, None)
    }

    /// **The door, and whose hand knocked.** A carrier's hand wins over an origin an intent carries.
    pub fn apply_as(&mut self, intent: Intent, hand: Option<Origin>) -> Result<Applied, Refused> {
        if asked_the_authority(self, &intent) {
            return Ok(Applied::Changed(Changed(false)));
        }
        let applied = self.settle(intent, hand)?;
        if applied.changed() {
            self.layout_dirty = true;
        }
        Ok(applied)
    }

    /// **The door's body, without the viewer check or the redraw word.** The menu's leaf and a cursor's
    /// derived changes come back through here, so a derived change is not a fresh input.
    pub(super) fn settle(
        &mut self,
        intent: Intent,
        hand: Option<Origin>,
    ) -> Result<Applied, Refused> {
        settle_by(self, intent, hand)
    }
}

#[cfg(unix)]
impl State {
    /// **One job, settled on the thread that owns the state** (#222).
    ///
    /// A carrier's request is answered here, and a carrier that hung up takes its origin with it here. `poll`
    /// calls this between frames and a test calls it directly, so "what a job means" is one function rather
    /// than a habit repeated wherever a queue is drained. `true` when a pixel moved.
    pub fn answer(&mut self, job: crate::control::Job) -> bool {
        match &job {
            crate::control::Job::Request { request, hand, .. } => {
                let (reply, changed) = self.answer_request(request, hand.clone());
                job.answer(reply);
                changed
            }
            crate::control::Job::Departed { origin } => self.let_go(origin),
        }
    }

    /// **One request, answered against the wall** (#202): the authority's whole vocabulary, in one function.
    ///
    /// A wall's loop and a viewer's session both come here, so there is one place that decides what a request
    /// means and one place that could grow a second opinion (`INV-07`).
    pub fn answer_request(
        &mut self,
        request: &wire::Request,
        hand: Option<Origin>,
    ) -> (wire::Answer, bool) {
        match request {
            wire::Request::Intent(intent) => match self.apply_as(intent.clone(), hand) {
                Ok(Applied::Changed(changed)) => {
                    let moved = changed.0;
                    (wire::Answer::Changed(Ok(changed)), moved)
                }
                Ok(Applied::Paired(paired)) => (wire::Answer::Paired(paired), true),
                Err(why) => (wire::Answer::Changed(Err(why)), false),
            },
            wire::Request::Wall => (
                wire::Answer::Wall(super::projection::versioned(self)),
                false,
            ),
            wire::Request::Watch(watch) => (wire::Answer::Watching(*watch), false),
            wire::Request::Scene => (wire::Answer::Scene(Box::new(self.scene())), false),
        }
    }
}

/// Whether this process handed `intent` to its authority instead of applying it (#204).
///
/// A viewer has a session and no authority of its own; everything else — a wall, a capture, the authority process
/// itself — applies in place. Two whole blocks rather than a `cfg`-ed expression: the field exists on one platform
/// only, and a branch would leave the rest of this function type-checked against it.
fn asked_the_authority(state: &State, intent: &Intent) -> bool {
    #[cfg(unix)]
    {
        // **True whenever this is a viewer**, whether or not the ask got through: a viewer that applied locally because
        // the wire was busy would move a pane the authority has not agreed to, and then move it back when the scene
        // arrives. A request that could not be queued is a press that did not happen — which the next scene shows.
        let Some(session) = state.session.as_ref() else {
            return false;
        };
        let _ = session.ask(wire::Request::Intent(intent.clone()));
        true
    }
    #[cfg(not(unix))]
    {
        let _ = (state, intent);
        false
    }
}

/// **The door, and whose hand knocked** (#222).
///
/// A request that arrived on a carrier with a name is that client's, whatever the frame said: the relay knows a phone
/// by the device name in `devices.toml`, the socket knows its client as `ctl`, and the origin the carrier reports wins
/// over an `origin` an intent carries. That is the ticket's rule made mechanical — a cursor is named by the hand that
/// moved it, so one remote cannot move another's pointer by typing its name.
///
/// `None` is a caller with no carrier: the pointer's own handlers, a scene's script, a test. Those may say which
/// origin they mean, which is what #205's per-client cursors are.
fn settle_by(
    state: &mut State,
    intent: Intent,
    hand: Option<crate::verb::Origin>,
) -> Result<Applied, Refused> {
    let changed = match intent {
        Intent::FocusPane(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            state.focus_pane(pane)
        }
        Intent::ToggleFullSize(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            super::toggle_full_size(state, pane)
        }
        Intent::ClosePane(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            // **The same predicate the menu row is built from** (#141): the control and the action cannot
            // disagree, and a refusal is the rule's name rather than a no-op.
            if !state.can_close(pane) {
                return Err(Refused(LAST_PANE));
            }
            super::close_from_menu(state, pane)
        }
        Intent::BeginRename(target) => {
            if !target_exists(state, target) {
                return Err(Refused(no_such(target)));
            }
            super::begin_rename_target(state, target);
            true
        }
        Intent::Rename { target, name } => {
            if !target_exists(state, target) {
                return Err(Refused(no_such(target)));
            }
            super::rename_to(state, target, name.as_deref())
        }
        Intent::SplitPane { pane, content } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            super::open_pane_beside(state, pane, content).is_some()
        }
        Intent::SplitAt { pane, at } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            // No frame means no camera to unproject against, and a pane opened at a guessed point shows the
            // wrong water — the failure `INV-04` exists to prevent (#77).
            if state.pane(pane).and_then(|pane| pane.frame).is_none() {
                return Err(Refused("the pane has no frame yet"));
            }
            super::split_at(state, pane, at).is_some()
        }
        Intent::SplitToWorkspace(pane) => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            super::new_workspace_from_pane(state, pane).is_some()
        }
        Intent::MovePane { pane, workspace } => {
            let Some(source) = state.workspace_of(pane) else {
                return Err(Refused("no such pane"));
            };
            if workspace >= state.workspaces.len() {
                return Err(Refused("no such workspace"));
            }
            if workspace == source {
                return Err(Refused("the pane is already in that workspace"));
            }
            if !super::move_pane_to(state, pane, workspace) {
                return Err(Refused("the tiling refused the pane"));
            }
            true
        }
        Intent::CloseOtherPanes => {
            // **An empty workspace has no focused pane to keep** (#141's state): the intent says so by name rather
            // than closing nothing and calling it a success.
            let Some(closed) = state.close_panes_but_focused() else {
                return Err(Refused("no pane has the keys"));
            };
            closed > 0
        }
        Intent::CloseAllPanes => {
            // **The wall's last pane is the one thing no intent may take** (#141), and the refusal is the request's:
            // `State::close_all_panes` asks the rule before the first pane goes, so a `None` here is a settled no and
            // not a wall with two panes missing. The words are `LAST_PANE` — the wall's rule — because the rule is
            // about the wall, never about this workspace.
            let Some(closed) = state.close_all_panes() else {
                return Err(Refused(LAST_PANE));
            };
            // An empty workspace is already what the client asked for, so nothing closed is not a change.
            closed > 0
        }
        Intent::CloseOtherWorkspaces => state.close_workspaces_but_active() > 0,
        Intent::CloseWorkspace(workspace) => {
            if workspace >= state.workspaces.len() {
                return Err(Refused("no such workspace"));
            }
            // **The action and its row read one answer** (#242): the rule is in `State::close_workspace`, the same
            // predicate `State::offers_close_workspace` gives the menu, so a row that says it can and an action that
            // says it cannot is unrepresentable.
            match state.close_workspace(workspace) {
                Ok(changed) => changed,
                Err(why) => return Err(Refused(why)),
            }
        }
        Intent::Drop { pane, onto, edge } => {
            if state.pane(pane).is_none() || state.pane(onto).is_none() {
                return Err(Refused("no such pane"));
            }
            // **A drop on itself is a rule, not a tiling that failed** (#219): the pointer cannot make one —
            // `drop_for` answers `None` over the pane it is dragging — so a client that says it meant it gets the
            // rule's own name rather than `Layout::apply`'s `false`.
            if pane == onto {
                return Err(Refused(SELF_DROP));
            }
            // **The release's own call**: `super::drop_for` builds the `view::Drop` a pointer's release builds, and
            // `State::apply_drop` is what that release calls, so the preview a client drew and this landing are
            // one function. A second layout computed here is the bug this intent exists to prevent.
            if !state.apply_drop(super::drop_for(edge, pane, onto)) {
                // Both ids are on the wall and the tiling still said no: the two are in different workspaces, or
                // the workspace on screen holds one pane. A crossing is `pane.move` and then a drop (#219).
                return Err(Refused("the tiling refused the pane"));
            }
            true
        }
        Intent::Resize { pane, other, ratio } => {
            if state.pane(pane).is_none() || state.pane(other).is_none() {
                return Err(Refused("no such pane"));
            }
            // **The field's shape first** (#219): a share the tiling cannot hold is a call to fix, not a rule
            // about panes, and the refusal names the range so a client is not left guessing it.
            let (low, high) = crate::config::layout::RATIO_RANGE;
            if !ratio.is_finite() || !(low..=high).contains(&ratio) {
                return Err(Refused(RATIO_WORDS));
            }
            // **The pair names the border**, unordered: a border carries both of its sides because an id alone
            // cannot say which of a pane's edges is meant (`Layout::set_ratio`). No border, no ratio to set.
            let border = state.borders().into_iter().find(|border| {
                (border.a == pane && border.b == other) || (border.a == other && border.b == pane)
            });
            let Some(border) = border else {
                return Err(Refused(NOT_NEIGHBOURS));
            };
            // **The border drag's own call**, clamp and all: `State::set_ratio` is what a hand's release ends in,
            // so the share a client asked for is the share the gutter lands on.
            if !state.set_ratio(&border, ratio.clamp(low, high)) {
                return Err(Refused("the tiling refused the pane"));
            }
            true
        }
        Intent::OpenPane(content) => super::open_pane_focused(state, content).is_some(),
        Intent::Follow { pane, unit, zoom } => {
            let Some(held) = state.pane(pane) else {
                return Err(Refused("no such pane"));
            };
            if !held.content.is_map() {
                return Err(Refused("the pane draws no map"));
            }
            match unit {
                Some(unit) => {
                    if !state.follow(pane, unit, zoom) {
                        // **Absence is not zero** (`INV-01`): a hull with no fix cannot be followed, and the reason
                        // names the hull rather than leaving a camera where a guess would put it.
                        return Err(Refused("the hull has not reported"));
                    }
                }
                None => {
                    if let Some(held) = state.pane_mut(pane) {
                        held.follow = None;
                    }
                }
            }
            true
        }
        Intent::Select { pane, unit } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            match unit {
                Some(unit) => {
                    state.select_in(pane, unit);
                }
                None => state.clear_selection(pane),
            }
            true
        }
        Intent::FocusWorkspace(index) => {
            if index >= state.workspaces.len() {
                return Err(Refused("no such workspace"));
            }
            state.switch_to(index)
        }
        Intent::NewWorkspace => {
            state.add_workspace();
            true
        }
        Intent::Camera { pane, camera } => {
            if state.pane(pane).is_none() {
                return Err(Refused("no such pane"));
            }
            state.set_camera_of(pane, camera);
            true
        }
        Intent::MenuFocus { row } => {
            if state.menu().is_none() {
                return Err(Refused("no menu is open"));
            }
            super::menu_ring(state, row)
        }
        Intent::MenuSubmenu { open } => {
            let Some(menu) = state.menu() else {
                return Err(Refused("no menu is open"));
            };
            if open {
                // **A submenu opens from the ring** (#164): the keyboard's row, never the pointer's.
                let Some(ring) = menu.ring else {
                    return Err(Refused("no row has the ring"));
                };
                let Some(row) = super::menu_rows(state).get(ring).cloned() else {
                    return Err(Refused("the ring is on no row"));
                };
                if !row.opens() {
                    return Err(Refused("the row opens nothing"));
                }
                let MenuAction::Submenu(key) = row.action else {
                    return Err(Refused("the row opens nothing"));
                };
                super::open_submenu(state, key);
                // The first row of the surface that just appeared, as a native menu does.
                super::menu_ring(state, Some(0));
                true
            } else {
                if menu.open.is_none() {
                    return Err(Refused("no submenu is open"));
                }
                super::close_submenu(state);
                true
            }
        }
        Intent::MenuChoose => {
            if state.menu().is_none() {
                return Err(Refused("no menu is open"));
            }
            if state.menu().and_then(|menu| menu.ring).is_none() {
                return Err(Refused("no row has the ring"));
            }
            super::menu_choose(state)
        }
        Intent::CursorMove { at, origin } => {
            state.cursor_move(hand.or(origin).unwrap_or_default(), at);
            true
        }
        Intent::CursorPress { origin } => {
            let origin = hand.or(origin).unwrap_or_default();
            if !state.cursors.contains_key(&origin) {
                // Nothing to press with: a client must point before it can press, and guessing a point would be
                // the wall inventing input nobody gave it.
                return Err(Refused("the cursor has not been placed"));
            }
            state.cursor_press(&origin);
            true
        }
        Intent::CursorRelease { origin } => {
            let origin = hand.or(origin).unwrap_or_default();
            let Some(cursor) = state.cursors.get(&origin).copied() else {
                return Err(Refused("the cursor has not been placed"));
            };
            if !cursor.down {
                return Err(Refused("the cursor is not pressed"));
            }
            state.cursor_release(&origin);
            true
        }
        Intent::PairBegin => pair::begin(state)?.0,
        Intent::PairClaim { code, name } => {
            // **The one verb whose answer is more than a flag** (#194): the key goes back to the carrier, once.
            return Ok(Applied::Paired(pair::claim(state, &code, &name)?));
        }
        Intent::PairForget { name } => pair::forget(state, &name).0,
        Intent::OpenSettings => super::open_settings(state),
        Intent::CloseSurfaces => super::close_surfaces(state),
    };
    Ok(Applied::Changed(Changed(changed)))
}

/// Whether the thing a rename names is on the wall.
fn target_exists(state: &State, target: RenameTarget) -> bool {
    match target {
        RenameTarget::Pane(pane) => state.pane(pane).is_some(),
        RenameTarget::Workspace(index) => index < state.workspaces.len(),
    }
}

fn no_such(target: RenameTarget) -> &'static str {
    match target {
        RenameTarget::Pane(_) => "no such pane",
        RenameTarget::Workspace(_) => "no such workspace",
    }
}
