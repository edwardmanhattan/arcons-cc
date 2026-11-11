//! Where the wall opens (#108): **one read at startup, for the pane that existed when it was asked
//! for**.
//!
//! `GET /viewpoint` answers `data { latitude, longitude }` and is authentication-only, so it is the
//! one operational resource every account can read. The value comes from deployment configuration
//! today and from instance provisioning later; the wall does not care which.
//!
//! Three rules decide this file, and each one is a way the obvious implementation goes wrong:
//!
//! - **The read is off the UI thread.** It is a [`crate::data::Command`] on the reads thread, answered
//!   between frames like every other read (`INV-05`); nothing here waits.
//! - **The first frame is the seed camera.** A read that is late is not a wall that is blank, so the
//!   camera moves only when an answer arrives — never before it.
//! - **The answer belongs to one pane, captured when the read was asked for.** The operator may split
//!   the wall or focus another pane while the read is in flight; "the focused pane when the reply
//!   lands" would move a pane they had already begun panning.
//!
//! **The response names no zoom.** The wall's start zoom is kept and only the centre is taken: a zoom
//! invented here would be a scale nobody chose. An unset viewpoint (`data: {}`) is an **absent**
//! value, not `0,0` (`INV-01`): the seed camera stays and the log says the backend has not chosen a
//! view. A refused or failed read is the same picture with a different line, never fatal.

use crate::data::Error;
use crate::data::types::Viewpoint;
use crate::map::geometry::Camera;
use crate::ui::State;

/// Ask the deployment where the map opens, **once**, and remember which pane is waiting for it.
///
/// The id is captured here, when the request is made — not read from the focus when the reply lands.
pub(crate) fn ask(state: &mut State) {
    if state.viewpoint_pane.is_some() {
        return;
    }
    let pane = state.layout.focused();
    let Some(backend) = &state.backend else {
        return;
    };
    backend.request(crate::data::Command::Viewpoint);
    state.viewpoint_pane = Some(pane);
}

/// Fold the answer into the wall. Returns whether anything the view draws changed.
pub(crate) fn apply(state: &mut State, result: &Result<Viewpoint, Error>) -> bool {
    // The pane that was on the wall when the read was made. No waiting pane means no read to answer
    // — a second reply must not move a pane the operator has chosen since.
    let Some(id) = state.viewpoint_pane.take() else {
        return false;
    };
    match result {
        Ok(viewpoint) => {
            let Some((latitude, longitude)) = viewpoint.coordinates() else {
                // `data: {}` is a deployment that has not chosen a view. Absence is not `0,0`
                // (`INV-01`), so the seed camera stays and the line says why.
                tracing::info!(
                    pane = id,
                    "GET /viewpoint: the backend has not chosen a view; the wall keeps its start camera"
                );
                return false;
            };
            let Some(pane) = state.pane_mut(id) else {
                // The pane was closed while the read was in flight. Nothing to move, and nothing to
                // invent: the panes that remain keep the cameras the operator gave them.
                tracing::info!(
                    pane = id,
                    "GET /viewpoint: the pane it was asked for is gone"
                );
                return false;
            };
            // **The response names no zoom.** Take the centre and keep the wall's start zoom.
            let camera = Camera::new((latitude, longitude), pane.camera.zoom);
            pane.camera = camera;
            #[cfg(feature = "map")]
            if let Some(map) = &pane.map {
                map.set_camera(camera);
            }
            tracing::info!(
                pane = id,
                lat = camera.center.0,
                lon = camera.center.1,
                zoom = camera.zoom,
                "the wall opens where the backend says"
            );
            true
        }
        Err(error) => {
            // A refused or failed read is a log line and the seed camera, never fatal.
            tracing::warn!(
                pane = id,
                error = %error,
                "GET /viewpoint did not read; the wall keeps its start camera"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stand where `ask` stands: the pane on the wall now is the one the answer belongs to.
    fn waiting(state: &mut State) -> (crate::ui::ViewId, Camera) {
        let pane = state.layout.focused();
        state.viewpoint_pane = Some(pane);
        (pane, state.camera_of(pane))
    }

    /// #108's claim: **a reply moves the first pane's camera, and only that pane's.** The second pane
    /// is opened *after* the read was asked for, so it is the case "the focused pane when the reply
    /// lands" would get wrong — and the split focuses it, which is what makes that bug visible.
    #[test]
    fn a_reply_moves_the_pane_that_asked_and_no_pane_opened_after_it() {
        let mut state = State::default();
        let (first, seed) = waiting(&mut state);

        let second = state
            .split_pane(first, Camera::new((1.0, 2.0), 8.0))
            .expect("a second pane");
        assert_eq!(
            state.layout.focused(),
            second,
            "the split focuses the new pane, so a reply that read the focus would move it"
        );
        let second_camera = state.camera_of(second);

        let changed = apply(
            &mut state,
            &Ok(Viewpoint {
                latitude: Some(-6.2),
                longitude: Some(106.8),
            }),
        );

        assert!(changed, "a reply that moves a camera must redraw the wall");
        let moved = state.camera_of(first);
        assert_eq!(
            moved,
            Camera::new((-6.2, 106.8), seed.zoom),
            "the first pane opens where the backend says, at the wall's start zoom"
        );
        assert_eq!(
            moved.zoom, seed.zoom,
            "the response names no zoom: the wall's start zoom is kept"
        );
        assert_eq!(
            state.camera_of(second),
            second_camera,
            "a pane opened after the read must not be moved by its answer"
        );
        assert_eq!(state.viewpoint_pane, None, "the read is answered once");
    }

    /// The dev server's answer today: `data: {}`, both coordinates absent together. **Absence is not
    /// `0,0`** (`INV-01`): the seed camera stays rather than moving the wall to the Gulf of Guinea.
    #[test]
    fn an_unset_viewpoint_leaves_the_seed_camera() {
        let mut state = State::default();
        let (first, seed) = waiting(&mut state);

        let changed = apply(&mut state, &Ok(Viewpoint::default()));

        assert!(
            !changed,
            "an unset viewpoint changes nothing the wall draws"
        );
        assert_eq!(
            state.camera_of(first),
            seed,
            "INV-01: an absent viewpoint must leave the seed camera, not draw 0,0"
        );
        assert_eq!(state.viewpoint_pane, None, "the read is answered once");
    }

    /// A refused or failed read is a log line and the seed camera, never fatal.
    #[test]
    fn a_refused_read_leaves_the_seed_camera_and_is_not_fatal() {
        let mut state = State::default();
        let (first, seed) = waiting(&mut state);

        let changed = apply(&mut state, &Err(Error::RateLimited));

        assert!(!changed, "a refused read is not a camera move");
        assert_eq!(
            state.camera_of(first),
            seed,
            "the seed camera is the fallback, not 0,0"
        );
        assert_eq!(state.viewpoint_pane, None, "a refusal still ends the read");
    }
}
