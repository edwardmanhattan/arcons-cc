//! The map: an offline seeded basemap on its own thread, and the arithmetic that puts a fix on it.
//!
//! v0.1 draws a real map — tiles, coastlines, labels — from an **embedded offline seed**, so a wall
//! with no network still opens on the picture. MapLibre is a map engine and nothing else: units are
//! overlay elements drawn by Rune above it (#6), never style layers. A unit that lives in the style
//! cannot be selected, carded or moved without rebuilding the map, and that is the single decision
//! TFG's ADR-0001 is about.
//!
//! Layout of this module:
//!
//! - [`geometry`] — Web Mercator, the camera, ground resolution. **No MapLibre in it**, so the gate
//!   proves the projection with no GL context, no tile and no network. `INV-04` lives here.
//! - [`render`] and [`thread`] — the engine and the thread that owns it, behind `--features map`. Its
//!   default build compiles C++ and downloads an amalgam, which the gate must not pay for.
//!
//! The handoff is **raw RGBA**: the thread calls [`rune::Pixels::set`] with straight-alpha bytes and
//! the view draws them with `El::pixels`. No framework image type crosses the channel, no decode
//! happens on the UI thread, and a same-size frame reuses its atlas slot.
//!
//! This machine's panel is wired to the Intel iGPU, so both Rune and the map must land on the
//! compositor's GPU; ADR-0001 records what `RUNE_GPU_LOG=1` said. The map's adapter is the one the
//! GL context gets, and a discrete-GPU context here would copy every frame over the bus.

pub mod geometry;

#[cfg(feature = "map")]
pub mod render;
#[cfg(feature = "map")]
pub mod thread;

use std::path::PathBuf;

/// The offline tile seed, embedded by [`render::SEED`] and kept in the tree so `--self-check` can
/// answer "is the seed here?" without the feature.
pub const SEED_FILE: &str = "assets/tiles-cache.seed.sqlite";

/// The seed's path in the source tree.
pub fn seed_path() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(SEED_FILE)
}

/// Whether the seed is present and plausibly a cache. The size is checked because a zero-byte file
/// from an interrupted checkout is worse than a missing one: it looks installed.
pub fn seed_present() -> bool {
    std::fs::metadata(seed_path()).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 1024)
}

/// Which Vulkan driver the map renders through.
///
/// **MapLibre picks the wrong GPU on this hybrid laptop.** Measured: it enumerates the physical
/// devices, sorts them itself, and takes the **RTX 2050** — while the panel is wired to the Intel
/// iGPU and the compositor runs there. Every map frame would be shaded on a GPU nobody is looking
/// at, and read back across the bus at 4 Hz. Mesa's `MESA_VK_DEVICE_SELECT` does not move it.
/// Limiting the loader to one ICD does, because then there is only one device to pick: with
/// `VK_DRIVER_FILES=/usr/share/vulkan/icd.d/intel_icd.json` MapLibre logs
/// `Using "Intel(R) Graphics (RPL-P)"`, and a deliberately bogus path makes it fail outright, which
/// is how we know the loader honours it.
///
/// The **default is the integrated GPU** when the machine has both integrated and discrete Vulkan
/// drivers. That is not a guess about which is faster, it is the compositor: on a hybrid laptop the
/// panel hangs off the integrated one, and a wall display is not a game. It is also what Rune itself
/// does — `RUNE_GPU_LOG=1` reports "the compositor's GPU" — so both halves of the window end up on
/// one device. Overridable:
///
/// - `ARCONS_MAP_VK_DRIVER=<path>` — an ICD manifest, e.g. `…/nvidia_icd.json`;
/// - `ARCONS_MAP_VK_DRIVER=off` — leave the choice to the Vulkan loader.
///
/// A path rather than a vendor id on purpose: mapping "8086" to a file is a guess about driver
/// packaging, and a wall is not the place for that guess.
#[cfg(feature = "map")]
pub fn pin_gpu_from_env() {
    let icds = vulkan_icds();
    let explicit = std::env::var("ARCONS_MAP_VK_DRIVER").ok();
    match choose_driver(explicit.as_deref(), &icds) {
        Selection::None => {}
        Selection::Invalid(value) => eprintln!(
            "arcons-cc: ARCONS_MAP_VK_DRIVER={value} is neither `auto`, `off`, nor a readable ICD file; \
             the map will render on whatever the Vulkan loader picks"
        ),
        Selection::Driver(path) => {
            eprintln!(
                "arcons-cc: the map renders through {} — {}",
                path.display(),
                reason(&path)
            );
            // SAFETY: called from `main` before Rune starts any thread, so nothing else reads the
            // environment concurrently. `VK_DRIVER_FILES` is the current loader's name;
            // `VK_ICD_FILENAMES` is the one an older loader still reads.
            unsafe {
                std::env::set_var("VK_DRIVER_FILES", &path);
                std::env::set_var("VK_ICD_FILENAMES", &path);
            }
        }
    }
}

/// What [`choose_driver`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Leave it to the loader: `off`, or nothing suitable found.
    None,
    /// The value was not a directive or a file.
    Invalid(String),
    /// This ICD, and nothing else.
    Driver(std::path::PathBuf),
}

/// The decision, as a **pure function** so it can be tested without a GPU, a loader, or a display.
///
/// `icds` is what the Vulkan loader would have offered. An explicit value wins over `auto`,
/// including when it names the discrete card: the operator asked.
pub fn choose_driver(explicit: Option<&str>, icds: &[std::path::PathBuf]) -> Selection {
    match explicit.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("auto") => icds
            .iter()
            // Rank, do not merely filter: this machine offers `intel_hasvk_icd.json` (Haswell-era
            // Intel) beside `intel_icd.json`, and the legacy one sorts first. Choosing it finds no
            // device on a modern iGPU and loses the map entirely.
            .min_by_key(|path| rank(path))
            .cloned()
            .map_or(Selection::None, Selection::Driver),
        Some("off") => Selection::None,
        Some(value) => {
            let path = std::path::PathBuf::from(value);
            if path.is_file() {
                Selection::Driver(path)
            } else {
                Selection::Invalid(value.to_string())
            }
        }
    }
}

/// How good a candidate this ICD is, lowest first: a modern integrated GPU, then any other
/// integrated one, then anything else the loader offered.
fn rank(path: &std::path::Path) -> u8 {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let legacy = name.contains("hasvk");
    match (is_integrated(path), legacy) {
        (true, false) => 0,
        (true, true) => 1,
        (false, _) => 2,
    }
}

/// A Vulkan ICD manifest that is a GPU the *panel* is likely on.
///
/// Filenames, not vendor ids: the manifest's `library_path` names a driver, not a bus address, and
/// every packager spells the file after the vendor. `lvp`/`swrast` are lavapipe, which is a software
/// device and never what a wall should choose when real hardware is present.
fn is_integrated(path: &std::path::Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let integrated = ["intel", "radeon", "amdgpu", "panfrost", "freedreno", "v3d"]
        .iter()
        .any(|v| name.contains(v));
    let software = name.contains("lvp") || name.contains("swrast") || name.contains("software");
    integrated && !software
}

#[cfg(feature = "map")]
fn reason(path: &std::path::Path) -> &'static str {
    if is_integrated(path) {
        "the integrated GPU, where the panel is"
    } else {
        "chosen by ARCONS_MAP_VK_DRIVER"
    }
}

/// Every ICD manifest the Vulkan loader would search, in its own order.
#[cfg(feature = "map")]
fn vulkan_icds() -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        dirs.push(home.join(".local/share/vulkan/icd.d"));
        dirs.push(home.join(".config/vulkan/icd.d"));
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_DIRS") {
        for dir in std::env::split_paths(&xdg) {
            dirs.push(dir.join("vulkan/icd.d"));
        }
    }
    dirs.push(std::path::PathBuf::from("/usr/local/share/vulkan/icd.d"));
    dirs.push(std::path::PathBuf::from("/usr/share/vulkan/icd.d"));
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut files: Vec<std::path::PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        files.sort();
        found.extend(files);
    }
    found
}

/// Where the writable copy of the seed lives: MapLibre writes into its cache, so the shipped seed
/// stays pristine and a corrupt cache is recoverable by deleting one file.
pub fn cache_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from(".cache"));
    base.join("arcons-cc").join("map-cache.sqlite")
}

/// Render one frame to a PNG without opening a window, for CI.
///
/// This is the ticket's `--probe-map`: if it cannot produce a picture on a machine with no display,
/// then the map's whole verification story is a screenshot somebody took by hand.
#[cfg(feature = "map")]
pub fn probe(
    camera: geometry::Camera,
    size: (u32, u32),
    out: &std::path::Path,
) -> Result<(u32, u32), String> {
    let cache = cache_path();
    render::prepare_cache(&cache).map_err(|e| e.to_string())?;
    let mut map =
        render::LiveMap::new(camera, size.0, size.1, &cache).map_err(|e| e.to_string())?;
    // Tiles arrive asynchronously even from the local cache: a single render can beat them and hand
    // back the style's background. Pump until the frame stops changing, bounded so a cold cache
    // cannot hang CI.
    map.pump(24);
    let rgba = map.frame_rgba();
    let (width, height) = map.dims();

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let file =
        std::fs::File::create(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("png header: {e}"))?;
    writer
        .write_image_data(&rgba)
        .map_err(|e| format!("png data: {e}"))?;
    writer.finish().map_err(|e| format!("png finish: {e}"))?;
    Ok((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icd(name: &str) -> PathBuf {
        PathBuf::from("/usr/share/vulkan/icd.d").join(name)
    }

    #[test]
    fn by_default_the_map_takes_the_integrated_gpu_the_panel_is_on() {
        // The bug this fixes: MapLibre sorted the devices and took the RTX 2050 while the compositor
        // and the panel were on the Intel iGPU. Integrated first is the compositor on a hybrid
        // laptop, and it is what Rune itself chooses.
        let icds = vec![icd("nvidia_icd.json"), icd("intel_icd.json")];
        assert_eq!(
            choose_driver(None, &icds),
            Selection::Driver(icd("intel_icd.json"))
        );
        assert_eq!(
            choose_driver(Some("auto"), &icds),
            Selection::Driver(icd("intel_icd.json"))
        );
    }

    #[test]
    fn lavapipe_is_not_a_gpu_the_wall_should_choose() {
        // A CI runner has only this. Choosing it as "integrated" would be right for CI and wrong for
        // a wall, so it falls back to whatever is first rather than pretending it is the panel's GPU.
        let icds = vec![icd("lvp_icd.x86_64.json"), icd("nvidia_icd.json")];
        assert_eq!(
            choose_driver(None, &icds),
            Selection::Driver(icd("lvp_icd.x86_64.json"))
        );
        assert!(!is_integrated(&icd("lvp_icd.x86_64.json")));
        assert!(is_integrated(&icd("intel_icd.json")));
        assert!(!is_integrated(&icd("nvidia_icd.json")));
    }

    #[test]
    fn an_explicit_driver_wins_off_wins_and_junk_is_refused() {
        let icds = vec![icd("nvidia_icd.json"), icd("intel_icd.json")];
        assert_eq!(
            choose_driver(Some("off"), &icds),
            Selection::None,
            "off means the loader decides"
        );
        // A path that is not a file is refused loudly rather than silently ignored: a wall that
        // renders on the wrong GPU because of a typo is this ticket all over again.
        assert_eq!(
            choose_driver(Some("/nope.json"), &icds),
            Selection::Invalid("/nope.json".to_string())
        );
        // A real file wins even when it is the discrete card: the operator asked for it.
        let this_file = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        assert_eq!(
            choose_driver(Some(&this_file.display().to_string()), &icds),
            Selection::Driver(this_file)
        );
    }

    #[test]
    fn the_legacy_intel_driver_loses_to_the_modern_one() {
        // `intel_hasvk_icd.json` sorts first on this machine and finds no device on a modern iGPU.
        // Ranking, not filtering, keeps one rule for "integrated" while still preferring the one
        // that can actually drive the panel.
        let icds = vec![
            icd("intel_hasvk_icd.json"),
            icd("nvidia_icd.json"),
            icd("intel_icd.json"),
        ];
        assert_eq!(
            choose_driver(None, &icds),
            Selection::Driver(icd("intel_icd.json"))
        );
        assert!(rank(&icd("intel_icd.json")) < rank(&icd("intel_hasvk_icd.json")));
    }

    #[test]
    fn a_machine_with_no_icd_leaves_the_loader_alone() {
        assert_eq!(choose_driver(None, &[]), Selection::None);
    }

    #[test]
    fn the_offline_seed_is_in_the_tree_and_not_empty() {
        // The seed is the reason a network-free cold start draws at all. Its absence is the one
        // failure that turns the map into a black rectangle with no error to read.
        assert!(
            seed_present(),
            "the offline seed at {} is missing or empty",
            seed_path().display()
        );
        let size = std::fs::metadata(seed_path())
            .expect("the seed exists")
            .len();
        assert!(
            size > 1_000_000,
            "the seed is {size} bytes, too small to hold a style and tiles"
        );
    }

    #[test]
    fn the_writable_cache_is_ours_and_absolute() {
        let cache = cache_path();
        assert!(
            cache.is_absolute(),
            "the cache must not resolve against the working directory: {}",
            cache.display()
        );
        assert_eq!(
            cache.file_name().and_then(|name| name.to_str()),
            Some("map-cache.sqlite")
        );
        assert_ne!(cache, seed_path(), "the shipped seed is never written to");
    }
}
