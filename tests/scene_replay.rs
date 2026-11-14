//! **A `Scene` is the whole arrangement** (#201, `ADR-0006`).
//!
//! The claim is a picture, so the proof is pixels: render a scene the ordinary way, then render it again with
//! `ARCONS_REPLAY=1`, which makes the wall take its own `Scene` and apply it back before drawing. If the two captures
//! differ, the scene is missing something the view reads — a camera, a name, a filter, a selection, a tiling, a ring,
//! a cursor — and that is exactly the failure a fleet would ship.
//!
//! **Every scene, and that is the point** (#211). A sample proves the fields it happens to touch; the whole list is the
//! claim `ADR-0006` makes — *the scene is the whole picture* — and it can only be checked by rebuilding the picture
//! from it and finding nothing left over. A field family added to `Scene` therefore needs no careful choice of
//! representative scene: the new family is *already* drawn somewhere, by whichever scene draws it, and that scene is
//! in this list.
//!
//! The **gestures** stay out, because a hold is the pointer's and not the arrangement (`ADR-0005`), and the test below
//! asserts they replay *differently*.
//!
//! It runs the real binary, because only Rune can draw: `cargo test` alone cannot make a picture, and a test that
//! compared two structs would pass while the wall drew a camera nobody replicated.

use std::path::{Path, PathBuf};
use std::process::Command;

/// **The scene that is a hand, not a wall** (`ADR-0005`): a pane still in the air with the drop it would make.
///
/// Everything else replays identically, including the two that look like gestures and are not: `panes-pan` *ends* in a
/// camera, and `panes-gap-resize` is a gap **setting** — the scene says so in a comment, because a border may only be
/// moved by a pointer and there is no pointer in a capture.
/// They are outside the `Scene` on purpose, so they are the ones this proof expects to replay *differently*.
const HELD: &[&str] = &["panes-drag"];

/// **A held gesture is not arrangement, and must not replay** (`ADR-0005`, #201).
///
/// A pane in the air and a border being dragged are the *pointer's*, last milliseconds, and are deliberately outside
/// the `Scene`: a scene that carried one could strand a pane in the air on somebody else's screen. So this asserts the
/// opposite of the test above — the drag scene renders **differently** when rebuilt — which keeps the exclusion a
/// decision rather than an oversight, and fails the day someone quietly puts a hold into the vocabulary.
#[test]
fn a_held_gesture_is_not_arrangement() {
    for scene in HELD {
        let plain = render(scene, false);
        let replayed = render(scene, true);
        assert_ne!(
            plain, replayed,
            "{scene} replayed identically: a held gesture is now part of the Scene, which ADR-0005 keeps outside the \
             door"
        );
    }
}

#[test]
fn a_scene_rebuilds_the_picture_it_came_from() {
    let scenes: Vec<&str> = arcons_cc::scene::names()
        .into_iter()
        .filter(|scene| !HELD.contains(scene))
        .collect();
    assert!(
        scenes.len() >= 40,
        "only {} scenes were replayed: this proof is meant to cover the whole list (#211)",
        scenes.len()
    );
    // **Four at a time**, because two renders per scene over the whole list is the gate's longest test otherwise
    // (#211). A capture is a process and a GPU frame; nothing here shares state, and a scene's file names are its own.
    let workers = 4;
    let mut failures = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let scenes = &scenes;
                scope.spawn(move || {
                    let mut bad = Vec::new();
                    for scene in scenes.iter().skip(worker).step_by(workers) {
                        let plain = render(scene, false);
                        let replayed = render(scene, true);
                        if plain.len() != replayed.len() {
                            bad.push(format!(
                                "{scene}: the two renders are different sizes, which is not a Scene problem — the \
                                 capture failed. Set ARCONS_CAPTURE_VERBOSE=1 to see the renderer's own output."
                            ));
                        } else if plain != replayed {
                            bad.push(format!(
                                "{scene}: the wall rebuilt from its own Scene drew a different picture (#201, #211). \
                                 Something the view reads is not in the Scene — a camera, a name, a filter, a \
                                 selection, a tiling, a surface, a cursor, a hull's card, the newsroom's rows, the \
                                 facilities, the objects or a structure — and a viewer on another machine would draw \
                                 the wrong wall."
                            ));
                        }
                    }
                    bad
                })
            })
            .collect();
        for handle in handles {
            match handle.join() {
                Ok(bad) => failures.extend(bad),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    failures.sort();
    assert!(
        failures.is_empty(),
        "{} of {} scenes did not replay:\n{}",
        failures.len(),
        scenes.len(),
        failures.join("\n")
    );
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
