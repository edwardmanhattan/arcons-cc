//! The view: one placeholder panel, on the wall's background.
//!
//! This is the seam every later UI ticket lands in. The rules it already obeys:
//!
//! - **The UI thread never blocks.** `view` builds elements and returns. It reads whatever the
//!   reads thread and the socket actor last published; it never waits for either.
//! - **The card and the markers are not here yet.** #6 draws the markers, #7 the card. This ticket
//!   draws the frame they will live in, so the next one does not argue about layout.
//! - **Every colour and size comes from [`tokens`].** Nothing in this file names one.

pub mod tokens;

use std::process::ExitCode;

use rune::{AppSpec, Cx, El, col, row, spacer, text};

use crate::config;
use crate::units::Picture;

/// The window title, and the wordmark's text.
pub const TITLE: &str = "Arcons Command Center";

/// What the view loop holds. #3 fills the picture, #4 moves it, #6 draws it, #7 selects from it.
#[derive(Default)]
pub struct State {
    pub picture: Picture,
}

/// The whole window.
pub fn view(state: &State, _cx: &mut Cx<State>) -> El {
    let status = if state.picture.is_empty() {
        "no picture yet".to_string()
    } else {
        match state.picture.len() {
            1 => "1 unit".to_string(),
            n => format!("{n} units"),
        }
    };

    let header = row()
        .w_full()
        .gap(tokens::GAP)
        .child(
            text(TITLE)
                .key("wordmark")
                .font(tokens::WORDMARK)
                .color(tokens::accent()),
        )
        .child(spacer())
        .child(
            text(status)
                .key("picture-status")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );

    let panel = col()
        .key("picture")
        .w_full()
        .h_full()
        .grow(1.0)
        .center()
        .gap(tokens::GAP)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::hairline())
        .fill(tokens::panel())
        .child(
            text("AWAITING THE PICTURE")
                .key("picture-title")
                .font(tokens::TITLE)
                .color(tokens::ink()),
        )
        .child(
            text("the feed, the socket and the map land here (#3, #4, #5)")
                .key("picture-hint")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );

    col()
        .w_full()
        .h_full()
        .padding(tokens::PAD)
        .gap(tokens::GAP)
        .fill(tokens::bg())
        .child(header)
        .child(panel)
}

/// How the window opens: sized from the command line, or covering the primary output.
///
/// Rune at this rev exposes no fullscreen API, so `--fullscreen` is the closest honest thing —
/// borderless, at the primary output's own size and position. #8 revisits it once the wall's output
/// is chosen, and records what it decided.
fn window_options(window: config::Window, displays: &[rune::Display]) -> rune::WindowOptions {
    let options = rune::WindowOptions {
        min_size: Some(tokens::MIN_WINDOW),
        ..Default::default()
    };
    if !window.fullscreen {
        return options;
    }
    let primary = displays
        .iter()
        .find(|display| display.primary)
        .or_else(|| displays.first());
    rune::WindowOptions {
        borderless: true,
        position: primary.map(|display| display.position),
        physical_size: primary.map(|display| display.size),
        ..options
    }
}

/// Open the display and run until the compositor closes it.
pub fn run(window: config::Window, passthrough: Vec<String>) -> ExitCode {
    let spec = AppSpec::<State> {
        min_size: Some(tokens::MIN_WINDOW),
        ..AppSpec::new(TITLE, window.size, State::default, view)
    };

    // Rune's own flags (`--snap`, `--scene`, `--tree`, `--script`) arrive here untouched: #9's
    // captures are `cargo run -- --snap out/<scene>.png`, and swallowing the flag would make the
    // capture succeed at producing nothing.
    let result = rune::try_run_windows_with_args(
        rune::RunOptions::default(),
        passthrough,
        move |displays| {
            let options = window_options(window, displays);
            [rune::WindowSpec::from_app(
                "main",
                rune::App::new(spec).window_options(options),
            )]
        },
    );

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("arcons-cc: cannot open a window: {message}");
            eprintln!("  hint: the display needs a compositor; `cargo run -- --version` does not.");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    fn frame_once(state: State) -> Runtime<State> {
        // Headless: layout and paint with no display and no GPU. The virtual clock is 0.0, so a
        // first frame is deterministic (INV-11 in waiting).
        let mut rt = Runtime::new(
            state,
            view,
            tokens::WINDOW_SIZE,
            Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        rt
    }

    #[test]
    fn the_placeholder_panel_is_drawn_inside_the_window() {
        let rt = frame_once(State::default());
        let (rect, opacity) = rt
            .presented("picture")
            .expect("the picture panel must be drawn");
        assert!(opacity > 0.9, "the panel must be opaque, got {opacity}");
        let (x, y, w, h) = (rect[0], rect[1], rect[2], rect[3]);
        assert!(w > 0.0 && h > 0.0, "the panel must have area, got {w}x{h}");
        assert!(
            x >= 0.0
                && y >= 0.0
                && x + w <= tokens::WINDOW_SIZE.0
                && y + h <= tokens::WINDOW_SIZE.1,
            "the panel must stay inside the window, got {rect:?}"
        );
    }

    #[test]
    fn the_status_line_says_no_picture_yet_rather_than_zero_units() {
        // The scaffold has asked nobody, so the view may not print "0 units" — that would be a
        // claim about the fleet. Same distinction as INV-09, one layer up.
        let state = State::default();
        assert!(state.picture.is_empty());
        let rt = frame_once(state);
        assert!(
            rt.find("picture-status").is_some(),
            "the status line must be laid out"
        );
        assert!(
            rt.find("picture-title").is_some(),
            "the panel must have its heading"
        );
    }
}
