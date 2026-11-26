//! **The relay** (#196): the wall's one outbound carrier, and the frames it speaks on it.
//!
//! `docs/mobile-remote.md` decision 2: the wall does not listen. It **dials out** to the deployment's relay over an
//! authenticated, reconnecting connection, and a phone talks to the relay. The relay may be Minos or a separate
//! service, so the wall's half of that contract is pinned here and in `docs/control.md` — and the transport is
//! [`super::carrier::Carrier`] (#195), so a line from a relay goes through **the same session loop** a line from the
//! unix socket does. There is one vocabulary (`wire::Request`, `wire::Answer`) and one place that could break
//! `INV-05`; a second encoder for "relay frames" is the bug this module exists to make unnecessary.
//!
//! Four rules, each of them a test:
//!
//! - **The wall says who it is first**, with the credential it already holds — its Minos credential and nothing
//!   invented (`INV-08`: it reaches the socket and never a log line).
//! - **"Connected" is the relay's answer, not the dial.** A socket that opened and then said nothing is a relay that
//!   is not answering; one that says `welcome: false` is a relay that refused the wall. The two are different names
//!   because they are different fixes: wait, or fix the credential.
//! - **Who is speaking is a device key**, hashed and looked for in `devices.toml` — the file keeps hashes, so a phone
//!   that presents the hash instead of the key is refused, and a revoked device stops at its next frame. A frame
//!   from a key no row holds is `that device is not paired`, answered to the relay, and the wall never sees it —
//!   **except `pair.claim`, and except nothing else** (#262): a phone that has never paired has no key to send, so the
//!   one verb that can precede a name is carried, and its answer goes back on the connection it arrived on **with no
//!   `to`**, the refusal's own convention extended to the one grant that can come before one.
//! - **A line the carrier cannot carry is dropped, never waited on** (`INV-05`): the dial, the read, the backoff and
//!   the redial all happen on the carrier's own thread, and the answer the session loop wrote is dropped rather than
//!   blocking the wall for a relay that went away.
//!
//! A wall with no relay is a supported state, not a failure to start: `ARCONS_RELAY_URL` unset is [`Uncarried::NoRelay`],
//! `--self-check` says `not now`, and the wall works locally as it always did.

use std::path::PathBuf;

use serde_json::Value;

use super::Refused;
use crate::config::Config;

// The version and the frame builder are the carrier's, and the carrier is a unix one: a Windows build has the
// vocabulary, the row and nothing to dial (`src/control.rs` says the same where it gates the module). Both imports
// are gated with it, or the gate's Windows job reports them as dead (#236).
#[cfg(unix)]
use super::PROTOCOL;
#[cfg(unix)]
use serde_json::json;

/// **The relay's frame names, in one place** (#196).
///
/// `docs/control.md` names them, and `tests/doc_claims.rs` fails when the two disagree in either direction — the same
/// tripwire the verb table has, because this is the other vocabulary a document is tempted to invent.
pub mod frame {
    /// The wall's **first** frame on every connection: who it is. Nothing else is sent until the relay answers.
    pub const HELLO: &str = "hello";
    /// The wall's own Minos credential, inside [`HELLO`]: what it already holds, in the shape its own file has.
    pub const CREDENTIAL: &str = "credential";
    /// The relay's answer to [`HELLO`]. **`true` is what "connected" means**; `false` is `the relay refused the wall`.
    pub const WELCOME: &str = "welcome";
    /// A phone's key, on every request the relay hands over: what makes a frame a **request**, and who it is from.
    pub const DEVICE: &str = "device";
    /// The device an **answer** is for: how the relay routes a frame back to the phone that asked.
    pub const TO: &str = "to";

    /// Every name above, so a document that describes one is checked against it, both ways (#196).
    pub const NAMES: &[&str] = &[HELLO, CREDENTIAL, WELCOME, DEVICE, TO];
}

/// Where the wall is told to dial. Named once, so the row, the log and the document cannot drift apart.
pub const URL_VARIABLE: &str = "ARCONS_RELAY_URL";

/// `ARCONS_RELAY_URL` unset: **a wall in a cupboard**, which is a wall that works, and says so.
pub const NO_RELAY: &str = "no relay is configured";

/// **The relay's address as an operator wrote it** (#270): `ARCONS_RELAY_URL`, trimmed, and `None` when it is
/// unset or blank — a blank URL is a relay nobody meant to point at.
///
/// One function reads the variable, so the carrier and the pairing QR cannot disagree about which string this
/// wall's relay is. The address is a deployment address and not a secret: the QR carries it (for the two minutes
/// a code is open), the `--self-check` row prints it, and no log line does (`INV-08`).
pub fn configured(env: &crate::config::Env) -> Option<String> {
    env.get(URL_VARIABLE)
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

/// The relay said no to the wall's credential. Not retried: the same credential is the same answer.
pub const REFUSED_WALL: &str = "the relay refused the wall";

/// Nothing answered — no connection, a connection that said nothing, or one that went away. Worth retrying.
pub const NOT_ANSWERING: &str = "the relay is not answering";

/// A request whose key is in no row of `devices.toml` (`INV-07`): answered to the relay, never carried to the wall.
pub const NOT_PAIRED: &str = "that device is not paired";

/// How long a dial, a hello and a welcome may take before the relay counts as not answering. Short, because the
/// wall keeps drawing while it waits and a relay that took five seconds is a relay with a problem.
pub const WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// The first redial wait, doubled up to [`BACKOFF_MAX`]. Small, so a relay that restarted is back in a beat.
pub const BACKOFF_START: std::time::Duration = std::time::Duration::from_millis(200);

/// The longest the wall waits between dials: a relay that is down for an afternoon costs one line every five seconds.
pub const BACKOFF_MAX: std::time::Duration = std::time::Duration::from_secs(5);

/// **Why the wall is not carrying a line to its relay**, by name (#196).
///
/// The three states `--self-check`'s `relay` row can be in, and the three refusals the carrier can end a session with.
/// Each names an action: wait ([`Uncarried::NotAnswering`]), fix the credential ([`Uncarried::Refused`]), or configure
/// a relay ([`Uncarried::NoRelay`]) — and every one is in `control::REFUSALS` with its kind (`INV-07`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uncarried {
    /// `ARCONS_RELAY_URL` is unset — the supported state a wall in a cupboard is in.
    NoRelay,
    /// The relay answered `welcome: false`: the wall's credential is not one it accepts.
    Refused,
    /// Nothing answered: no connection, no welcome, or a connection that went away.
    NotAnswering,
}

impl Uncarried {
    /// Every one, so a table over them is exhaustive rather than representative (`INV-07`).
    pub const ALL: &'static [Uncarried] = &[
        Uncarried::NoRelay,
        Uncarried::Refused,
        Uncarried::NotAnswering,
    ];

    /// The words, which are the refusal's name.
    pub fn words(self) -> &'static str {
        match self {
            Uncarried::NoRelay => NO_RELAY,
            Uncarried::Refused => REFUSED_WALL,
            Uncarried::NotAnswering => NOT_ANSWERING,
        }
    }

    /// **The same words as a refusal a client can branch on** (#198): the kind lives in `control::REFUSALS`.
    pub fn refusal(self) -> Refused {
        Refused(self.words())
    }
}

impl std::fmt::Display for Uncarried {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.words())
    }
}

/// **Where a relay is**, as this build can reach one (#196).
///
/// `unix:<path>` (or a bare path, which has no scheme to get wrong) is a relay beside the wall — the first deployment,
/// and what the tests drive. `tcp://host:port` is a relay on the network, without TLS: `wss://` needs a websocket
/// client, which is not this ticket, and is a relay that does not answer *this* wall rather than a fourth refusal name.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Dial {
    Unix(PathBuf),
    Tcp(String),
}

impl Dial {
    fn parse(url: &str) -> Option<Dial> {
        let url = url.trim();
        if let Some(rest) = url.strip_prefix("unix:") {
            let path = rest.trim_start_matches("//").trim();
            return (!path.is_empty()).then(|| Dial::Unix(PathBuf::from(path)));
        }
        if let Some(rest) = url.strip_prefix("tcp://") {
            let address = rest.trim();
            return (!address.is_empty()).then(|| Dial::Tcp(address.to_string()));
        }
        url.starts_with('/').then(|| Dial::Unix(PathBuf::from(url)))
    }
}

/// **What a carrier needs to dial** (#196): where, as whom, and who may speak.
///
/// The credential is the wall's **own** Minos credential (`config::Minos::relay_credential`): no wall-side secret is
/// invented, and `Debug` redacts it so a line about a relay cannot carry one (`INV-08`).
#[derive(Clone)]
pub struct Settings {
    url: String,
    dial: Dial,
    credential: Value,
    devices: PathBuf,
}

impl Settings {
    /// The three parts, checked: no URL is [`Uncarried::NoRelay`], and a URL this build has no carrier for is a relay
    /// that does not answer this wall.
    pub fn of(
        url: Option<&str>,
        credential: Value,
        devices: PathBuf,
    ) -> Result<Settings, Uncarried> {
        let Some(url) = url.map(str::trim).filter(|url| !url.is_empty()) else {
            return Err(Uncarried::NoRelay);
        };
        let dial = Dial::parse(url).ok_or(Uncarried::NotAnswering)?;
        Ok(Settings {
            url: url.to_string(),
            dial,
            credential,
            devices,
        })
    }

    /// From a wall's configuration and an environment: the URL from the environment, the credential from the
    /// configuration, and `devices.toml` from the machine. `None` for a configuration is a process that has not read
    /// one — a test, or a wall before `config::set_current` — and it has nothing to present, which the relay is the one
    /// to refuse.
    pub fn of_config(
        config: Option<&Config>,
        env: &crate::config::Env,
    ) -> Result<Settings, Uncarried> {
        let credential = config
            .map(|config| config.minos.relay_credential())
            .unwrap_or(Value::Null);
        Settings::of(
            configured(env).as_deref(),
            credential,
            crate::config::devices::devices_file(),
        )
    }

    /// This process's own, as the display starts: `ARCONS_RELAY_URL`, its configuration, `devices.toml`.
    pub fn from_process() -> Result<Settings, Uncarried> {
        Settings::of_config(
            crate::config::current(),
            &crate::config::Env::from_process(),
        )
    }

    /// The URL as an operator wrote it. Not a secret: it is where the relay is, and the row prints it.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// **One dial, one hello, one welcome** — whether the relay answers this wall, for `--self-check`. A running wall
    /// never calls this: its carrier dials on its own thread, and redials.
    #[cfg(unix)]
    pub fn reaches(&self) -> Result<(), Uncarried> {
        let mut carrier = RelayCarrier::new(self.clone());
        carrier.connect()?;
        carrier.hang_up();
        Ok(())
    }
}

/// The credential is redacted by construction, the same decision [`crate::config::Secret`] makes: a log line about a
/// relay is safe to write (`INV-08`).
impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Settings")
            .field("url", &self.url)
            .field("credential", &"<redacted>")
            .field("devices", &self.devices)
            .finish()
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The carrier. Unix only, because the one socket this build has is (`src/control.rs` says the same where it gates
// the modules): a Windows wall compiles the frames and the row, and has no relay to dial.
// ---------------------------------------------------------------------------------------------------------------

#[cfg(unix)]
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(unix)]
use std::sync::{Arc, Mutex};

#[cfg(unix)]
use super::carrier::Carrier;
#[cfg(unix)]
use super::server::serve_session;
#[cfg(unix)]
use crate::verb::Origin;

/// One end of a dialled connection: a unix socket beside the wall, or a TCP relay on the network.
#[cfg(unix)]
enum Wire {
    Unix(std::os::unix::net::UnixStream),
    Tcp(std::net::TcpStream),
}

#[cfg(unix)]
impl Wire {
    fn try_clone(&self) -> std::io::Result<Wire> {
        Ok(match self {
            Wire::Unix(stream) => Wire::Unix(stream.try_clone()?),
            Wire::Tcp(stream) => Wire::Tcp(stream.try_clone()?),
        })
    }

    /// **Wake a read that is blocked on a relay with nothing to say**: the trick the listener uses for a client that
    /// has stopped typing, and what lets a stopping wall end its carrier thread instead of waiting for a phone.
    fn shutdown(&self) {
        let _ = match self {
            Wire::Unix(stream) => stream.shutdown(std::net::Shutdown::Both),
            Wire::Tcp(stream) => stream.shutdown(std::net::Shutdown::Both),
        };
    }

    fn set_read_timeout(&self, timeout: Option<std::time::Duration>) -> std::io::Result<()> {
        match self {
            Wire::Unix(stream) => stream.set_read_timeout(timeout),
            Wire::Tcp(stream) => stream.set_read_timeout(timeout),
        }
    }

    fn give_up_within(&self) -> std::io::Result<()> {
        self.set_read_timeout(Some(WITHIN))?;
        let _ = match self {
            Wire::Unix(stream) => stream.set_write_timeout(Some(WITHIN)),
            Wire::Tcp(stream) => stream.set_write_timeout(Some(WITHIN)),
        };
        Ok(())
    }
}

#[cfg(unix)]
impl Read for Wire {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Wire::Unix(stream) => stream.read(buffer),
            Wire::Tcp(stream) => stream.read(buffer),
        }
    }
}

#[cfg(unix)]
impl Write for Wire {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self {
            Wire::Unix(stream) => stream.write(buffer),
            Wire::Tcp(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Wire::Unix(stream) => stream.flush(),
            Wire::Tcp(stream) => stream.flush(),
        }
    }
}

#[cfg(unix)]
impl Dial {
    fn open(&self) -> std::io::Result<Wire> {
        match self {
            Dial::Unix(path) => {
                let wire = Wire::Unix(std::os::unix::net::UnixStream::connect(path)?);
                wire.give_up_within()?;
                Ok(wire)
            }
            Dial::Tcp(address) => {
                use std::net::ToSocketAddrs;
                let mut last = std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "the relay's address resolves to nothing",
                );
                for candidate in address.to_socket_addrs()? {
                    match std::net::TcpStream::connect_timeout(&candidate, WITHIN) {
                        Ok(stream) => {
                            let wire = Wire::Tcp(stream);
                            wire.give_up_within()?;
                            return Ok(wire);
                        }
                        Err(error) => last = error,
                    }
                }
                Err(last)
            }
        }
    }
}

/// The frames on one connection: one JSON object per line, in both directions — the socket's framing, because it is
/// the same session loop at the other end of it.
#[cfg(unix)]
struct Connection {
    reader: BufReader<Wire>,
    writer: Wire,
}

#[cfg(unix)]
impl Connection {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(line)),
            Err(error) => Err(error),
        }
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        writeln!(self.writer, "{line}")?;
        self.writer.flush()
    }

    /// **The same read with a deadline** (#221), for a watch's beat. A deadline with **half a frame in hand** is not a
    /// deadline — the frame is on its way — so the read keeps going until the line ends or the deadline catches an
    /// empty one.
    fn read_line_within(&mut self, within: std::time::Duration) -> std::io::Result<Option<String>> {
        self.reader.get_ref().set_read_timeout(Some(within))?;
        let mut line = String::new();
        loop {
            match self.reader.read_line(&mut line) {
                Ok(0) => return Ok(None),
                Ok(_) => return Ok(Some(line)),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if super::watch::timed_out(&error) && line.is_empty() => {
                    return Err(error);
                }
                Err(error) if super::watch::timed_out(&error) => continue,
                Err(error) => return Err(error),
            }
        }
    }

    /// A relay with nothing to say is a relay waiting for a phone: after the welcome there is no deadline.
    fn wait(&mut self) -> std::io::Result<()> {
        self.reader.get_ref().set_read_timeout(None)
    }
}

/// **Which device an answer goes to** (#196, #262): the name a row of `devices.toml` gave, or — for one frame — no
/// name at all.
#[cfg(unix)]
#[derive(Clone, Debug, PartialEq, Eq)]
enum Address {
    /// A key whose hash a row holds: the answer goes back with that row's `name` in `to`, so the relay can route it.
    Named(String),
    /// **A phone that has not paired yet** (#262): its frame was `pair.claim`, the only verb a device with no row may
    /// send, and its answer goes back **on the connection it arrived on, with no `to`** — the only address a device
    /// with no name has, and the refusal's existing convention rather than a second addressing rule.
    Unnamed,
}

/// **The verb a frame's intent names** (#262), or `None` when it names none.
///
/// The gate is the **verb**, not "an intent that parses": a claim with a mistyped field still names the one verb a
/// device with no row may send, so it is carried and the session loop tells the phone what to fix — an `input` refusal
/// it can act on, rather than `that device is not paired`, which is a stranger's answer.
#[cfg(unix)]
fn verb_of(intent: &Value) -> Option<&str> {
    intent.get("verb").and_then(Value::as_str)
}

/// **The wall's end of a relay connection** (#196): one [`Carrier`], so the session loop does not know it is a relay.
/// `read_line` **never surfaces a drop**: it redials, with backoff, on this thread. The two things it does surface are
/// a refusal (`the relay refused the wall`, which no retry fixes) and the wall stopping (`Ok(None)`).
#[cfg(unix)]
pub struct RelayCarrier {
    settings: Settings,
    connection: Option<Connection>,
    /// The device the next answer is for: the phone that asked the question the session loop is answering. One
    /// question at a time, because the loop asks the wall and waits (`INV-05`).
    to: Option<Address>,
    /// **The device a watch is publishing to** (#221), if one is open. A publish has no request of its own to take a
    /// name from, and the name is the relay's routing field rather than the plane's, so the carrier keeps it: set when
    /// a paired frame asks for a watch, and cleared when the connection goes — **a watch does not resume across one**
    /// (`docs/mobile-remote.md` decision 1).
    watching: Option<String>,
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Option<Wire>>>,
    connected: Arc<AtomicBool>,
    /// The wait before the next dial, doubled on each failure and reset by a welcome.
    backoff: std::time::Duration,
    /// Whether the current outage has been said out loud, so a relay that is down for an afternoon logs one line and
    /// not one every five seconds.
    reported: bool,
}

#[cfg(unix)]
impl RelayCarrier {
    /// A carrier that has not dialled yet. `read_line` dials; [`RelayCarrier::connect`] dials now.
    pub fn new(settings: Settings) -> RelayCarrier {
        RelayCarrier::sharing(
            settings,
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(AtomicBool::new(false)),
        )
    }

    /// The same, over state the spawner can reach: the stop flag `Relay::drop` sets, the wire it shuts down to wake a
    /// blocked read, and the flag [`Relay::connected`] answers — the truth in one place rather than two.
    fn sharing(
        settings: Settings,
        stop: Arc<AtomicBool>,
        current: Arc<Mutex<Option<Wire>>>,
        connected: Arc<AtomicBool>,
    ) -> RelayCarrier {
        RelayCarrier {
            settings,
            connection: None,
            to: None,
            watching: None,
            stop,
            current,
            connected,
            backoff: BACKOFF_START,
            reported: false,
        }
    }

    /// **Dial, say hello, read the welcome** — the whole of "connected" (#196). Blocks, so it is called on the
    /// carrier's own thread and never in `view` (`INV-05`).
    pub fn connect(&mut self) -> Result<(), Uncarried> {
        if self.stop.load(Ordering::SeqCst) {
            return Err(Uncarried::NotAnswering);
        }
        let wire = self.settings.dial.open().map_err(|error| {
            // The URL, never the credential: a relay's address is a fact and its wall's key is not (`INV-08`).
            tracing::debug!(
                "relay: {} did not answer a connection ({error})",
                self.settings.url
            );
            Uncarried::NotAnswering
        })?;
        let writer = wire.try_clone().map_err(|_| Uncarried::NotAnswering)?;
        // Kept so a stopping wall can wake a read blocked here (see `Wire::shutdown`). Cleared on every drop.
        let wake = writer.try_clone().map_err(|_| Uncarried::NotAnswering)?;
        *self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(wake);
        let mut connection = Connection {
            reader: BufReader::new(wire),
            writer,
        };

        // **The wall says who it is first**: one frame, and nothing else until the relay answers it. The credential
        // is the wall's own Minos credential, in the shape its own file has — no wall-side secret is invented.
        let hello =
            json!({ "v": PROTOCOL, frame::HELLO: { frame::CREDENTIAL: self.settings.credential } });
        connection
            .write_line(&hello.to_string())
            .map_err(|_| Uncarried::NotAnswering)?;

        // **"Connected" is the relay's answer, not the dial.** A socket that opened and said nothing is a relay that
        // is not answering; one that answered `false` refused the wall's credential. The two are different fixes.
        let welcomed = match connection.read_line() {
            Ok(Some(line)) => serde_json::from_str::<Value>(&line)
                .ok()
                .and_then(|value| value.get(frame::WELCOME).and_then(Value::as_bool)),
            _ => None,
        };
        if welcomed != Some(true) {
            self.close();
            return Err(match welcomed {
                Some(false) => Uncarried::Refused,
                _ => Uncarried::NotAnswering,
            });
        }
        connection.wait().map_err(|_| Uncarried::NotAnswering)?;
        self.connection = Some(connection);
        self.connected.store(true, Ordering::SeqCst);
        self.backoff = BACKOFF_START;
        self.reported = false;
        tracing::info!("relay: connected to {}", self.settings.url);
        Ok(())
    }

    /// Whether the relay is answering: dialled, welcomed, and nothing has failed since.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    /// **Who is speaking** (#196): the phone's key, hashed and looked for in `devices.toml`.
    ///
    /// The file keeps the **hash**, so a leaked `devices.toml` is a list of names and cannot speak to the relay
    /// (`INV-08`) — and a frame that presents the hash instead of the key is refused like any other stranger. The file
    /// is read **per request**, on this carrier's own thread: a phone paired a second ago must drive the wall without a
    /// restart, and a few lines of TOML cannot block a frame (`INV-05`).
    fn paired(&self, key: Option<&str>) -> Option<String> {
        let hash = crate::config::devices::Key::from_hex(key?)?.hash();
        let (devices, _) = crate::config::devices::Devices::read(&self.settings.devices);
        devices?
            .device
            .into_iter()
            .find(|device| device.key == hash)
            .map(|device| device.name)
    }

    /// One frame from the relay: a request to carry, or `None` when there is nothing for the wall to see.
    ///
    /// A frame carrying a device key is a **request** and is carried **unchanged** — the session loop then reads it
    /// with `wire::request`, so there is one decoder and one vocabulary. A frame from a key no row holds is refused
    /// here, by name, and never reaches the wall.
    fn frame(&mut self, line: &str) -> Option<String> {
        let value: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            // Not JSON at all names no device, so there is nobody to attribute it to. Refused rather than guessed at
            // (`INV-07`): a request nobody authenticated is a request the wall does not answer.
            Err(_) => {
                self.refuse(NOT_PAIRED);
                return None;
            }
        };
        // The relay's own traffic, not a phone's: a repeated welcome is a relay saying hello, and there is nothing to
        // carry. (The first one is read by `connect`, and is what "connected" means.)
        if value.get(frame::WELCOME).is_some() {
            return None;
        }
        match self.paired(value.get(frame::DEVICE).and_then(Value::as_str)) {
            Some(name) => {
                // **A watch is a stream, so its device outlives the request that asked for it** (#221): the publishes
                // that follow have no request to take a name from, and the relay routes on the name. Only a watch
                // *this wall carries* starts one: a malformed ask is refused by the session, and the publishes the
                // carrier would otherwise address to it belong to nobody.
                if value
                    .get("watch")
                    .and_then(Value::as_str)
                    .and_then(super::watch::Watch::of_word)
                    .is_some()
                {
                    self.watching = Some(name.clone());
                }
                self.to = Some(Address::Named(name));
                Some(line.to_string())
            }
            // **One verb wide** (#262): a device with no row may send `pair.claim` — a phone in the room, with six
            // digits read off the screen, has no key yet, which is the whole point of the frame. The answer has no
            // `to`, because the wall has no name for the device until the claim writes one. Any other verb from the
            // same key is still a stranger, and `a_phone_with_no_row_may_claim_the_code_and_nothing_else` holds both
            // halves of that.
            None if value.get("intent").and_then(verb_of) == Some(crate::verb::PAIR_CLAIM) => {
                self.to = Some(Address::Unnamed);
                Some(line.to_string())
            }
            None => {
                self.refuse(NOT_PAIRED);
                None
            }
        }
    }

    /// Refuse a frame the relay handed over: the wire's own answer, with no `to`, because the wall cannot name a
    /// device it has no row for.
    fn refuse(&mut self, why: &'static str) {
        let line = super::wire::reply(super::wire::Answer::Changed(Err(Refused(why))));
        self.publish(&line);
    }

    /// Write one frame, or **drop it** (`INV-05`): a relay that went away must not make the wall wait, so a failed
    /// write costs the line and nothing else — the next `read_line` redials and the wall keeps drawing.
    fn publish(&mut self, line: &str) {
        let written = match self.connection.as_mut() {
            Some(connection) => connection.write_line(line).is_ok(),
            None => false,
        };
        if !written {
            self.dropped();
        }
    }

    /// The connection is gone: forget it, and wake nothing.
    fn dropped(&mut self) {
        self.connection = None;
        self.to = None;
        // **A watch does not resume**: a connection that came back knows nothing, and the phone reads before it asks
        // again (`docs/mobile-remote.md` decision 1, #221).
        self.watching = None;
        self.connected.store(false, Ordering::SeqCst);
        self.close();
    }

    /// Shut the wire down and hand it back, so nothing holds a socket the relay thinks is a wall.
    fn close(&mut self) {
        if let Some(wire) = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            wire.shutdown();
        }
    }

    /// Wait before the next dial, in slices so a stopping wall does not wait out a five-second backoff to end its
    /// carrier thread.
    fn back_off(&mut self, why: Uncarried) {
        let delay = self.backoff;
        self.backoff = (self.backoff * 2).min(BACKOFF_MAX);
        if !self.reported {
            tracing::warn!("relay: {why}; redialling in {} ms", delay.as_millis());
            self.reported = true;
        } else {
            tracing::debug!("relay: still {why}");
        }
        let until = std::time::Instant::now() + delay;
        while std::time::Instant::now() < until {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep((until - std::time::Instant::now()).min(BACKOFF_START));
        }
    }
}

#[cfg(unix)]
impl Carrier for RelayCarrier {
    /// **A relay carries a watch** (#221): the connection is open already — it is how a phone reaches this wall at
    /// all — so a stream of lines costs it nothing it was not already paying. The socket beside the wheel does not,
    /// and says so by name.
    fn carries_a_watch(&self) -> bool {
        true
    }

    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(None);
            }
            if self.connection.is_none() {
                match self.connect() {
                    Ok(()) => {}
                    // **A refused credential is not retried** (#196): the relay has said no, and asking again with
                    // the same credential is a lockout loop. The session ends with the name, and the wall works
                    // locally until an operator fixes it.
                    Err(Uncarried::Refused) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            REFUSED_WALL,
                        ));
                    }
                    Err(why) => {
                        self.back_off(why);
                        continue;
                    }
                }
            }
            let read = match self.connection.as_mut() {
                Some(connection) => connection.read_line(),
                None => Ok(None),
            };
            match read {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Some(line) = self.frame(&line) {
                        return Ok(Some(line));
                    }
                    // Nothing to carry to the wall: a refusal written back to the relay, or the relay's own traffic.
                    continue;
                }
                Ok(None) => tracing::debug!("relay: the connection closed"),
                // A socket that failed, not a frame: nothing a client did, and nothing to carry.
                Err(error) => tracing::debug!("relay: the connection failed ({error})"),
            }
            // A relay that hung up, or a socket that failed: the connection is gone, and the next pass redials it.
            if self.stop.load(Ordering::SeqCst) {
                return Ok(None);
            }
            self.dropped();
            self.back_off(Uncarried::NotAnswering);
        }
    }

    /// **The same read, waiting at most `within`** (#221): a watch's beat. A deadline here is **not** a relay that
    /// went away — the connection is still good and the wall simply has nothing to publish yet — so it is handed back
    /// rather than redialled, and the beat asks the wall again.
    fn read_line_within(&mut self, within: std::time::Duration) -> std::io::Result<Option<String>> {
        loop {
            if self.stop.load(Ordering::SeqCst) {
                return Ok(None);
            }
            if self.connection.is_none() {
                match self.connect() {
                    Ok(()) => {}
                    Err(Uncarried::Refused) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::PermissionDenied,
                            REFUSED_WALL,
                        ));
                    }
                    Err(why) => {
                        self.back_off(why);
                        continue;
                    }
                }
            }
            let read = match self.connection.as_mut() {
                Some(connection) => connection.read_line_within(within),
                None => Ok(None),
            };
            match read {
                Ok(Some(line)) => {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Some(line) = self.frame(&line) {
                        return Ok(Some(line));
                    }
                    continue;
                }
                Ok(None) => tracing::debug!("relay: the connection closed"),
                // **The beat's own deadline**: nothing from the relay inside it, which is a relay waiting for a phone.
                Err(error) if super::watch::timed_out(&error) => return Err(error),
                Err(error) => tracing::debug!("relay: the connection failed ({error})"),
            }
            if self.stop.load(Ordering::SeqCst) {
                return Ok(None);
            }
            self.dropped();
            self.back_off(Uncarried::NotAnswering);
        }
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        // **The answer's own line, with the device it is for on it** (`frame::TO`): `wire::reply` wrote the rest, so
        // the relay speaks the shape the socket speaks, and there is one encoder rather than two.
        //
        // **A publish has no request to take a name from** (#221), so it goes to the device watching — which is also
        // why a line with neither a `to` nor a watcher is dropped: after a redial the watch is gone by design, and a
        // phone that has not read and asked again is not watching anything.
        let to = self
            .to
            .take()
            .or_else(|| self.watching.clone().map(Address::Named));
        match serde_json::from_str::<Value>(line) {
            Ok(mut value) => match to {
                Some(Address::Named(to)) => {
                    value[frame::TO] = json!(to);
                    self.publish(&value.to_string());
                }
                // **A frame with no `to` goes back on the connection it arrived on** (#262): the refusal's own
                // convention, extended to `pair.claim`'s answer — the one grant that can precede a name. No second
                // addressing rule, and no invented field: the line `wire::reply` wrote, verbatim.
                Some(Address::Unnamed) => self.publish(&value.to_string()),
                None => tracing::debug!("relay: dropped a line with no device to carry it to"),
            },
            // Not a frame this wall wrote: dropping it is better than writing half a frame to a relay.
            Err(_) => tracing::debug!("relay: dropped a line that is not a frame"),
        }
        // `Ok` even when it was dropped: the line is the carrier's loss, and a session that ended for it would leave
        // the relay unable to reach the wall again until the wall restarted (`INV-05`).
        Ok(())
    }

    fn hang_up(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.dropped();
    }

    /// **The device behind the frame just read** (#222): the name `devices.toml` holds beside the key's hash — so a
    /// cursor, a refusal and a log line name the same thing. Never the key, the hash or the relay's address
    /// (`INV-08`): a name a phone chose in the room is the only thing here that is the phone's.
    fn origin(&self) -> Option<Origin> {
        match self.to.as_ref()? {
            Address::Named(name) => Some(Origin::named(name)),
            // **A claim has no name yet** (#262): until the wall writes the row there is nobody to name, the request
            // arrives at the door with no hand, and a claim moves no cursor.
            Address::Unnamed => None,
        }
    }
}

/// **The running carrier** (#196): a thread, a way to stop, and an end of the **wall's** queue (#222).
///
/// The shape [`super::server::Server`] has, and the same queue: a phone's request waits in the wall's one channel
/// beside a `ctl`'s, so `poll` applies them in the order they arrived. The difference is that this one *dials*.
#[cfg(unix)]
pub struct Relay {
    stop: Arc<AtomicBool>,
    current: Arc<Mutex<Option<Wire>>>,
    connected: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[cfg(unix)]
impl Relay {
    /// Dial in the background and serve for as long as the wall runs, handing every request to `queue` — the wall's.
    /// Called only by a wall that is **the** wall: a viewer draws somebody else's state, and two processes publishing
    /// one relay would be two answers to one device.
    pub fn spawn(settings: Settings, queue: &super::sessions::Queue) -> std::io::Result<Relay> {
        let tx = queue.sender();
        let stop = Arc::new(AtomicBool::new(false));
        let current: Arc<Mutex<Option<Wire>>> = Arc::new(Mutex::new(None));
        let connected = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            let serving = Arc::clone(&stop);
            let current = Arc::clone(&current);
            let connected = Arc::clone(&connected);
            std::thread::Builder::new()
                .name("relay".to_string())
                .spawn(move || {
                    let carrier = RelayCarrier::sharing(settings, stop, current, connected);
                    // **The same session loop the unix socket uses** (#195): a request in, a `Job` for the wall, an
                    // answer out. Nothing about intents or answers is spelled twice.
                    match serve_session(carrier, &tx, &serving) {
                        Ok(()) => tracing::info!("relay: carrier stopped"),
                        // The name, never a frame: `the relay refused the wall` is a sentence and a credential is not
                        // (`INV-08`).
                        Err(error) => tracing::warn!("relay: {error}"),
                    }
                })?
        };
        Ok(Relay {
            stop,
            current,
            connected,
            thread: Some(thread),
        })
    }

    /// Whether the relay is answering — the fact the log line and a test read, rather than a claim.
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }
}

#[cfg(unix)]
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // A read blocked on a relay with nothing to say is woken first: the alternative is a `join` that waits for a
        // phone to press something.
        if let Some(wire) = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            wire.shutdown();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// **The `relay` row of `--self-check`** (#196): configured, unreachable, or not now.
///
/// A **fact, not a verdict**, like `control` and `authority`: it never changes the exit code. A wall in a cupboard
/// with no relay must pass every other row, and a wall whose relay is down is a wall that still draws the sea.
#[cfg(unix)]
pub fn row(config: Option<&Config>, env: &crate::config::Env) -> String {
    match Settings::of_config(config, env) {
        Err(Uncarried::NoRelay) => format!("not now ({NO_RELAY}) — this wall works locally"),
        Err(why) => why.to_string(),
        Ok(settings) => match settings.reaches() {
            Ok(()) => format!("configured at {} (the relay answers)", settings.url()),
            Err(why) => format!("configured at {}, but {why}", settings.url()),
        },
    }
}

/// Windows has the vocabulary and no carrier (`src/control.rs` says the same where it gates the modules).
#[cfg(not(unix))]
pub fn row(_config: Option<&Config>, _env: &crate::config::Env) -> String {
    "not available on this platform: the relay's carrier is a unix socket here".to_string()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::config::devices::{Device, Devices, Key, VERSION};
    use crate::control::Job;
    use crate::control::pair::PAIR_ATTEMPTS;
    use crate::control::wire::{self, Answer, Request};
    use crate::control::{Changed, RefusalKind, refusal_kind};
    use crate::verb::Intent;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// **A credential that must never be printed**: not by a `Debug`, not by a row, not by an error (`INV-08`).
    const CREDENTIAL: &str = "canary-relay-credential-never-print";

    /// Long enough for a redial with backoff on a slow machine, short enough that a wedged test fails rather than
    /// hangs.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// A directory of its own for one test, removed on drop — the rule `config::layout`'s tests keep, so two runs of
    /// the gate cannot collide.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let path =
                std::env::temp_dir().join(format!("arcons-cc-relay-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("a temp directory");
            TempDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// **The relay's side of the wire**: a unix listener standing in for the deployment's service, because the
    /// ticket's proof is that the wall's *real* carrier is driven (#196).
    struct Fake {
        /// The directory the socket lives in, owned so the socket's path survives the conversation.
        _dir: TempDir,
        listener: UnixListener,
        path: PathBuf,
    }

    impl Fake {
        fn start(name: &str) -> Fake {
            let dir = TempDir::new(name);
            let path = dir.path().join("relay.sock");
            let listener = UnixListener::bind(&path).expect("the fake relay binds");
            // A test that blocks is a test that tells you nothing: every dialect below has a deadline.
            listener
                .set_nonblocking(true)
                .expect("the fake relay listens without blocking");
            Fake {
                _dir: dir,
                listener,
                path,
            }
        }

        fn url(&self) -> String {
            format!("unix:{}", self.path.display())
        }

        /// A wall's settings pointed at this relay, with `devices.toml` at `devices`.
        fn settings(&self, devices: &Path) -> Settings {
            Settings::of(
                Some(&self.url()),
                json!({ "refresh_token": CREDENTIAL }),
                devices.to_path_buf(),
            )
            .expect("a URL this build dials")
        }

        /// A phone called `phone` whose key this test holds, written into `into` — a directory of the test's own,
        /// because this is the **wall's** file and not the relay's. **The file keeps the hash**, as a wall's does.
        fn pair_phone(&self, into: &Path) -> (PathBuf, String) {
            let key = Key::from_hex(&"ab".repeat(32)).expect("32 bytes of hex");
            let devices = Devices {
                version: VERSION,
                device: vec![Device {
                    name: "phone".to_string(),
                    key: key.hash(),
                    paired_at: "2026-11-15T00:00:00Z".to_string(),
                }],
            };
            let path = into.join("devices.toml");
            devices.write(&path).expect("devices.toml is written");
            (path, key.hex())
        }

        /// The wall, dialling. Bounded, so a carrier that never dials fails with a sentence instead of hanging.
        fn claim(&self) -> End {
            let deadline = Instant::now() + PATIENCE;
            while Instant::now() < deadline {
                match self.listener.accept() {
                    Ok((stream, _)) => return End::new(stream),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("the fake relay could not accept: {error}"),
                }
            }
            panic!("the wall never dialled the relay (#196)");
        }

        /// Whether **nobody** dialled within `within` — the falsifier of a retry loop.
        fn quiet_for(&self, within: Duration) -> bool {
            let deadline = Instant::now() + within;
            while Instant::now() < deadline {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.shutdown(std::net::Shutdown::Both);
                        return false;
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(5)),
                }
            }
            true
        }
    }

    /// The relay's end of one connection.
    struct End {
        reader: BufReader<UnixStream>,
        writer: UnixStream,
    }

    impl End {
        fn new(stream: UnixStream) -> End {
            stream
                .set_read_timeout(Some(PATIENCE))
                .expect("a deadline on every read");
            let writer = stream.try_clone().expect("one socket, two handles");
            End {
                reader: BufReader::new(stream),
                writer,
            }
        }

        /// The next frame the wall wrote.
        fn recv(&mut self) -> String {
            let mut line = String::new();
            self.reader
                .read_line(&mut line)
                .expect("the wall writes a frame");
            assert!(
                !line.is_empty(),
                "the wall hung up where a frame was expected (#196)"
            );
            line.trim().to_string()
        }

        /// The hello, checked, and the welcome written back: one function so every test says the same thing about
        /// what "connected" means.
        fn welcome(&mut self) {
            let hello = self.recv();
            let value: Value = serde_json::from_str(&hello).expect("the hello is JSON");
            assert_eq!(
                value["v"],
                json!(PROTOCOL),
                "the hello carries the protocol version (#196)"
            );
            assert_eq!(
                value[frame::HELLO][frame::CREDENTIAL]["refresh_token"],
                json!(CREDENTIAL),
                "the wall authenticates as the wall, with the credential it already holds (#196, INV-08)"
            );
            self.send(&format!(r#"{{"v":{PROTOCOL},"welcome":true}}"#));
        }

        fn send(&mut self, line: &str) {
            writeln!(self.writer, "{line}").expect("the fake relay writes a frame");
        }

        /// Whether **nothing** arrives within `within`: the falsifier of a carrier that answered for the wall, and of
        /// one that redialled a credential the relay had refused (`INV-05`, #196).
        fn quiet_for(&mut self, within: Duration) -> bool {
            let before = self.reader.get_ref().read_timeout().ok().flatten();
            assert!(
                self.reader.get_ref().set_read_timeout(Some(within)).is_ok(),
                "a deadline on the read"
            );
            let mut line = String::new();
            let quiet = match self.reader.read_line(&mut line) {
                Ok(_) => false,
                Err(error) => matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ),
            };
            let _ = self.reader.get_ref().set_read_timeout(before);
            quiet
        }

        /// Whether nothing arrives within `within`, and what did if something did.
        fn quiet_line(&mut self, within: Duration) -> (bool, String) {
            let before = self.reader.get_ref().read_timeout().ok().flatten();
            let _ = self.reader.get_ref().set_read_timeout(Some(within));
            let mut line = String::new();
            let quiet = match self.reader.read_line(&mut line) {
                Ok(0) => (true, String::new()),
                Ok(_) => (false, line.trim().to_string()),
                Err(error) => (
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ),
                    String::new(),
                ),
            };
            let _ = self.reader.get_ref().set_read_timeout(before);
            quiet
        }

        /// One request, as the relay forwards it: the wire's own request, with the phone's key on it.
        fn request(&mut self, key: &str, intent: &str) {
            self.send(&format!(
                r#"{{"v":{PROTOCOL},"device":"{key}","intent":{intent}}}"#
            ));
        }

        /// The answer, read back as a value so the fields can be asked about.
        fn answer(&mut self) -> Value {
            serde_json::from_str(&self.recv()).expect("the answer is JSON")
        }
    }

    /// **The wall dials, says who it is, and "connected" is the relay's answer** (#196).
    #[test]
    fn a_relay_carrier_dials_and_says_hello_with_the_walls_own_credential() {
        let fake = Fake::start("hello");
        let wall = TempDir::new("hello-wall");
        let (devices, _) = fake.pair_phone(wall.path());
        let settings = fake.settings(&devices);
        let relay = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            end
        });

        let mut carrier = RelayCarrier::new(settings);
        assert!(
            !carrier.is_connected(),
            "a carrier that has not dialled is not connected (#196)"
        );
        carrier
            .connect()
            .expect("a welcome is what connected means (#196)");
        assert!(carrier.is_connected(), "the relay welcomed the wall");
        drop(relay.join().expect("the relay's half ends"));
    }

    /// **A phone's request is the wire's own request**, handed to the session loop unchanged (#196): one vocabulary,
    /// `wire::Request`, and the device's key is the only thing the relay added.
    #[test]
    fn a_request_from_a_paired_phone_is_carried_as_the_same_wire_request() {
        let fake = Fake::start("request");
        let wall = TempDir::new("request-wall");
        let (devices, key) = fake.pair_phone(wall.path());
        let settings = fake.settings(&devices);
        let intent = r#"{"verb":"workspace.new"}"#;
        let sent = format!(r#"{{"v":{PROTOCOL},"device":"{key}","intent":{intent}}}"#);
        let forwarded = sent.clone();
        let relay = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            end.send(&forwarded);
            end
        });

        let mut carrier = RelayCarrier::new(settings);
        let line = carrier
            .read_line()
            .expect("no error")
            .expect("a request from a paired phone");
        assert_eq!(
            line.trim_end(),
            sent,
            "the carrier carries the relay's frame as it arrived, line ending and all: a second spelling of a \
             request is the second vocabulary #196 forbids"
        );
        assert_eq!(
            wire::request(&line),
            Ok(Request::Intent(Intent::NewWorkspace)),
            "what the carrier carries is a `wire::Request` and nothing else (#196, INV-07)"
        );
        // **And whose hand it is** (#222): the device's *name*, from the row the key's hash matched — so the session
        // stamps a cursor, a refusal and a log line with the same thing, and never with the key, the hash or the
        // relay's address (`INV-08`).
        assert_eq!(
            carrier.origin(),
            Some(crate::verb::Origin::named("phone")),
            "a relay's carrier names its client by the device name in devices.toml (#222 rule 4)"
        );
        drop(relay.join().expect("the relay's half ends"));
    }

    /// **An answer goes back to the device that asked**, with the device's *name* on it — the wall has a name for the
    /// key it verified, and the relay routes on it. Apart from that field the frame is `wire::reply`'s own line.
    #[test]
    fn an_answer_goes_back_to_the_device_that_asked() {
        let fake = Fake::start("answer");
        let wall = TempDir::new("answer-wall");
        let (devices, key) = fake.pair_phone(wall.path());
        let settings = fake.settings(&devices);
        let relay = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            end.request(&key, r#"{"verb":"workspace.new"}"#);
            end.answer()
        });

        let mut carrier = RelayCarrier::new(settings);
        let line = carrier
            .read_line()
            .expect("no error")
            .expect("a request from a paired phone");
        let request = wire::request(&line).expect("a wire request");
        assert_eq!(request, Request::Intent(Intent::NewWorkspace));

        // The UI thread's answer, written through the carrier the way `serve_session` writes it.
        let answer = Answer::Changed(Ok(Changed(true)));
        carrier
            .write_line(&wire::reply(answer.clone()))
            .expect("the answer is written, or dropped, never blocking");

        let relay = relay.join().expect("the relay's half ends");
        assert_eq!(
            relay["v"],
            json!(PROTOCOL),
            "the answer carries the protocol version"
        );
        assert_eq!(
            relay[frame::TO],
            json!("phone"),
            "the answer names the device it is for, so the relay can route it (#196)"
        );
        // And nothing else was added: the frame minus `to` is the line `wire::reply` wrote.
        let mut stripped = relay.clone();
        stripped
            .as_object_mut()
            .expect("an object")
            .remove(frame::TO);
        assert_eq!(
            stripped.to_string(),
            wire::reply(answer),
            "the relay's answer is the socket's answer with one field on it: one encoder, not two (#196)"
        );
    }

    /// **A key no row holds is refused, and the wall never sees the request** (#196, `INV-07`): a revoked phone, a
    /// stranger, and a phone that presented the *hash* its own file keeps are all `that device is not paired`.
    #[test]
    fn a_key_no_row_in_devices_holds_is_refused_by_name() {
        let fake = Fake::start("stranger");
        let wall = TempDir::new("stranger-wall");
        let (devices, key) = fake.pair_phone(wall.path());
        // What `devices.toml` holds is a hash: presenting it is not presenting the key (`INV-08`).
        let hash = Key::from_hex(&key).expect("a key").hash();
        let stranger = "cd".repeat(32);
        let settings = fake.settings(&devices);
        let good =
            format!(r#"{{"v":{PROTOCOL},"device":"{key}","intent":{{"verb":"workspace.new"}}}}"#);
        let forwarded = good.clone();
        let relay = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            end.request(&hash, r#"{"verb":"workspace.new"}"#);
            end.request(&stranger, r#"{"verb":"workspace.new"}"#);
            end.send(&forwarded);
            let first = end.answer();
            let second = end.answer();
            (first, second, end)
        });

        let mut carrier = RelayCarrier::new(settings);
        let line = carrier
            .read_line()
            .expect("no error")
            .expect("the paired phone's request is carried");
        assert_eq!(
            line.trim_end(),
            good,
            "the wall is handed the paired phone's request, and only that: a request from a device no row holds must \
             not reach it (#196)"
        );

        let (first, second, _) = relay.join().expect("the relay's half ends");
        for refusal in [first, second] {
            assert_eq!(
                refusal["ok"],
                json!(false),
                "a request from a device this wall does not know is refused (#196)"
            );
            assert_eq!(
                refusal["why"],
                json!(NOT_PAIRED),
                "the refusal is the rule's name, not prose (`INV-07`)"
            );
            assert_eq!(
                refusal["kind"],
                json!("rule"),
                "and its kind, so the relay can decide without reading the words (#198)"
            );
            assert!(
                refusal.get(frame::TO).is_none(),
                "a device the wall cannot name is not addressed: there is no row to take a name from"
            );
        }
    }

    /// **A dropped connection is redialled, with the wall's credential again, and the wall carries on** (#196): the
    /// relay going away is not the session ending.
    #[test]
    fn a_dropped_connection_is_redialled_and_the_wall_carries_on() {
        let fake = Fake::start("redial");
        let wall = TempDir::new("redial-wall");
        let (devices, key) = fake.pair_phone(wall.path());
        let settings = fake.settings(&devices);
        let relay = std::thread::spawn(move || {
            let mut first = fake.claim();
            first.welcome();
            first.request(&key, r#"{"verb":"workspace.new"}"#);
            let answered = first.answer();
            // The relay goes away: a restart, a redeploy, a wire that was cut.
            drop(first);

            // And the wall dials again, says who it is again, and is served again.
            let mut second = fake.claim();
            second.welcome();
            second.request(&key, r#"{"verb":"close_others","x":0}"#);
            let again = second.answer();
            (answered, again)
        });

        let mut carrier = RelayCarrier::new(settings);
        let first = carrier
            .read_line()
            .expect("no error")
            .expect("the first request");
        assert_eq!(
            wire::request(&first),
            Ok(Request::Intent(Intent::NewWorkspace))
        );
        carrier
            .write_line(&wire::reply(Answer::Changed(Ok(Changed(true)))))
            .expect("the first answer");

        let second = carrier
            .read_line()
            .expect("a drop is not an error: the carrier redials (#196)")
            .expect("a request after the redial");
        assert!(
            wire::request(&second).is_err(),
            "the second frame is the relay's own, read by the wire's own decoder: {second}"
        );
        carrier
            .write_line(&wire::reply(Answer::Changed(Err(Refused(
                super::super::NOT_NEIGHBOURS,
            )))))
            .expect("the second answer");

        let (answered, again) = relay.join().expect("the relay's half ends");
        assert_eq!(answered[frame::TO], json!("phone"));
        assert_eq!(
            again[frame::TO],
            json!("phone"),
            "the redialled connection serves the same device (#196)"
        );
        assert!(
            again["ok"] == json!(false),
            "the second answer is the wall's refusal, carried over the new connection"
        );
    }

    /// **A relay that refuses the wall's credential is a named refusal and not a retry loop** (#196): the same
    /// credential asked again is the same answer, and a wall that looped would be a lockout.
    #[test]
    fn a_relay_that_refuses_the_walls_credential_is_named_and_not_retried() {
        let fake = Fake::start("refused");
        let wall = TempDir::new("refused-wall");
        let (devices, _) = fake.pair_phone(wall.path());
        let settings = fake.settings(&devices);
        let relay = std::thread::spawn(move || {
            let mut end = fake.claim();
            let hello: Value = serde_json::from_str(&end.recv()).expect("the hello is JSON");
            assert_eq!(
                hello[frame::HELLO][frame::CREDENTIAL]["refresh_token"],
                json!(CREDENTIAL)
            );
            end.send(&format!(r#"{{"v":{PROTOCOL},"welcome":false}}"#));
            // Ten times the first backoff: a retry loop would have dialled again by now.
            assert!(
                fake.quiet_for(BACKOFF_START * 10),
                "the wall dialled again after `{REFUSED_WALL}`: a refused credential is not retried (#196)"
            );
        });

        let mut carrier = RelayCarrier::new(settings);
        let error = carrier
            .read_line()
            .expect_err("a refused credential ends the session");
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::PermissionDenied,
            "the error is a refusal, not a broken line (#196)"
        );
        assert_eq!(
            error.to_string(),
            REFUSED_WALL,
            "the refusal is the wall's name for it, and a client can branch on it (`INV-07`)"
        );
        assert!(!carrier.is_connected(), "nothing is connected");
        relay.join().expect("the relay's half ends");
    }

    /// **No relay configured is a supported state** (#196): the names are the three the ticket gave, each with a kind,
    /// and none of them is a wall that cannot start.
    #[test]
    fn no_relay_is_a_named_state_and_three_refusals_have_kinds() {
        let devices = PathBuf::from("/nonexistent/devices.toml");
        assert_eq!(
            Settings::of(None, json!({}), devices.clone()).expect_err("no URL, no relay"),
            Uncarried::NoRelay
        );
        assert_eq!(
            Settings::of(Some("   "), json!({}), devices.clone()).expect_err("blank is unset"),
            Uncarried::NoRelay,
            "a blank `{URL_VARIABLE}` is a relay nobody meant to point at"
        );
        assert_eq!(
            Uncarried::NoRelay.refusal(),
            Refused("no relay is configured"),
            "`no relay is configured` is the name, and the table can classify it"
        );
        assert_eq!(
            Uncarried::Refused.refusal(),
            Refused("the relay refused the wall"),
            "the relay's `welcome: false` is the wall's own name for it (`INV-07`)"
        );
        assert_eq!(
            Uncarried::NotAnswering.refusal(),
            Refused("the relay is not answering"),
            "a relay that never answered is the third name, and it is the one worth retrying"
        );
        assert_eq!(
            Refused(NOT_PAIRED),
            Refused("that device is not paired"),
            "a request from a key no row holds is refused by this name, on the connection it arrived on"
        );
        for (name, kind) in [
            (NO_RELAY, RefusalKind::Rule),
            (REFUSED_WALL, RefusalKind::Rule),
            (NOT_ANSWERING, RefusalKind::Busy),
            (NOT_PAIRED, RefusalKind::Rule),
        ] {
            assert_eq!(
                refusal_kind(name),
                Some(kind),
                "{name} has no kind in `control::REFUSALS` (`INV-07`): a client that has to read the words to \
                 decide between waiting and fixing a key is a client coupled to a sentence"
            );
        }
        // The three forms the document names are the three this build dials, and a fourth is nobody's guess.
        assert_eq!(
            Dial::parse("unix:///run/relay.sock"),
            Some(Dial::Unix(PathBuf::from("/run/relay.sock"))),
            "`unix:///path` and `unix:/path` are one address, because an operator typing either means the same"
        );
        assert_eq!(
            Dial::parse("unix:/run/relay.sock"),
            Some(Dial::Unix(PathBuf::from("/run/relay.sock")))
        );
        assert_eq!(
            Dial::parse("/run/relay.sock"),
            Some(Dial::Unix(PathBuf::from("/run/relay.sock"))),
            "a bare path is a relay beside the wall: the first deployment, and it has no scheme to get wrong"
        );
        assert_eq!(
            Dial::parse("tcp://relay.example.test:7030"),
            Some(Dial::Tcp("relay.example.test:7030".to_string()))
        );
        assert!(
            Settings::of(
                Some("tcp://relay.example.test:7030"),
                json!({}),
                PathBuf::from("/dev/null")
            )
            .is_ok(),
            "a `tcp://` relay is one this wall dials"
        );
        // And a URL this build has no carrier for is a relay that does not answer *this* wall.
        assert_eq!(
            Settings::of(Some("wss://relay.example.test/"), json!({}), devices)
                .expect_err("no websocket carrier"),
            Uncarried::NotAnswering
        );
    }

    /// **The relay runs the session loop the socket runs** (#196): a request arrives as a `Job`, the wall answers it
    /// on its own thread, and nothing was answered before the wall did (`INV-05`).
    #[test]
    fn the_relay_serves_the_same_session_loop_the_socket_does() {
        let fake = Fake::start("session");
        let wall = TempDir::new("session-wall");
        let (devices, key) = fake.pair_phone(wall.path());
        let settings = fake.settings(&devices);
        let (looked, looking) = std::sync::mpsc::channel();
        let relay_half = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            end.request(&key, r#"{"verb":"workspace.new"}"#);
            // **Nothing is answered before the wall answers** (`INV-05`): the carrier's thread hands the request over
            // and waits for the wall, so a relay that wrote its own answer here would be a second authority. The
            // observation is sequenced: the wall does not answer until this thread has looked.
            let quiet = end.quiet_for(Duration::from_millis(300));
            let _ = looked.send(quiet);
            end.answer()
        });

        // **The wall's queue** (#222): the relay is handed an end of it, exactly as the socket's listener is.
        let queue = crate::control::sessions::Queue::new();
        let relay = Relay::spawn(settings, &queue).expect("the carrier thread starts");
        let job = {
            let deadline = Instant::now() + PATIENCE;
            loop {
                if let Some(job) = queue.next() {
                    break job;
                }
                assert!(
                    Instant::now() < deadline,
                    "the relay never handed the wall a request (#196)"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        assert_eq!(
            job.request(),
            Some(&Request::Intent(Intent::NewWorkspace)),
            "the job is the wire's request, unchanged (#196)"
        );
        assert!(
            looking
                .recv_timeout(PATIENCE)
                .expect("the relay's half reports what it saw"),
            "the relay answered before the wall did: something applied a change off the UI thread (INV-05)"
        );
        // The UI thread, played by this test: apply, then answer, through the one door.
        let mut state = crate::ui::State::default();
        assert!(
            crate::control::authority::serve(&mut state, job),
            "a new workspace is a change"
        );
        assert_eq!(
            state.workspaces.len(),
            4,
            "the wall changed once, on the thread that owns it"
        );

        let answer = relay_half.join().expect("the relay's half ends");
        assert_eq!(answer["ok"], json!(true));
        assert_eq!(answer["changed"], json!(true));
        assert_eq!(answer[frame::TO], json!("phone"));
        drop(relay);
    }

    /// **What the relay's half of a watched connection saw**, reported as it happens: a test cannot both answer the
    /// wall's beats and block on the phone's side of one socket.
    enum Report {
        /// The answer to the watch request.
        Ack(Value),
        /// A projection the wall published.
        Published(Value),
        /// Whether nothing arrived after the publish, within a beat and a half — and what did, if anything.
        Quiet(bool, String),
    }

    /// **The fake relay carries a watch** (#221): a phone asks, the wall grants it, a change is **published to that
    /// phone**, and a wall that changed nothing writes nothing at all.
    ///
    /// The two falsifiers are named in the ticket: a wall that publishes when nothing changed (a phone woken for
    /// nothing), and a publish that does not reach the device that asked — so the `to` on the frame is checked as
    /// carefully as the projection is.
    #[test]
    fn the_relay_carries_a_watch_and_publishes_only_what_changed() {
        let fake = Fake::start("watch");
        let wall = TempDir::new("watch-wall");
        let (devices, key) = fake.pair_phone(wall.path());
        let settings = fake.settings(&devices);
        let (seen, observed) = std::sync::mpsc::channel::<Report>();
        let relay_half = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            end.send(&format!(
                r#"{{"v":{PROTOCOL},"device":"{key}","watch":"wall"}}"#
            ));
            let ack = end.answer();
            let _ = seen.send(Report::Ack(ack));
            let published = end.answer();
            let _ = seen.send(Report::Published(published));
            // **A wall that changed nothing publishes nothing** (#221): a beat and a half of silence, which a wall
            // that published on every beat could not manage.
            let (quiet, what) = end.quiet_line(super::super::watch::BEAT * 3 / 2);
            let _ = seen.send(Report::Quiet(quiet, what));
        });

        // **The wall**, played by this thread: it answers every beat, because the loop asks between frames exactly as
        // `poll` answers it, and a test that stopped answering would be a wall that stopped drawing (`INV-05`).
        // **The wall's queue** (#222): the relay is handed an end of it, exactly as the socket's listener is.
        let queue = crate::control::sessions::Queue::new();
        let relay = Relay::spawn(settings, &queue).expect("the carrier thread starts");
        let mut state = crate::ui::State::default();
        let deadline = Instant::now() + PATIENCE;
        let (mut ack, mut published, mut quiet) = (None, None, None);
        let mut changed = false;
        while ack.is_none() || published.is_none() || quiet.is_none() {
            assert!(
                Instant::now() < deadline,
                "the relay never carried the watch through: ack={ack:?} published={published:?} quiet={quiet:?}"
            );
            if let Some(job) = queue.next() {
                // The wall is asked between frames, exactly as a poll answers it (INV-05): the beat's read and the
                // client's frames are the same door.
                let _ = crate::control::authority::serve(&mut state, job);
                // **The change, once the grant has been based on the wall as it was**: a phone that has just read is
                // watching a wall somebody else may move.
                if !changed {
                    changed = true;
                    let _ = crate::control::apply(&mut state, Intent::NewWorkspace);
                }
                continue;
            }
            while let Ok(report) = observed.try_recv() {
                match report {
                    Report::Ack(answer) => ack = Some(answer),
                    Report::Published(answer) => published = Some(answer),
                    Report::Quiet(within, what) => quiet = Some((within, what)),
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        relay_half.join().expect("the relay's half ends");
        // **The last job is answered before the carrier stops** (INV-05): a session blocked in `ask_the_wall`
        // outlives the wall by its own deadline, and a test that left one would spend five seconds saying nothing.
        while let Some(job) = queue.next() {
            let _ = crate::control::authority::serve(&mut state, job);
        }
        drop(relay);

        let ack = ack.expect("the grant");
        assert_eq!(
            ack["watch"],
            json!("wall"),
            "the grant says which stream is open (#221)"
        );
        assert_eq!(
            ack[frame::TO],
            json!("phone"),
            "and it is addressed to the device that asked, so the relay can route it"
        );
        assert!(
            ack.get("wall").is_none(),
            "the grant carries no projection: the client has just read one (#221)"
        );

        let published = published.expect("a publish");
        assert_eq!(
            published["watch"],
            json!("wall"),
            "a publish carries the watch it belongs to, so a client with a request in flight can tell the two \
             apart (#221)"
        );
        assert_eq!(published[frame::TO], json!("phone"));
        assert_eq!(
            published["wall"]["seq"],
            json!(1),
            "the change moved the projection's version by one, and the publish says which version it is (#221)"
        );
        assert_eq!(
            published["wall"]["workspaces"].as_array().map(Vec::len),
            Some(4),
            "and the body is the projection the change left, not the one the client read"
        );
        assert!(
            published["wall"]["focused"].is_number() || published["wall"]["focused"].is_null(),
            "a publish is a whole projection and not a patch: every field a read answers with is on it"
        );
        let (none_arrived, what) = quiet.expect("the quiet check");
        assert!(
            none_arrived,
            "the wall wrote {what:?} after publishing a change: a wall that publishes when nothing moved is the \
             ticket's falsifier — a phone woken for nothing (#221)"
        );
    }

    /// **A watch is a carrier's capability, not a verb of the plane** (#221): the socket answers one request with one
    /// answer — a `ctl` script must not be able to hold the wall's one connection open for ever — and refuses a watch
    /// **by name**, while the connection stays usable for the next request.
    #[test]
    fn the_unix_socket_refuses_a_watch_by_name_and_keeps_one_answer_per_request() {
        let (wall_end, client) =
            UnixStream::pair().expect("a socket pair, with no file to leave behind");
        let (tx, jobs) = mpsc::channel::<Job>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let serving = std::thread::spawn(move || {
            serve_session(
                super::super::carrier::Unix::new(wall_end).expect("a carrier over the socket"),
                &tx,
                &stopping,
            )
        });
        let mut writer = client.try_clone().expect("two handles on one socket");
        let mut reader = BufReader::new(client);

        writeln!(writer, r#"{{"v":{PROTOCOL},"watch":"wall"}}"#).expect("the ask goes");
        let mut line = String::new();
        reader.read_line(&mut line).expect("the refusal comes");
        assert_eq!(
            line.trim(),
            r#"{"kind":"rule","ok":false,"v":1,"why":"this carrier does not carry a watch"}"#,
            "a socket that was asked for a stream refuses by name, with the kind a client acts on (#221, INV-07)"
        );
        assert!(
            jobs.try_recv().is_err(),
            "the watch reached the wall: whether a watch can exist is the carrier's question (#221)"
        );

        // **One request in, one answer out, still**: the refusal is a line and not a closed connection.
        writeln!(writer, r#"{{"v":{PROTOCOL},"read":"wall"}}"#).expect("the next request goes");
        let job = jobs
            .recv_timeout(PATIENCE)
            .expect("the socket still answers requests after refusing a watch");
        assert_eq!(job.request(), Some(&Request::Wall));
        let (answer, _) = crate::control::authority::answer(
            &mut crate::ui::State::default(),
            &Request::Wall,
            None,
        );
        job.answer(answer);
        line.clear();
        reader.read_line(&mut line).expect("the read is answered");
        assert!(
            line.contains(r#""wall""#),
            "the connection did not answer a read after refusing a watch: {line}"
        );

        drop(reader);
        drop(writer);
        stop.store(true, Ordering::SeqCst);
        serving
            .join()
            .expect("the loop thread ends")
            .expect("a hung-up client is end of file, not an error");
    }

    /// **No line the carrier writes carries the wall's credential** (`INV-08`): not a `Debug`, not a refusal's words,
    /// not the refusal of a device. The hello is the one frame it is in, and that is the frame the wall sends to prove
    /// who it is.
    #[test]
    fn the_relay_never_prints_the_walls_credential() {
        let settings = Settings::of(
            Some("unix:/run/arcons-cc/relay.sock"),
            json!({ "refresh_token": CREDENTIAL }),
            PathBuf::from("/run/devices.toml"),
        )
        .expect("a URL this build dials");
        let written = [
            format!("{settings:?}"),
            format!("{}", Uncarried::NoRelay),
            format!("{}", Uncarried::Refused),
            format!("{}", Uncarried::NotAnswering),
            format!("{:?}", Uncarried::Refused),
        ];
        for text in written {
            assert!(
                !text.contains(CREDENTIAL),
                "INV-08: the wall's credential reached a log-shaped line: {text}"
            );
        }
    }

    /// **The wall's half of a conversation** (#262): take every job a carrier left on the wall's queue and settle it
    /// on this thread, exactly as `poll` does between frames, until `wanted` requests have been answered.
    ///
    /// Bounded, so a frame that never reached the wall is a failing test with a sentence rather than a gate that
    /// hangs — the rule every dialect in this module keeps.
    fn serve_the_wall(
        queue: &crate::control::sessions::Queue,
        state: &mut crate::ui::State,
        wanted: usize,
    ) -> Vec<Request> {
        let mut served: Vec<Request> = Vec::new();
        let deadline = Instant::now() + PATIENCE;
        while served.len() < wanted {
            if let Some(job) = queue.next() {
                let request = job.request().cloned();
                let _ = crate::control::authority::serve(state, job);
                if let Some(request) = request {
                    served.push(request);
                }
                continue;
            }
            assert!(
                Instant::now() < deadline,
                "the wall was handed {} of the {wanted} requests a relay conversation carries (#262)",
                served.len()
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        served
    }

    /// A wall with pairing open and nothing else: headless, so the code is a function of the frame clock (`INV-11`)
    /// and a claim writes no `devices.toml` — the file's own write is `ui::devices`'s test, not this one's.
    fn a_wall_with_a_code(directory: &TempDir) -> (crate::ui::State, String) {
        let mut state = crate::ui::State::default();
        state.headless = true;
        state.devices_path = directory.path().join("devices.toml");
        crate::control::apply(&mut state, Intent::PairBegin).expect("pairing opens");
        let digits = state.pairing().expect("a code on the wall").digits();
        (state, digits)
    }

    /// **One verb wide** (#262): a frame from a device this wall has no row for is carried only when its intent's verb
    /// is exactly `pair.claim`, and that answer goes back **on the connection it arrived on, with no `to`**.
    ///
    /// The falsifiers the ticket names: a second verb carried without a device key, a `to` on the claim's answer, and
    /// the key reaching the answer anywhere but `paired.key`. A `pane.focus` from the same unknown key is refused
    /// before the wall sees it, so the difference between the bypass and the rule is asserted rather than described.
    #[test]
    fn a_phone_with_no_row_may_claim_the_code_and_nothing_else() {
        let fake = Fake::start("claim");
        let wall = TempDir::new("claim-wall");
        let (mut state, digits) = a_wall_with_a_code(&wall);
        // **This phone has never paired**: there is no `devices.toml` row for any key, which is the whole point.
        let settings = fake.settings(&wall.path().join("devices.toml"));
        let queue = crate::control::sessions::Queue::new();
        let relay = Relay::spawn(settings, &queue).expect("the carrier thread starts");

        let stranger = "cd".repeat(32);
        let code = digits.clone();
        let (seen, observed) = mpsc::channel::<String>();
        let phone = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            // **The same unknown key, three other verbs**: a refusal on this connection each time, and the wall never
            // sees one. `pane.focus` is the plane's; `pair.begin` and `pair.forget` are pairing's own — a stranger who
            // could open a code could draw one on the wall, and one who could forget a device could revoke the phone
            // in the room, so the bypass is one verb wide *inside* pairing too.
            for intent in [
                r#"{"verb":"pane.focus","pane":2}"#,
                r#"{"verb":"pair.begin"}"#,
                r#"{"verb":"pair.forget","name":"phone"}"#,
            ] {
                end.request(&stranger, intent);
                let _ = seen.send(end.recv());
            }
            // **The claim, with no `device` at all**: there is no key yet, and that is what the frame is for.
            end.send(&format!(
                r#"{{"v":{PROTOCOL},"intent":{{"verb":"pair.claim","code":"{code}","name":"phone"}}}}"#
            ));
            let _ = seen.send(end.recv());
        });

        let served = serve_the_wall(&queue, &mut state, 1);
        let mut refused = Vec::new();
        for _ in 0..3 {
            let line = observed
                .recv_timeout(PATIENCE)
                .expect("a stranger's refusal");
            refused.push(serde_json::from_str::<Value>(&line).expect("a JSON answer"));
        }
        let answer = observed.recv_timeout(PATIENCE).expect("the claim's answer");
        phone.join().expect("the relay's half ends");
        drop(relay);

        // **The gate is the verb, not the key**: three other verbs were answered here and never became a `Job`.
        for refused in &refused {
            assert_eq!(
                refused["why"],
                json!(NOT_PAIRED),
                "a verb that is not `pair.claim` from a key no row holds is still `{NOT_PAIRED}` (#262): the \
                 bypass is one verb wide"
            );
            assert!(
                refused.get(frame::TO).is_none(),
                "a device the wall cannot name is not addressed"
            );
        }
        assert_eq!(
            served,
            vec![Request::Intent(Intent::PairClaim {
                code: digits.clone(),
                name: "phone".to_string(),
            })],
            "the wall was handed the claim as the wire's own request and nothing else (#196, #262)"
        );

        // **The claim's answer**: `paired`, once, with no `to`.
        let line = answer.clone();
        let answer: Value = serde_json::from_str(&answer).expect("a JSON answer");
        assert_eq!(
            answer["ok"],
            json!(true),
            "a claim that names the code on the wall pairs (#194)"
        );
        assert!(
            answer.get(frame::TO).is_none(),
            "the claim's answer carries a `to`: a device with no name has no address, and the frame goes back on \
             the connection it arrived on (#262)"
        );
        let paired = answer["paired"].as_object().expect("a `paired` answer");
        let mut fields: Vec<&str> = paired.keys().map(String::as_str).collect();
        fields.sort_unstable();
        assert_eq!(
            fields,
            vec!["key", "name"],
            "the pairing's answer carries the name and the key and no other field (INV-08, #262)"
        );
        assert_eq!(
            paired["name"],
            json!("phone"),
            "the answer names the device by the name it chose"
        );
        let key = paired["key"].as_str().expect("a key is a string");
        assert!(
            state.devices.has("phone"),
            "the wall knows the device by the row it wrote (#194)"
        );
        assert_eq!(
            state.devices.device[0].key,
            Key::from_hex(key)
                .expect("a key is 64 lowercase hex characters")
                .hash(),
            "the row is the SHA-256 of the key the answer carries, and nothing else (#194, INV-08)"
        );
        assert_eq!(
            line.matches(key).count(),
            1,
            "the key is on the claim's answer more than once: `paired.key` is the only field that may carry it \
             (INV-08)"
        );
    }

    /// **The budget is the wall's, and the relay path reaches it** (#262): five wrong codes burn the code, the fifth
    /// answer is the existing `no pairing is open`, and every answer has no `to` — a phone with no name gets one shape.
    ///
    /// The falsifiers: a code still claimable after five wrong guesses, and a refusal name added for the budget.
    #[test]
    fn five_wrong_codes_over_the_relay_burn_the_code() {
        let fake = Fake::start("claim-budget");
        let wall = TempDir::new("claim-budget-wall");
        let (mut state, digits) = a_wall_with_a_code(&wall);
        let settings = fake.settings(&wall.path().join("devices.toml"));
        let queue = crate::control::sessions::Queue::new();
        let relay = Relay::spawn(settings, &queue).expect("the carrier thread starts");

        let wrong = if digits == "000000" {
            "000001".to_string()
        } else {
            "000000".to_string()
        };
        let code = digits.clone();
        let typo = wrong.clone();
        let (seen, observed) = mpsc::channel::<String>();
        let phone = std::thread::spawn(move || {
            let mut end = fake.claim();
            end.welcome();
            for _ in 0..PAIR_ATTEMPTS {
                end.send(&format!(
                    r#"{{"v":{PROTOCOL},"intent":{{"verb":"pair.claim","code":"{typo}","name":"phone"}}}}"#
                ));
                let _ = seen.send(end.recv());
            }
            // **The right digits, after the budget**: not a second chance, and the same name.
            end.send(&format!(
                r#"{{"v":{PROTOCOL},"intent":{{"verb":"pair.claim","code":"{code}","name":"phone"}}}}"#
            ));
            let _ = seen.send(end.recv());
        });

        serve_the_wall(&queue, &mut state, PAIR_ATTEMPTS as usize + 1);
        phone.join().expect("the relay's half ends");
        drop(relay);

        let answers: Vec<Value> = (0..=PAIR_ATTEMPTS)
            .map(|_| {
                let line = observed.recv_timeout(PATIENCE).expect("an answer");
                serde_json::from_str(&line).expect("a JSON answer")
            })
            .collect();
        for (spent, answer) in answers.iter().take(PAIR_ATTEMPTS as usize - 1).enumerate() {
            assert_eq!(
                answer["why"],
                json!("that code is not the code"),
                "wrong code {} of {PAIR_ATTEMPTS} must be `that code is not the code`",
                spent + 1
            );
            assert!(
                answer.get(frame::TO).is_none(),
                "a claim's answer is not addressed: the wall has no name for the device yet (#262)"
            );
        }
        let fifth = &answers[PAIR_ATTEMPTS as usize - 1];
        assert_eq!(
            fifth["why"],
            json!("no pairing is open"),
            "the fifth wrong code must burn the pairing and answer the name that already exists — a code still \
             claimable after {PAIR_ATTEMPTS} wrong guesses is the ticket's falsifier (#262)"
        );
        assert_eq!(
            fifth["kind"],
            json!("rule"),
            "and its kind is the one `no pairing is open` already has: no new refusal for the budget (`INV-07`)"
        );
        assert!(
            fifth.get(frame::TO).is_none(),
            "the burnt code's answer is addressed to a device the wall cannot name (#262)"
        );
        let sixth = &answers[PAIR_ATTEMPTS as usize];
        assert_eq!(
            sixth["why"],
            json!("no pairing is open"),
            "the right digits after the budget are not a second chance (#262)"
        );
        assert!(
            state.pairing.is_none(),
            "the burnt code is gone from the wall, not merely refused"
        );
        assert!(
            !state.devices.has("phone"),
            "a burnt code paired the phone anyway"
        );
    }
}
