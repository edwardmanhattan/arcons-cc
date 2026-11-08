//! The machine's side of the wall: the log, the signal, and the clock it started on.
//!
//! Nothing here knows about Minos, the map or the view. This is the layer that makes a process
//! survive a night in a cupboard: it writes a log an operator can read, it exits *cleanly* when
//! systemd asks it to stop, and it says out loud which GPU is drawing, because on a hybrid laptop
//! that is the first question when the wall is slow.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// When the process started. The first map frame reports against this.
static STARTED: OnceLock<Instant> = OnceLock::new();
/// Set by `SIGTERM` or `SIGINT`, read by the view loop.
static TERMINATE: OnceLock<Arc<AtomicBool>> = OnceLock::new();
/// Whether the first-frame line has been written, so it is written once.
static FIRST_FRAME: AtomicBool = AtomicBool::new(false);

/// The process start, for measuring startup against.
pub fn started() -> Instant {
    *STARTED.get_or_init(Instant::now)
}

/// Milliseconds since [`started`].
pub fn since_start() -> u128 {
    started().elapsed().as_millis()
}

/// Write the log to stderr at the level `RUST_LOG` names (default `info`), and to `log_file` too when
/// the wall has one — a machine in a cupboard has no terminal, and stderr goes to the journal.
///
/// Structured redaction is not this layer's job: everything that could hold a secret already prints
/// as `<redacted>` (`config::Secret`), so there is nothing to grep out here (`INV-08`).
pub fn init_logging(log_file: Option<&Path>) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let result = match log_file {
        None => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .try_init(),
        Some(path) => match open_log(path) {
            Ok(file) => {
                let file = std::sync::Arc::new(std::sync::Mutex::new(file));
                tracing_subscriber::fmt()
                    .with_env_filter(filter)
                    .with_target(false)
                    // The terminal keeps its lines and the file gets the same ones: an operator
                    // watching a session and a log read afterwards must not disagree.
                    .with_writer(move || Tee {
                        file: std::sync::Arc::clone(&file),
                        stderr: std::io::stderr(),
                    })
                    .try_init()
            }
            Err(error) => {
                eprintln!(
                    "arcons-cc: cannot write the log to {}: {error}",
                    path.display()
                );
                Err(Box::new(error) as Box<dyn std::error::Error + Send + Sync>)
            }
        },
    };
    if let Err(error) = result {
        // A log that is already installed is not a failure of the wall's; anything else is worth a
        // line, because the operator will otherwise look for a file that was never written.
        eprintln!("arcons-cc: the log could not be started: {error}");
    }
}

/// Open the log for appending, creating it with mode `0600`: a log is not a secret, but it is also
/// not something to publish, and the file may end up beside a credential the operator copied in.
fn open_log(path: &Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// One writer, two destinations.
struct Tee {
    file: std::sync::Arc<std::sync::Mutex<std::fs::File>>,
    stderr: std::io::Stderr,
}

impl std::io::Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // A log that cannot be written is not a crash: the wall keeps drawing, and stderr still has
        // the line. Failing the process over a full disk would be worse than the missing line.
        if let Ok(mut file) = self.file.lock() {
            let _ = file.write_all(buf);
        }
        self.stderr.write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
        self.stderr.flush()
    }
}

/// This process's resident memory in megabytes, or `None` where the platform does not say.
///
/// Read from `/proc/self/statm` on Linux: the second field is resident pages, and the page size is the
/// kernel's. `None` everywhere else — a wall that invents a number is worse than one that admits it does
/// not know, and the log line says which it is.
pub fn rss_mb() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: f64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        // 4 KiB pages: `sysconf(_SC_PAGESIZE)` is 4096 on every Linux this wall runs on, and reading it
        // would mean a libc call for a number that has not changed since the 1990s.
        Some(pages * 4096.0 / (1024.0 * 1024.0))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Watch for `SIGTERM` and `SIGINT` and set a flag the view loop reads.
///
/// A wall is stopped by systemd (`SIGTERM`) or by someone pressing Ctrl-C in a terminal (`SIGINT`).
/// Neither should tear the process apart mid-frame: the map thread owns a GL context, and a hard exit
/// leaves the driver complaining. The loop quits through Rune's own quit path instead, which drops the
/// context in order.
pub fn watch_for_termination() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let installed = TERMINATE.set(Arc::clone(&flag)).is_ok();
    if installed {
        #[cfg(unix)]
        {
            // `signal` is async-signal-safe and this handler does one relaxed store: no allocation,
            // no lock, nothing to deadlock in a signal context.
            unsafe {
                libc::signal(
                    libc::SIGTERM,
                    handle_terminate as *const () as libc::sighandler_t,
                );
                libc::signal(
                    libc::SIGINT,
                    handle_terminate as *const () as libc::sighandler_t,
                );
            }
        }
    }
    flag
}

/// Whether a terminate signal has arrived.
pub fn terminated() -> bool {
    TERMINATE
        .get()
        .is_some_and(|flag| flag.load(Ordering::Relaxed))
}

#[cfg(unix)]
extern "C" fn handle_terminate(_signal: libc::c_int) {
    if let Some(flag) = TERMINATE.get() {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Say, once, how long the first frame took. The ticket's target is under 3 s, and a number in a log
/// is what makes that a fact rather than a hope.
pub fn first_frame(what: &str) {
    if !FIRST_FRAME.swap(true, Ordering::Relaxed) {
        tracing::info!("{what} in {} ms", since_start());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn the_clock_and_the_flag_are_usable_without_a_window() {
        // Both are read from the view loop, which in a test never starts.
        let _ = started();
        let _ = since_start();
        assert!(!terminated(), "nothing has signalled this process");
        first_frame("test frame");
        first_frame("test frame again");
    }

    #[test]
    fn a_log_file_is_created_private_and_appended_to() {
        let dir = std::env::temp_dir().join(format!("arcons-cc-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("nested").join("wall.log");
        let mut handle = open_log(&file).expect("create the log");
        handle.write_all(b"first\n").unwrap();
        drop(handle);
        let again = open_log(&file).expect("append to the log");
        drop(again);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "first\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "the log is not group or world readable");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
