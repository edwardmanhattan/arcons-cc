//! The MapLibre renderer: one live scene per map thread, and an offline seed that stays offline.
//!
//! **Built only with `--features map`.** The default gate has no MapLibre: no GL, no C++, no
//! network. `cargo test map::` covers this module's geometry and scheduling without it, and
//! `cargo test --features map map::` covers the engine.
//!
//! The discipline is copied from TFG on purpose (its ADR-0001 is the same argument): one persistent
//! `Continuous` renderer per thread, so the GL context, tile cache, glyph atlas and shaders stay hot.
//! A re-render is then a camera update plus a few pumped frames — milliseconds. The static path
//! rebuilds the pipeline and costs seconds, which is the difference between a map that tracks a drag
//! and one that jumps at the end of it.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use maplibre_native::{
    CameraUpdate, Continuous, ImageRenderer, ImageRendererBuilder, LatLng, ResourceOptions,
};

use crate::map::geometry::Camera;

/// The style the seed was captured for. It is a URL, and it is fetched **from the cache**: the seed
/// carries the style document as a `resources` row, which is why pinning its expiry is what makes a
/// network-free cold start possible at all.
const STYLE_URL: &str = "https://tiles.openfreemap.org/styles/liberty";

/// Pixel layout of [`LiveMap::frame_rgba`]. MapLibre hands back **premultiplied** bytes; Rune's
/// `Pixels` wants straight alpha. If the picture draws washed out, this is the switch to look at —
/// not a colour, and not a tint.
const PREMULTIPLIED: bool = true;

/// The expiry every seed row is pinned to: 2100-01-01.
///
/// The seed was captured with ordinary HTTP expiry timestamps, so on a fresh install MapLibre asks
/// the network to revalidate the style, and a machine with no network can never report
/// style-loaded. Pinning the rows before the cache is opened is the difference between an offline map
/// and a black rectangle.
const OFFLINE_SEED_EXPIRY: i64 = 4_102_444_800;

/// How long a first load may take before it is a failure rather than a slow disk.
const STYLE_LOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// What can go wrong building the scene.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MapError {
    #[error("the seed has no bytes at {0}")]
    SeedMissing(String),
    #[error("the map cache at {path} could not be prepared: {reason}")]
    Cache { path: String, reason: String },
    #[error("the map style did not load within {} s (offline seed not usable?)", STYLE_LOAD_TIMEOUT.as_secs())]
    StyleNotLoaded,
    #[error("the map engine reported: {0}")]
    Engine(String),
    #[error("the frame was not {0} x {1} pixels of RGBA")]
    BadFrame(u32, u32),
}

/// A persistent map scene: build once, re-render for the life of the thread.
pub struct LiveMap {
    renderer: ImageRenderer<Continuous>,
    w: u32,
    h: u32,
}

impl LiveMap {
    /// Build the scene from the seed and pump until the style is loaded.
    ///
    /// `cache` is a writable copy of the seed: MapLibre writes tiles into it, so the seed itself stays
    /// pristine in `assets/` and a corrupt cache is recoverable by deleting one file.
    pub fn new(camera: Camera, w: u32, h: u32, cache: &Path) -> Result<Self, MapError> {
        let (w, h) = (w.max(1), h.max(1));
        let mut renderer = ImageRendererBuilder::new()
            .with_size(nonzero(w), nonzero(h))
            .with_resource_options(
                ResourceOptions::default()
                    .with_cache_path(cache.to_path_buf())
                    .with_maximum_cache_size(256 * 1024 * 1024),
            )
            .build_continuous_renderer();

        let loaded = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let observer = renderer.map_observer();
        observer.set_did_finish_loading_style_callback({
            let loaded = Arc::clone(&loaded);
            move || loaded.store(true, Ordering::SeqCst)
        });
        observer.set_did_fail_loading_map_callback({
            let failed = Arc::clone(&failed);
            move |_| failed.store(true, Ordering::SeqCst)
        });

        renderer.update_camera(&camera_update(camera));
        renderer.load_style_from_url(
            &STYLE_URL
                .parse()
                .map_err(|e| MapError::Engine(format!("style url: {e}")))?,
        );

        let deadline = Instant::now() + STYLE_LOAD_TIMEOUT;
        while !loaded.load(Ordering::SeqCst) {
            if failed.load(Ordering::SeqCst) {
                return Err(MapError::StyleNotLoaded);
            }
            if Instant::now() > deadline {
                return Err(MapError::StyleNotLoaded);
            }
            renderer.render_once();
            std::thread::sleep(Duration::from_millis(20));
        }

        let mut map = Self { renderer, w, h };
        // Let the first tiles arrive before anyone reads a frame: a first frame with no tiles is a
        // grey rectangle, and the wall's first impression is the one it keeps all night.
        // Eight renders is about a second of pumping: enough for the first tiles to decode and land,
        // which is the difference between opening on a map and opening on its background colour.
        map.pump(24);
        Ok(map)
    }

    /// Move the camera. The next [`Self::frame_rgba`] shows the new view.
    pub fn set_camera(&mut self, camera: Camera) {
        self.renderer.update_camera(&camera_update(camera));
    }

    /// Pump the runloop N frames so asynchronous work (tiles, glyphs) can land.
    pub fn pump(&mut self, frames: u32) {
        for _ in 0..frames {
            self.renderer.render_once();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Render one frame and hand back **straight-alpha sRGB RGBA**, rows top first.
    ///
    /// The unpremultiply happens here rather than at the consumer: `Pixels::set` is documented to take
    /// straight alpha, and a second consumer that forgot would draw the map washed out.
    pub fn frame_rgba(&mut self) -> Vec<u8> {
        self.renderer.render_once();
        let buffer = self.renderer.read_still_image().buffer().to_vec();
        if PREMULTIPLIED {
            unpremultiply(buffer)
        } else {
            buffer
        }
    }

    pub fn dims(&self) -> (u32, u32) {
        (self.w, self.h)
    }
}

fn nonzero(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap_or(NonZeroU32::new(1).expect("1 is not zero"))
}

fn camera_update(camera: Camera) -> CameraUpdate {
    CameraUpdate::new()
        .center(LatLng {
            lat: camera.center.0,
            lng: camera.center.1,
        })
        .zoom(camera.zoom)
}

/// Premultiplied RGBA → straight-alpha RGBA. Alpha 0 becomes transparent black, which is what
/// both layouts mean by it.
fn unpremultiply(mut rgba: Vec<u8>) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks_mut::<4>();
    for pixel in pixels {
        let alpha = u32::from(pixel[3]);
        if alpha == 0 {
            pixel[..3].fill(0);
            continue;
        }
        if alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
        }
    }
    rgba
}

/// The seed shipped with the binary, embedded: a wall installs from a package and a missing asset
/// beside the executable is a black map, not an error anybody reads.
pub const SEED: &[u8] = include_bytes!("../../assets/tiles-cache.seed.sqlite");

/// Prepare the writable cache from the embedded seed, then pin every row's expiry.
///
/// Called on every start, not only the first: a cache from an earlier launch holds the same rows with
/// the seed's original HTTP expiry, so pinning again is what keeps an old cache offline-usable.
pub fn prepare_cache(cache: &Path) -> Result<(), MapError> {
    let path = cache.display().to_string();
    if cache.is_file() && !cache_is_usable(cache) {
        // A half-written or foreign SQLite file: set it aside instead of guessing. The rename is
        // atomic, so an interrupted launch cannot leave a truncated cache in place.
        let aside = cache.with_extension(format!("sqlite.corrupt-{}", std::process::id()));
        std::fs::rename(cache, &aside).map_err(|e| MapError::Cache {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = cache.as_os_str().to_os_string();
            sidecar.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(sidecar));
        }
    }

    if cache.is_file() {
        return pin_expiries(cache);
    }

    let parent = cache.parent().ok_or_else(|| MapError::Cache {
        path: path.clone(),
        reason: "no parent directory".to_string(),
    })?;
    std::fs::create_dir_all(parent).map_err(|e| MapError::Cache {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    let temporary = cache.with_extension(format!("sqlite.tmp-{}", std::process::id()));
    std::fs::write(&temporary, SEED).map_err(|e| MapError::Cache {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    pin_expiries(&temporary)?;

    if let Err(e) = std::fs::rename(&temporary, cache) {
        // Another process won the first-launch race. Its complete seed is the result we wanted.
        let _ = std::fs::remove_file(&temporary);
        if cache.is_file() {
            return Ok(());
        }
        return Err(MapError::Cache {
            path,
            reason: e.to_string(),
        });
    }
    Ok(())
}

/// Pin `resources.expires` and `tiles.expires`. Both tables, always: a style row that expires is a
/// style that tries the network, and a tile row that expires is a tile that goes missing offline.
fn pin_expiries(cache: &Path) -> Result<(), MapError> {
    let path = cache.display().to_string();
    let connection = rusqlite::Connection::open(cache).map_err(|e| MapError::Cache {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    for table in ["resources", "tiles"] {
        connection
            .execute(
                &format!("UPDATE {table} SET expires = ?1"),
                rusqlite::params![OFFLINE_SEED_EXPIRY],
            )
            .map_err(|e| MapError::Cache {
                path: path.clone(),
                reason: format!("{table}: {e}"),
            })?;
    }
    Ok(())
}

/// Whether a file is a usable MapLibre cache: SQLite, intact, and holding rows in both tables.
pub fn cache_is_usable(cache: &Path) -> bool {
    let Ok(connection) =
        rusqlite::Connection::open_with_flags(cache, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    let check: Result<String, _> = connection.query_row("PRAGMA quick_check", [], |row| row.get(0));
    if !matches!(check.as_deref(), Ok("ok")) {
        return false;
    }
    ["tiles", "resources"].iter().all(|table| {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .is_ok_and(|count| count > 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("arcons-cc-map-test-{}-{name}", std::process::id()))
    }

    #[test]
    fn the_embedded_seed_is_a_cache_with_tiles_and_a_style() {
        // The seed is what makes a cold start draw with no network. If it is not a usable cache, the
        // wall opens on a black rectangle, so this is checked before anything else.
        let path = temp("seed.sqlite");
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, SEED).expect("the seed writes");
        assert!(
            cache_is_usable(&path),
            "the embedded seed is not a usable MapLibre cache"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn preparing_the_cache_pins_every_expiry_row() {
        let path = temp("pin.sqlite");
        let _ = std::fs::remove_file(&path);
        prepare_cache(&path).expect("the cache prepares");
        let connection = rusqlite::Connection::open(&path).expect("opens");
        for table in ["resources", "tiles"] {
            let stale: i64 = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE expires < ?1"),
                    rusqlite::params![OFFLINE_SEED_EXPIRY],
                    |row| row.get(0),
                )
                .expect("counts");
            assert_eq!(
                stale, 0,
                "{table} still has rows that will try to revalidate over the network"
            );
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_cache_is_set_aside_rather_than_written_over() {
        // A truncated file from a killed first launch must not become the wall's map for all time.
        let path = temp("corrupt.sqlite");
        std::fs::write(&path, b"this is not a database").expect("writes");
        prepare_cache(&path).expect("the cache recovers");
        assert!(cache_is_usable(&path), "the cache is usable again");
        let aside = path.with_extension(format!("sqlite.corrupt-{}", std::process::id()));
        assert!(
            aside.is_file(),
            "the corrupt file is kept for the person who has to explain it"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&aside);
    }

    #[test]
    fn a_premultiplied_frame_becomes_straight_alpha() {
        // PREMULTIPLIED is the switch the ticket names: getting it wrong draws a washed-out map, and
        // it is a one-line bug that looks like a colour.
        let premultiplied = vec![128, 64, 32, 128, 0, 0, 0, 0, 200, 100, 50, 255];
        let straight = unpremultiply(premultiplied);
        assert_eq!(
            &straight[0..4],
            &[255, 128, 64, 128],
            "half alpha doubles the channels back"
        );
        assert_eq!(
            &straight[4..8],
            &[0, 0, 0, 0],
            "transparent stays transparent black"
        );
        assert_eq!(
            &straight[8..12],
            &[200, 100, 50, 255],
            "opaque is untouched"
        );
    }
}
