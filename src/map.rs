//! The map thread: MapLibre into a `Pixels` buffer Rune draws, from an offline tile seed.
//!
//! #5 owns this module, and it is the one with a hardware decision in it. This machine is a hybrid
//! laptop whose panel is wired to the Intel iGPU, so the map and Rune must land on **the
//! compositor's GPU**, not the discrete one: `RUNE_GPU_LOG=1` prints the adapter Rune picked, and
//! ADR-0001 records what it said. Choosing the wrong adapter here is a stutter on every pan, not a
//! startup error.
//!
//! Rules that outlive #5:
//!
//! - The map owns its thread and its camera. It never blocks the view loop; it publishes frames.
//! - Markers and tiles must agree on projection: both sides call the same `project_mercator` with
//!   the camera the map last rendered (`INV-04`), to within a pixel.
//! - The seed is on disk and complete: v0.1 must cold-start with **no network**.
//! - `maplibre_native` is an optional dependency until #5 enables the `map` feature; its build
//!   script fetches and compiles ~2 GB of C++, which the gate must not pay for. See
//!   `vendor/README.md`.

/// Where the offline tile seed lives, relative to the crate root.
///
/// The directory exists in the tree from the scaffold on, so a cold clone answers "is the seed
/// missing?" with an empty directory rather than a path that does not exist — an absent path and an
/// absent tile are different failures, and `--self-check` says which.
pub const SEED_DIR: &str = "assets/tiles";

/// The seed directory as an absolute path (`CARGO_MANIFEST_DIR` at build time).
pub fn seed_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SEED_DIR)
}

/// Whether the seed holds at least one tile. An empty directory is not a seed, and neither is one
/// holding only the placeholder that keeps the directory in git.
pub fn seed_present() -> bool {
    std::fs::read_dir(seed_dir()).is_ok_and(|mut entries| {
        entries.any(|entry| {
            entry.is_ok_and(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        })
    })
}
