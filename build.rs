//! The build refuses to make a wall binary that segfaults (#26, #93).
//!
//! `scripts/build-wall.sh` builds release with `MLN_PRECOMPILE=1`, which takes MapLibre Native's C++ from
//! **upstream's prebuilt amalgam**. Without it the vendored crate compiles MapLibre here — from a local
//! checkout if `MLN_LOCAL_REPOSITORY` is set, which `~/.cargo/config.toml` sets for development — and C++
//! compiled by this machine's gcc at `-O2` and above passes a null `SpvBuildLogger` into
//! `glslang::GlslangToSpv`, so the first map frame dies with `SIGSEGV` in the line layer's shader.
//!
//! `MLN_PRECOMPILE` defaults to `0`, so **nothing** used to stop `cargo build --release --features map` from
//! producing that binary — it has happened twice now, the second time while taking v0.1's definition of done
//! (#93). A warning would be a note in a log nobody reads; this is a hard failure with the command that
//! works, because a wall binary is not something to discover at 2 a.m.
//!
//! Debug builds are unaffected and stay allowed: the local C++ route at `-O0` renders, and the gate, the
//! captures and the engine's own examples all run there.

use std::process::exit;

fn main() {
    // A build script is re-run when these change, so the answer cannot be cached from a different shell.
    println!("cargo:rerun-if-env-changed=MLN_PRECOMPILE");
    println!("cargo:rerun-if-env-changed=MLN_LOCAL_REPOSITORY");

    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let wants_map = std::env::var_os("CARGO_FEATURE_MAP").is_some();
    if !(release && wants_map) {
        return;
    }

    let precompile = std::env::var("MLN_PRECOMPILE").unwrap_or_else(|_| "0".to_string());
    if matches!(precompile.trim(), "1" | "true") {
        return;
    }

    let local = std::env::var("MLN_LOCAL_REPOSITORY").unwrap_or_default();
    let route = if local.is_empty() {
        "this machine's compiler".to_string()
    } else {
        format!("the local checkout at {local}")
    };
    eprintln!(
        "\n\
         arcons-cc: refusing to build a release binary with the map engine compiled by {route}.\n\
         \n\
         That binary segfaults on its first map frame inside glslang (#26): MapLibre's C++ built at -O2 and\n\
         above passes a null argument into the shader translator. Upstream's prebuilt amalgam renders.\n\
         \n\
         Build the wall with the script that does it correctly:\n\
         \n    scripts/build-wall.sh\n\n\
         It sets MLN_PRECOMPILE=1 and clears MLN_LOCAL_REPOSITORY, which is what makes the difference. A\n\
         debug build (`cargo build --features map`) is unaffected and is what the gate uses.\n"
    );
    exit(1);
}
