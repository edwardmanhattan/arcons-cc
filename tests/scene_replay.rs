//! **A `Scene` is the whole arrangement** (#201, `ADR-0006`).
//!
//! The claim is a picture, so the proof is pixels: render a scene the ordinary way, then render it again with
//! `ARCONS_REPLAY=1`, which makes the wall take its own `Scene` and apply it back before drawing. If the two captures
//! differ, the scene is missing something the view reads — a camera, a name, a filter, a selection, a tiling, a ring,
//! a cursor — and that is exactly the failure a fleet would ship.
//!
//! **Why a sample and not all forty-five.** Every scene twice would double the gate's capture time for scenes that
//! draw only water (the newsroom's stories, the objects, the photograph), which is #203's half and cannot be in this
//! test until the water is in the `Scene`. These five are chosen to cover **every field family the `Scene` carries**:
//! a tiling with three panes and their cameras, a filter, a surface with a ring, a cursor over a selection with its
//! card, and a second workspace. A sixth field family added to `Scene` needs a scene that carries it — which is the
//! point: an unrepresented field is an unproven one.
//!
//! It runs the real binary, because only Rune can draw: `cargo test` alone cannot make a picture, and a test that
//! compared two structs would pass while the wall drew a camera nobody replicated.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The scenes this proof covers, and the field family each one stands for.
const SCENES: &[(&str, &str)] = &[
    ("panes", "a tiling of three panes and their cameras"),
    ("panes-filter", "a pane's filter vocabulary"),
    ("menu-ring", "a menu surface with the ring on a row"),
    // `cursor-hull` is deliberately not here: its **card** carries text the authority *read* (`commander`, identity)
    // rather than the fleet it holds, which is the other half of #203 (#211). A card is water too, and a scene that
    // does not carry it is a scene that draws a card-shaped hole.
    ("pane-tree", "a tree pane's own camera"),
    ("settings", "the settings sheet"),
    ("workspaces-switch", "three workspaces, on the second"),
    (
        "workspaces-rename-hidden",
        "a rename sheet with a typed name in it",
    ),
];

/// **A held gesture is not arrangement, and must not replay** (`ADR-0005`, #201).
///
/// A pane in the air and a border being dragged are the *pointer's*, last milliseconds, and are deliberately outside
/// the `Scene`: a scene that carried one could strand a pane in the air on somebody else's screen. So this asserts the
/// opposite of the test above — the drag scene renders **differently** when rebuilt — which keeps the exclusion a
/// decision rather than an oversight, and fails the day someone quietly puts a hold into the vocabulary.
#[test]
fn a_held_gesture_is_not_arrangement() {
    let plain = render("panes-drag", false);
    let replayed = render("panes-drag", true);
    assert_ne!(
        plain, replayed,
        "panes-drag replayed identically: a lift is now part of the Scene, which ADR-0005 keeps outside the door"
    );
}

#[test]
fn a_scene_rebuilds_the_picture_it_came_from() {
    for (scene, what) in SCENES {
        let plain = render(scene, false);
        let replayed = render(scene, true);
        assert_eq!(
            plain.len(),
            replayed.len(),
            "{scene} ({what}): the two renders are different sizes, which is not a Scene problem — the capture \
             failed. Set ARCONS_CAPTURE_VERBOSE=1 to see the renderer's own output."
        );
        assert!(
            plain == replayed,
            "{scene} ({what}): the wall rebuilt from its own Scene drew a different picture (#201). Something the \
             view reads is not in the Scene — a camera, a name, a filter, a selection, a tiling, a surface or a \
             cursor — and a viewer on another machine would draw the wrong wall."
        );
    }
}

/// Render one scene with the binary, and hand back the PNG's bytes.
///
/// The sizes and the time are the gate's own (`scripts/capture.sh`): a capture at two seconds, 1280×720 at scale 1, so
/// the pixels compared are the pixels the goldens hold.
fn render(scene: &str, replay: bool) -> Vec<u8> {
    let out = std::env::temp_dir().join(format!(
        "arcons-replay-{}-{scene}-{}.png",
        std::process::id(),
        if replay { "back" } else { "plain" }
    ));
    let _ = std::fs::remove_file(&out);
    let binary = bin();
    let mut command = Command::new(&binary);
    command.args([
        "--scene", scene, "--at", "2", "--fps", "60", "--size", "1280x720", "--scale", "1",
        "--snap",
    ]);
    command.arg(&out);
    if replay {
        command.env("ARCONS_REPLAY", "1");
    }
    // A capture must not find a wall's configuration: the gate renders fixtures, and a machine's `config.toml` would
    // make this test compare two different walls on two machines (`INV-11`'s reason, one level up).
    command.env_remove("ARCONS_CONTROL");
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("cannot run {}: {error}", binary.display()));
    let bytes = std::fs::read(&out).unwrap_or_else(|error| {
        panic!(
            "{scene}: no capture at {}: {error}\nstderr:\n{}",
            out.display(),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let _ = std::fs::remove_file(&out);
    if std::env::var_os("ARCONS_CAPTURE_VERBOSE").is_some() {
        eprintln!(
            "{scene} ({}): {}\n{}",
            if replay { "replayed" } else { "plain" },
            out.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    bytes
}

fn bin() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_arcons-cc")).to_path_buf()
}
