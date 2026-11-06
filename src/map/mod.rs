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

/// The Vulkan device the map's context must use, from `ARCONS_MAP_GPU` (`vendor:device`, e.g.
/// `8086:` for this laptop's Intel iGPU), exported to Mesa's device-select layer.
///
/// **MapLibre picks the wrong GPU on a hybrid laptop by default.** Measured here: it sorts the
/// physical devices and chooses the **RTX 2050**, while the panel is wired to the Intel iGPU and the
/// compositor runs there. Rendering the map on the discrete GPU means waking it every 4 Hz and
/// copying each frame back across the bus, for a picture the iGPU could shade.
///
/// Called from `main` before any thread starts (Rune included), because it is process-wide and the
/// loader reads it once. Unset, nothing is forced: the map uses whatever the loader picks, and a
/// hybrid machine is expected to set it. ADR-0001 records the value for this one.
#[cfg(feature = "map")]
pub fn pin_gpu_from_env() {
    if std::env::var_os("MESA_VK_DEVICE_SELECT").is_some() {
        return;
    }
    let Some(device) = std::env::var_os("ARCONS_MAP_GPU") else {
        return;
    };
    let device = device.to_string_lossy().to_string();
    if device.trim().is_empty() {
        return;
    }
    // SAFETY: called from `main` before Rune starts any thread, so nothing else is reading the
    // environment concurrently.
    unsafe { std::env::set_var("MESA_VK_DEVICE_SELECT", device.trim()) };
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
