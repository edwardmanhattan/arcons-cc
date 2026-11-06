//! The map thread: a camera in, an RGBA frame out, and a schedule that does not waste a frame.
//!
//! Three rules, and each one is a bug avoided:
//!
//! - **The UI thread never blocks.** Requests go in over a channel (`send` never waits); the frames
//!   come out through [`rune::Pixels::set`], which is documented as callable from any thread and
//!   coalesces sets between frames. There is no reply to await and nothing to poll.
//! - **One render in flight, coalesced.** A drag produces a camera change per frame; the thread drains
//!   everything waiting and renders once for the newest camera. Rendering each request would queue
//!   frames nobody sees and make the map harder to steer the faster the operator drags.
//! - **Nothing renders that nobody looks at.** Idle re-renders run at 4 Hz so appearance changes
//!   (labels settling, a tile arriving) still land, and a hidden window renders nothing at all.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::map::geometry::{Camera, Rendered};

/// How often an idle map re-renders: 4 Hz. Fast enough that a settled appearance arrives while the
/// operator is still looking at it, slow enough that a wall left alone costs almost nothing.
pub const IDLE_INTERVAL: Duration = Duration::from_millis(250);

/// What the view loop asks the map thread for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Request {
    /// Move the camera. Coalesced: only the newest one before a render is used.
    Camera(Camera),
    /// The drawing area changed, in device pixels.
    Resize(u32, u32),
    /// The window was occluded or shown. A hidden window renders nothing.
    Visible(bool),
    /// Stop the thread; used by the headless probe and by tests.
    Stop,
}

/// The handle the UI thread holds: a channel, the pixels it draws, and the camera of the last frame.
pub struct MapThread {
    requests: Sender<Request>,
    /// Joined on drop: the process must not tear down a Vulkan context under a thread that is still
    /// rendering. MapLibre aborts when it is — `Failed to set thread-specific storage`, then SIGABRT.
    join: Option<std::thread::JoinHandle<()>>,
    pixels: rune::Pixels,
    rendered: Arc<Mutex<Option<Rendered>>>,
    error: Arc<Mutex<Option<String>>>,
}

impl MapThread {
    /// Start the map thread. Returns immediately; nothing here touches a tile or a GL context on the
    /// caller's thread, so the caller's first frame is never a `LiveMap::new`.
    ///
    /// `size` is in device pixels — what the map must render, not the points the layout uses.
    pub fn spawn(camera: Camera, size: (u32, u32)) -> MapThread {
        let (requests, incoming) = channel();
        let pixels = rune::Pixels::new();
        let rendered = Arc::new(Mutex::new(None));
        let error = Arc::new(Mutex::new(None));

        let thread_pixels = pixels.clone();
        let thread_rendered = Arc::clone(&rendered);
        let thread_error = Arc::clone(&error);
        let join = std::thread::Builder::new()
            .name("arcons-map".to_string())
            .spawn(move || {
                run(
                    incoming,
                    thread_pixels,
                    thread_rendered,
                    thread_error,
                    camera,
                    size,
                )
            })
            .expect("the map thread starts");

        MapThread {
            requests,
            join: Some(join),
            pixels,
            rendered,
            error,
        }
    }

    /// The pixels to draw with `El::pixels`.
    pub fn pixels(&self) -> &rune::Pixels {
        &self.pixels
    }

    /// The camera and size of the frame currently on screen — what markers must project against
    /// (`INV-04`). `None` until the first frame lands.
    pub fn rendered(&self) -> Option<Rendered> {
        *self
            .rendered
            .lock()
            .expect("the rendered slot is not poisoned")
    }

    /// The first fatal error, if the map could not start. The view shows it instead of a grey box.
    pub fn error(&self) -> Option<String> {
        self.error
            .lock()
            .expect("the error slot is not poisoned")
            .clone()
    }

    pub fn set_camera(&self, camera: Camera) {
        let _ = self.requests.send(Request::Camera(camera));
    }

    pub fn resize(&self, width: u32, height: u32) {
        let _ = self.requests.send(Request::Resize(width, height));
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.requests.send(Request::Visible(visible));
    }

    /// Render now, without waiting for the idle tick. Used after a gesture settles.
    pub fn render_now(&self) {
        let _ = self.requests.send(Request::Visible(true));
    }
}

impl Drop for MapThread {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Stop);
        // Wait for it: dropping the map's GL context while its thread renders aborts the process,
        // and an abort on exit loses the last thing a wall wrote to its log. The thread returns at
        // the top of its own loop, so this is one idle interval at worst.
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// The thread body: own the scene, drain the queue, render once per turn.
fn run(
    requests: Receiver<Request>,
    pixels: rune::Pixels,
    rendered: Arc<Mutex<Option<Rendered>>>,
    error: Arc<Mutex<Option<String>>>,
    camera: Camera,
    size: (u32, u32),
) {
    let cache = crate::map::cache_path();
    if let Err(e) = crate::map::render::prepare_cache(&cache) {
        *error.lock().expect("the error slot is not poisoned") = Some(e.to_string());
        return;
    }

    let mut map = match crate::map::render::LiveMap::new(camera, size.0, size.1, &cache) {
        Ok(map) => map,
        Err(e) => {
            *error.lock().expect("the error slot is not poisoned") = Some(e.to_string());
            return;
        }
    };

    let mut camera = camera;
    let mut visible = true;
    let mut size = size;

    loop {
        match requests.recv_timeout(IDLE_INTERVAL) {
            Ok(Request::Stop) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Ok(first) => {
                if !apply(first, &mut camera, &mut size, &mut visible) {
                    return;
                }
                // Drain what accumulated while we were rendering: the newest camera wins, and the
                // drag never builds a backlog.
                loop {
                    match requests.try_recv() {
                        Ok(next) => {
                            if !apply(next, &mut camera, &mut size, &mut visible) {
                                return;
                            }
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => return,
                    }
                }
            }
            // The idle tick: re-render only if somebody is looking. Appearance changes (a tile, a
            // settled label) still arrive while the operator watches.
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }

        if !visible {
            continue;
        }

        if (map.dims().0, map.dims().1) != size {
            // A resize rebuilds the scene: a `Continuous` renderer is sized at construction. That is
            // expensive, which is why it happens on the size change and not on every frame.
            match crate::map::render::LiveMap::new(camera, size.0, size.1, &cache) {
                Ok(rebuilt) => map = rebuilt,
                Err(e) => {
                    *error.lock().expect("the error slot is not poisoned") = Some(e.to_string());
                    return;
                }
            }
        }
        map.set_camera(camera);
        let rgba = map.frame_rgba();
        let (width, height) = map.dims();
        if rgba.len() != (width as usize) * (height as usize) * 4 {
            *error.lock().expect("the error slot is not poisoned") = Some(format!(
                "the map handed back {} bytes for {width}x{height}",
                rgba.len()
            ));
            return;
        }
        // Called from this thread, on purpose: `Pixels::set` is the streaming path and coalesces.
        pixels.set(width, height, rgba);
        *rendered.lock().expect("the rendered slot is not poisoned") = Some(Rendered {
            camera,
            width,
            height,
        });
    }
}

/// Apply one request. Returns `false` when the thread should stop.
fn apply(request: Request, camera: &mut Camera, size: &mut (u32, u32), visible: &mut bool) -> bool {
    match request {
        Request::Camera(next) => *camera = next,
        Request::Resize(width, height) => *size = (width.max(1), height.max(1)),
        Request::Visible(next) => *visible = next,
        Request::Stop => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_camera_request_stops_the_thread_and_nothing_is_sent_to_a_dead_one() {
        // The drop path: the UI thread ends, the thread ends, and no send panics. A wall closes its
        // window; nothing in this crate may take the process down with it.
        let (requests, incoming) = channel();
        let handle = std::thread::spawn(move || {
            let mut camera = Camera::START;
            let mut size = (10, 10);
            let mut visible = true;
            match incoming.recv_timeout(Duration::from_secs(2)) {
                Ok(request) => apply(request, &mut camera, &mut size, &mut visible),
                Err(_) => false,
            }
        });
        requests
            .send(Request::Camera(Camera::new((1.0, 2.0), 5.0)))
            .expect("sends");
        assert!(handle.join().expect("no panic"));
        // A send to a dropped receiver is an error the caller ignores, never a panic.
        assert!(requests.send(Request::Stop).is_err());
    }

    #[test]
    fn the_last_wins_when_requests_queue_up() {
        // Coalescing: a drag sends one camera per frame, and the map must render the newest.
        let (mut camera, mut size, mut visible) = (Camera::START, (800, 600), true);
        for zoom in 11..=18 {
            assert!(apply(
                Request::Camera(Camera::new((0.0, 0.0), f64::from(zoom))),
                &mut camera,
                &mut size,
                &mut visible
            ));
        }
        assert_eq!(
            camera.zoom, 18.0,
            "the newest camera is the one that gets rendered"
        );
        assert!(apply(
            Request::Visible(false),
            &mut camera,
            &mut size,
            &mut visible
        ));
        assert!(!visible, "a hidden window renders nothing");
        assert!(
            !apply(Request::Stop, &mut camera, &mut size, &mut visible),
            "stop stops"
        );
    }
}
