//! **The named states a capture renders** — the desk's side of Rune's `AppSpec::scenes`.
//!
//! A scene is `&'static [(&'static str, fn(&mut State))]`, exactly as on the wall, so
//! `--scene time-field --snap out/desk-time.png` works with nothing from us beyond the state it
//! builds. Rune starts a scene at logical time zero, and the desk reads no clock of its own, so
//! two runs are the same picture.
//!
//! **Two states of one screen**, because the calendar is the one thing that covers the panel it
//! belongs to: `time-field` is the panel at rest, with the regime line and the UTC readout visible, and
//! `time-calendar` is the same panel with the popover open. A single scene with the calendar open would
//! hide the banner the ticket exists to show, which is how the first capture of this screen was wrong.

use arcons_cc::verify::Bounds;

use crate::ui::State;

/// One scene: a name and the function that builds its state.
pub type Scene = (&'static str, fn(&mut State));

/// Every scene the binary can render, in the order `--scenes` prints them.
pub const SCENES: &[Scene] = &[("time-field", time_field), ("time-calendar", time_calendar)];

/// The sample ADR-0009's wireframe names: 20 November 2026, 14:03:22 WIB, the calendar closed, and the
/// server's clock three seconds behind the wall clock. Closed, so the regime line and the UTC readout
/// are in the picture: they are what this ticket's proof is about.
fn time_field(state: &mut State) {
    *state = State::sample();
    state.calendar_open = false;
}

/// The same panel with the calendar open, to witness the anchoring: it covers what is behind it and
/// moves nothing.
fn time_calendar(state: &mut State) {
    *state = State::sample();
}

/// The scene function for `name`, or `None`.
pub fn find(name: &str) -> Option<fn(&mut State)> {
    SCENES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, scene)| *scene)
}

/// Build a scene's state without a window, for a test that wants what a capture renders.
pub fn state_for(name: &str) -> State {
    let mut state = State::sample();
    match find(name) {
        Some(scene) => scene(&mut state),
        None => eprintln!("desk: no scene {name:?}; scenes: {}", names().join(", ")),
    }
    state
}

/// The scene names, one per line, as `--scenes` prints them.
pub fn names() -> Vec<&'static str> {
    SCENES.iter().map(|(name, _)| *name).collect()
}

/// The bounds a desk golden is compared within.
///
/// **Unmeasured, and honestly so.** `arcons_cc::verify::golden_bounds` is keyed by the wall's scenes
/// and does not name one of ours — the wall does not know what the desk draws (#276). Every desk scene
/// therefore starts from its `DEFAULT_BOUNDS`: two steps of channel tolerance, 200 pixels changed by
/// more than one step, and 600 pixels changed by exactly one — the numbers the wall's ordinary scenes
/// use, and the numbers `--compare` prints on every run.
///
/// **Where a measurement goes:** here, beside the scene list it describes. When CI's lavapipe has
/// rendered these scenes and the first real diff is recorded, this function returns that measured
/// `Bounds` for the scene that measured it, exactly as `golden_bounds` does for the wall's exceptions.
/// It does not go in the wall's table, and it is never a number picked to make a diff pass.
pub fn bounds(name: &str) -> Bounds {
    arcons_cc::verify::golden_bounds(name)
}
