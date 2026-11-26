//! **Pairing** (#194, `docs/mobile-remote.md` decision 3): a code on the wall, a key on the phone, a hash in
//! a file.
//!
//! Two people in the same room pair a phone: someone at the wall opens pairing, the wall draws six digits for
//! **two minutes**, and the phone sends those digits and a name. The wall writes a row to `devices.toml` — the
//! **SHA-256 of a fresh 32-byte key**, the name, when — and hands the key back through the carrier, once.
//!
//! Three rules this module keeps, each of them a test:
//!
//! - **The code is drawn, never returned.** `pair.begin`'s answer is a change, not the digits: a client that can
//!   read its own pairing code could pair itself, and the code's whole point is that it is a picture on a screen
//!   in the room (`INV-08`).
//! - **The code lives two minutes, on the frame clock**, and a claim spends it. A code that outlived its two
//!   minutes, or its own claim, is a second phone pairing with something read over a shoulder.
//! - **A code gives up after [`PAIR_ATTEMPTS`] wrong ones** (#262): the budget is the wall's, because the code lives
//!   here — six digits with two minutes and a person in the room is a better bound than a rate limiter somebody
//!   else's process might apply.
//! - **A refusal is a name, with a kind** (`INV-07`): `no pairing is open`, `that code is not the code`,
//!   `that device is already paired` — a client that cannot tell the first two apart retries the wrong one for
//!   ever, and one that cannot tell the third would re-pair a device it already has.
//!
//! The file is `config::devices`'s; the write is `ui::devices`'s, on a thread (`INV-05`). This module is the
//! rules in between.
//!
//! **The wall also draws a QR beside the digits** (#270): `arcons://pair?relay=…&code=…`, encoded once when the
//! code is drawn ([`Matrix`]) and drawn as squares by `ui::qr`. The payload is a function of two values — the
//! digits and the relay's **string** — and never of a connection, so a capture binds no socket (`INV-11`). The
//! code in the payload is not `pair.begin`'s answer (`INV-08`): the answer is still a change, and what carries
//! the digits is a picture in the room, which is exactly who a pairing code is for.

use crate::config::devices::{Device, Key};
use crate::control::{Changed, Refused};
use crate::ui::State;

/// How long a code is good for, on the frame clock: **two minutes**. Long enough to type six digits on a phone,
/// short enough that a code nobody paired with is not a door left open for a shift.
pub const PAIR_FOR: f64 = 120.0;

/// **What a pairing QR says** (#270): the scheme and the route, the relay, and the digits.
pub const PAIR_SCHEME: &str = "arcons://pair";

/// **How many wrong codes a code survives** (#262, decided on #260).
///
/// A person mistypes six digits on a phone in a room, so one guess is not a budget — five is. And five, not fifty:
/// 10⁶ codes with a five-guess budget per code, one open code at a time and two minutes each, means a guesser needs
/// roughly 200,000 codes-worth of two-minute windows — and every one of those windows is a fresh code drawn on a
/// screen in a room, by somebody pressing `pair.begin`. That is a better bound than a rate limiter on somebody else's
/// relay, so it lives where the code lives.
///
/// After the fifth wrong `code` the pairing is **gone** and the answer is the existing `no pairing is open`: no new
/// refusal name, no new kind (`INV-07`), and **no counter a client can read** — a client that could count the tries
/// would know when a code was nearly burnt. A correct claim still spends the code, and `pair.begin` replaces a burnt
/// one, which is the remedy the operator already had.
pub const PAIR_ATTEMPTS: u32 = 5;

/// **The code on the wall** (#194): six digits, and the frame after which they are not a code any more.
///
/// It lives on [`State::pairing`] rather than in [`crate::ui::Interaction`] on purpose: a code holds neither the
/// pointer nor the keyboard, and the wall keeps drawing the sea behind it while two people read it.
#[derive(Clone, Debug, PartialEq)]
pub struct Pairing {
    /// 0–999 999, drawn zero-padded and split as `PAIR 481 902`.
    pub code: u32,
    /// The frame clock at which it stops being a code. Exclusive: at `until` it is gone.
    pub until: f64,
    /// **How many wrong codes have been spent on it** (#262), against [`PAIR_ATTEMPTS`].
    ///
    /// Deliberately not in any answer, read or drawing: the budget is the wall's, and a client that could count the
    /// tries could tell how nearly a code was burnt — `docs/control.md` says so where it says what the frame is.
    attempts: u32,
    /// **The module grid the panel draws** (#270), encoded when the code was drawn — never re-encoded in the
    /// view loop (`INV-05`, and a frame draws squares rather than building a QR). `None` when no relay is
    /// configured, or for a payload no version can hold: then the digits are drawn alone, which is what a phone
    /// with no camera needs anyway.
    qr: Option<Matrix>,
}

impl Pairing {
    /// **What the wall draws**: `PAIR 481 902`. The one place the form is written, so the panel and a test cannot
    /// disagree about what an operator reads.
    pub fn words(&self) -> String {
        let code = self.digits();
        format!("PAIR {} {}", &code[..3], &code[3..])
    }

    /// The six digits, zero-padded: what a phone sends back as `code`.
    pub fn digits(&self) -> String {
        format!("{:06}", self.code)
    }

    /// Whether the code is still a code at `now`.
    pub fn is_live(&self, now: f64) -> bool {
        now < self.until
    }

    /// Whether `code` is the digits this wall is drawing.
    pub fn accepts(&self, code: &str) -> bool {
        code.trim() == self.digits()
    }

    /// **The matrix the panel draws beside the digits** (#270), or nothing.
    pub fn qr(&self) -> Option<&Matrix> {
        self.qr.as_ref()
    }
}

/// **The payload a phone reads off the wall** (#270): `arcons://pair?relay=<percent-encoded>&code=481902`.
///
/// One function, so the QR and the words cannot disagree: `digits` is [`Pairing::digits`]'s string — the six
/// digits zero-padded, exactly what the panel draws — and `relay` is the address as an operator wrote it.
///
/// **`None` when no relay is configured**, because a QR that carries no address is a picture that fails on the
/// phone: the digits are then drawn alone, which is the fallback a phone with no camera needs anyway.
///
/// The code is on the glass and not in an answer (`INV-08`): `pair.begin` still replies with a change, and this
/// payload is drawn — a phone in the room is exactly who a pairing code is for, so the picture it reads is the
/// one place the digits may travel.
///
/// The relay is **percent-encoded by hand** and not by a second crate: the set that matters in a query value is
/// `%`, `&`, `=`, `#`, `+` and the space, and every other byte outside the URI's unreserved set is escaped too.
/// `:` and `/` are left alone because a query value may hold them and an operator reading the code beside the
/// digits should still see an address. The phone decodes with Dart's `Uri.parse` (#271), so the escapes are the
/// standard `%XX`, uppercase hex — not a private spelling.
pub fn payload(digits: &str, relay: Option<&str>) -> Option<String> {
    let relay = relay.map(str::trim).filter(|relay| !relay.is_empty())?;
    Some(format!(
        "{PAIR_SCHEME}?relay={}&code={digits}",
        percent_encode(relay)
    ))
}

/// Percent-encode one value for a query, by hand. Unreserved bytes, `:` and `/` stay as they are; everything
/// else — `%`, `&`, `=`, `#`, `+`, a space, a byte that is not ASCII at all — becomes `%XX`.
fn percent_encode(value: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b':' | b'/' => {
                out.push(char::from(byte));
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// **The module grid of the pairing QR**, encoded once (#270).
///
/// Modules only: no renderer, no image, no PNG. `ui::qr` turns the grid into the wall's own squares, and the
/// matrix is cached on the [`Pairing`] because a frame that built a QR would be a frame doing an encoder's work
/// (`INV-05`).
///
/// **Error correction is Quartile** (`qrcodegen::QrCodeEcc::Quartile`): a wall is a screen in a room rather
/// than a print — a phone held at an angle, the glare of a lit room, and a camera that does not come close are
/// the errors here, and the level the standard calls for on a printed label is the floor, not the ceiling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// Modules per side, the quiet zone excluded.
    size: usize,
    /// `size * size` modules, row-major. `true` is dark.
    modules: Vec<bool>,
    /// What the encoder actually used, so "we asked for Q" can be checked against "it is Q".
    level: qrcodegen::QrCodeEcc,
}

impl Matrix {
    /// Encode `payload` at Quartile, or `None` when no version can hold it.
    ///
    /// **No panic on an encoder result**: a payload that does not fit is a state the panel draws without a QR,
    /// not a wall that falls over — the same rule the panel keeps for a wall with no relay.
    pub fn encode(payload: &str) -> Option<Matrix> {
        let code = qrcodegen::QrCode::encode_text(payload, qrcodegen::QrCodeEcc::Quartile).ok()?;
        let size = usize::try_from(code.size()).ok()?;
        let mut modules = Vec::with_capacity(size * size);
        for y in 0..code.size() {
            for x in 0..code.size() {
                modules.push(code.get_module(x, y));
            }
        }
        Some(Matrix {
            size,
            modules,
            level: code.error_correction_level(),
        })
    }

    /// Modules per side, the quiet zone excluded: 21 for version 1, four more for every version after it.
    pub fn size(&self) -> usize {
        self.size
    }

    /// The version the encoder chose: `(size - 17) / 4`, 1 to 40.
    pub fn version(&self) -> u8 {
        ((self.size.saturating_sub(17)) / 4) as u8
    }

    /// The error correction level as the standard names it (`L`, `M`, `Q`, `H`) — what the encoder used, not
    /// what this module asked for.
    pub fn correction(&self) -> char {
        match self.level {
            qrcodegen::QrCodeEcc::Low => 'L',
            qrcodegen::QrCodeEcc::Medium => 'M',
            qrcodegen::QrCodeEcc::Quartile => 'Q',
            qrcodegen::QrCodeEcc::High => 'H',
        }
    }

    /// Whether the module at `(x, y)` is dark. Out of bounds is light, which is the quiet zone's own answer.
    pub fn module(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }
}

/// **Open pairing** (`pair.begin`): put a fresh code on the wall for [`PAIR_FOR`], replacing any open one.
pub fn begin(state: &mut State) -> Result<Changed, Refused> {
    let now = state.frame_clock();
    let code = if state.headless {
        // **A capture's code is a function of its frame clock** (`INV-11`): a random one would make two renders
        // of the same scene differ, and a headless wall binds no socket for a phone to claim through. The digits
        // are a picture here, not a secret.
        captured_code(now)
    } else {
        fresh_code()?
    };
    let mut pairing = Pairing {
        code,
        until: now + PAIR_FOR,
        attempts: 0,
        qr: None,
    };
    // **The payload is the digits and the relay's string** (#270), and the grid is built here, once: the view
    // draws squares out of it on every frame and builds nothing (`INV-05`). A relay nobody configured means no
    // QR — a picture that carries no address fails on the phone, which is worse than the digits alone.
    pairing.qr = payload(&pairing.digits(), state.relay_url.as_deref())
        .and_then(|text| Matrix::encode(&text));
    state.pairing = Some(pairing);
    Ok(Changed(true))
}

/// **A phone claims the code** (`pair.claim`): the row goes into `devices.toml` and the key comes back for the
/// carrier to hand over, once.
///
/// The code is **spent** on success: it paired the phone in the room, and a code that outlived its claim would
/// pair whoever read it over a shoulder. The key is fresh here and never written — only its SHA-256 is.
///
/// **A wrong code is one of [`PAIR_ATTEMPTS`]** (#262): the fourth wrong guess leaves the code open, the fifth burns
/// it, and the answer to that fifth guess is `no pairing is open` — the same name an expired or spent code gets, so
/// there is no new refusal (`INV-07`) and nothing a client can read about how many tries are left.
pub fn claim(state: &mut State, code: &str, name: &str) -> Result<super::Paired, Refused> {
    let Some(pairing) = state.pairing().cloned() else {
        return Err(Refused("no pairing is open"));
    };
    if !pairing.accepts(code) {
        let spent = pairing.attempts + 1;
        if spent >= PAIR_ATTEMPTS {
            // **The code gives up** (#262): gone, and the answer is the name a phone already knows. The operator's
            // remedy is the one they had — `pair.begin` replaces the code.
            state.pairing = None;
            return Err(Refused("no pairing is open"));
        }
        if let Some(open) = state.pairing.as_mut() {
            open.attempts = spent;
        }
        return Err(Refused("that code is not the code"));
    }
    let name = name.trim();
    if name.is_empty() {
        return Err(Refused("\"name\" is a device name"));
    }
    if state.devices.has(name) {
        return Err(Refused("that device is already paired"));
    }
    let key = Key::fresh().map_err(|_| Refused("the wall has no source of chance"))?;
    state.devices.append(Device {
        name: name.to_string(),
        key: key.hash(),
        paired_at: paired_at(),
    });
    state.save_devices();
    // The code paired one phone. `docs/mobile-remote.md` leaves "two remotes at once" open; this does not decide
    // it, it spends the code — a second phone needs its own `pair.begin`.
    state.pairing = None;
    Ok(super::Paired {
        name: name.to_string(),
        key,
    })
}

/// **Revoke a device** (`pair.forget`): remove its row. `changed` is false for a name that was not there.
pub fn forget(state: &mut State, name: &str) -> Changed {
    if state.devices.forget(name) {
        state.save_devices();
        Changed(true)
    } else {
        Changed(false)
    }
}

/// Six fresh digits, from the operating system's source of chance.
fn fresh_code() -> Result<u32, Refused> {
    let mut bytes = [0_u8; 4];
    getrandom::fill(&mut bytes).map_err(|_| Refused("the wall has no source of chance"))?;
    Ok(u32::from_le_bytes(bytes) % 1_000_000)
}

/// The code a capture draws: a mix of its frame clock, so the same scene renders the same digits.
///
/// The odd constant is there so frame zero is not `000 000` — a capture of a code should look like a code.
fn captured_code(now: f64) -> u32 {
    let frames = (now * 1_000.0) as u64;
    let mixed = (frames.wrapping_mul(6_364_136_223_846_793_005) >> 32) as u32;
    (mixed ^ 481_902) % 1_000_000
}

/// When a device paired, as `devices.toml` writes it: RFC 3339 in UTC.
///
/// A record of a real moment, not a drawing decision, so it is the wall clock — and a capture writes no file, so
/// no golden can depend on it.
fn paired_at() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verb::Intent;

    /// A wall that pairs in memory only: a test may not write to the machine's `devices.toml` (`INV-11`).
    fn capture() -> State {
        let mut state = State::default();
        state.headless = true;
        state
    }

    /// A wall with a code drawn on it at `logical`, and the digits a phone would type.
    fn opened() -> (State, String) {
        let mut state = capture();
        state.apply(Intent::PairBegin).expect("pairing opens");
        let digits = state.pairing().expect("a code on the wall").digits();
        (state, digits)
    }

    /// **The right code pairs**: the row is the name, a hash and when; the key comes back once; the code is spent.
    #[test]
    fn the_right_code_pairs_and_the_file_gets_a_hash() {
        let (mut state, digits) = opened();
        let paired = super::claim(&mut state, &digits, "  phone  ").expect("the code pairs");
        assert_eq!(
            paired.name, "phone",
            "a name is trimmed, and stored trimmed"
        );
        let key = paired.key.hex();
        assert!(
            state.devices.has("phone"),
            "the name is in devices.toml's model"
        );
        let row = &state.devices.device[0];
        assert_eq!(
            row.key,
            paired.key.hash(),
            "the row is the SHA-256 of the key, and the client got the key (INV-08)"
        );
        assert_ne!(
            row.key, key,
            "a devices.toml that holds the key is a file of keys, not a list of names (INV-08)"
        );
        assert!(!row.paired_at.is_empty(), "the row says when it paired");
        assert!(
            state.pairing().is_none(),
            "the code paired one phone and is spent: a second claim is `no pairing is open`"
        );
        assert_eq!(
            super::claim(&mut state, &digits, "tablet"),
            Err(Refused("no pairing is open")),
            "a spent code is not a second door"
        );
    }

    /// **The fifth wrong code burns the code, and the sixth is `no pairing is open`** (#262, `INV-07`).
    ///
    /// The falsifiers the ticket names: a code still claimable after five wrong guesses, and a refusal name added for
    /// the budget. The fourth guess leaves the code alone, the fifth takes it, and the right digits afterwards are not
    /// a second chance — they meet the name a spent or expired code already gets, so nothing on the wire says how many
    /// tries are left.
    #[test]
    fn five_wrong_codes_burn_the_code_and_the_sixth_is_no_pairing_is_open() {
        let (mut state, digits) = opened();
        let wrong = if digits == "000000" {
            "000001"
        } else {
            "000000"
        };
        // **One short of the budget**: still the code's own refusal, still open.
        for spent in 1..PAIR_ATTEMPTS {
            assert_eq!(
                super::claim(&mut state, wrong, "phone"),
                Err(Refused("that code is not the code")),
                "wrong code {spent} of {PAIR_ATTEMPTS} is `that code is not the code`"
            );
            assert!(
                state.pairing().is_some(),
                "wrong code {spent} of {PAIR_ATTEMPTS} burnt a code it should only have spent a try on"
            );
        }
        // **The fifth wrong code is the last**: the pairing is gone, and the answer is the name that already exists.
        assert_eq!(
            super::claim(&mut state, wrong, "phone"),
            Err(Refused("no pairing is open")),
            "the fifth wrong code must burn the code and answer `no pairing is open` — a code still claimable \
             after {PAIR_ATTEMPTS} wrong guesses is the ticket's falsifier (#262)"
        );
        assert!(
            state.pairing.is_none(),
            "the burnt code is gone from the wall, not merely refused"
        );
        // **And the right digits are not a second chance** — the same name, not `that code is not the code`.
        assert_eq!(
            super::claim(&mut state, &digits, "phone"),
            Err(Refused("no pairing is open")),
            "a burnt code answered by the right digits is still `no pairing is open` (#262)"
        );
        assert!(
            !state.devices.has("phone"),
            "a burnt code paired the phone anyway"
        );
        // **The remedy the operator already had**: `pair.begin` replaces the burnt code.
        state.apply(Intent::PairBegin).expect("pairing opens again");
        let fresh = state.pairing().expect("a fresh code").digits();
        assert!(
            super::claim(&mut state, &fresh, "phone").is_ok(),
            "a fresh `pair.begin` after a burnt code must pair: the budget is per code, not per wall"
        );
    }

    /// **A wrong code is refused by name, and by the right name** — not `no pairing is open`, which is what a
    /// client would retry for ever.
    #[test]
    fn a_wrong_code_is_refused_by_name_and_leaves_the_pairing_open() {
        let (mut state, digits) = opened();
        let wrong = if digits == "000000" {
            "000001"
        } else {
            "000000"
        };
        assert_eq!(
            super::claim(&mut state, wrong, "phone"),
            Err(Refused("that code is not the code")),
            "wrong digits are `that code is not the code`, never `no pairing is open`"
        );
        assert!(
            state.pairing().is_some(),
            "a wrong code must not burn the right one"
        );
        assert!(
            super::claim(&mut state, &digits, "phone").is_ok(),
            "and the right one still pairs"
        );
    }

    /// **No pairing is open** before `pair.begin`, and after its **two minutes** — the falsifier the ticket
    /// names: a code that survives its two minutes.
    #[test]
    fn a_code_lives_two_minutes_and_not_a_frame_longer() {
        let mut state = capture();
        assert_eq!(
            super::claim(&mut state, "000000", "phone"),
            Err(Refused("no pairing is open")),
            "nothing begun is `no pairing is open`"
        );
        let (mut state, digits) = opened();
        let until = state.pairing().expect("a code").until;
        assert_eq!(until, PAIR_FOR, "two minutes, on the frame clock");

        state.set_frame_clock(until - 0.001);
        assert!(state.pairing().is_some(), "still a code a frame before");
        state.set_frame_clock(until);
        assert!(
            state.pairing().is_none(),
            "a code that survives its two minutes is the ticket's falsifier"
        );
        assert_eq!(
            super::claim(&mut state, &digits, "phone"),
            Err(Refused("no pairing is open")),
            "an expired code is `no pairing is open`, not `that code is not the code`"
        );
    }

    /// **A name is paired once** — `that device is already paired`, so a second phone cannot take the first one's
    /// name (which is how `pair.forget` and a cursor's `origin` name it).
    #[test]
    fn a_name_that_is_taken_is_refused_by_name() {
        let (mut state, digits) = opened();
        super::claim(&mut state, &digits, "phone").expect("the first pairing");
        state.apply(Intent::PairBegin).expect("a fresh code");
        let next = state.pairing().expect("a code").digits();
        assert_eq!(
            super::claim(&mut state, &next, "phone"),
            Err(Refused("that device is already paired"))
        );
        assert_eq!(
            state.devices.names(),
            vec!["phone"],
            "the taken name did not get a second row"
        );
        assert!(
            super::claim(&mut state, &next, "tablet").is_ok(),
            "the same code pairs a device with a name of its own"
        );
    }

    /// **Forgetting is deleting the row** — by name, and **only** the row it names (#194, #240): a `Forget` on the
    /// wall is one phone's row pressed beside its neighbour's, so a revocation that took the list would revoke the
    /// phone the operator was keeping.
    #[test]
    fn forgetting_a_device_removes_its_row() {
        let (mut state, digits) = opened();
        super::claim(&mut state, &digits, "phone").expect("the first pairing");
        // A second phone, with its own code: the row that must survive the press on the first.
        state.apply(Intent::PairBegin).expect("a fresh code");
        let next = state.pairing().expect("a code").digits();
        super::claim(&mut state, &next, "tablet").expect("the second pairing");
        assert_eq!(state.devices.names(), vec!["phone", "tablet"]);

        assert_eq!(super::forget(&mut state, "phone"), Changed(true));
        assert!(
            !state.devices.has("phone"),
            "a forgotten device is a row that is gone"
        );
        assert_eq!(
            state.devices.names(),
            vec!["tablet"],
            "and its neighbour is not: a revocation names one device (#240)"
        );
        assert_eq!(
            super::forget(&mut state, "phone"),
            Changed(false),
            "forgetting nothing is a no-op, not a refusal"
        );
        assert_eq!(
            state.devices.names(),
            vec!["tablet"],
            "and forgetting it twice removes nothing else"
        );
    }

    /// **The key never prints** (`INV-08`): the value a claim returns carries it, and formatting that value — as a
    /// log line that formatted an answer would — shows a redaction, not the key.
    #[test]
    fn the_key_never_prints() {
        let (mut state, digits) = opened();
        let paired = super::claim(&mut state, &digits, "phone").expect("the code pairs");
        let hex = paired.key.hex();
        for line in [
            format!("{paired:?}"),
            format!(
                "{:?}",
                super::super::wire::to_wire(&Intent::PairClaim {
                    code: digits.clone(),
                    name: "phone".to_string(),
                })
            ),
            state.devices.device[0].key.clone(),
        ] {
            assert!(
                !line.contains(&hex),
                "INV-08: a key is in a line that should not have it: {line}"
            );
        }
        assert!(
            format!("{paired:?}").contains("<redacted>"),
            "the key redacts itself in Debug, so a whole answer may be logged"
        );
    }

    /// **A capture never pairs and never writes** (`INV-11`): the wall a scene builds gets its code and its row,
    /// and the machine's `devices.toml` is untouched.
    #[test]
    fn a_capture_pairs_in_memory_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("arcons-pair-capture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("devices.toml");
        let mut state = crate::scene::state_for("panes");
        // `setup` marks a capture headless before its script runs; `state_for` builds the wall a scene's setup
        // produces, so the flag is the scene's to carry.
        state.headless = true;
        state.devices_path = path.clone();
        state
            .apply(Intent::PairBegin)
            .expect("a capture may draw a code");
        assert!(state.pairing().is_some());
        // The same code twice: a capture's digits are a function of its frame clock, not a lottery (`INV-11`).
        let mut again = crate::scene::state_for("panes");
        again.headless = true;
        again.devices_path = path.clone();
        again
            .apply(Intent::PairBegin)
            .expect("a second capture draws the same code");
        assert_eq!(
            state.pairing().map(|pairing| pairing.code),
            again.pairing().map(|pairing| pairing.code),
            "INV-11: same scene, same code"
        );
        let digits = state.pairing().expect("a code").digits();
        super::claim(&mut state, &digits, "phone").expect("a capture may claim");
        assert!(state.devices.has("phone"), "the model took the row");
        assert!(
            !path.exists(),
            "INV-11: a capture writes no devices.toml, the same rule layout.toml has"
        );
    }

    /// **The code is never in the answer** (`INV-08`): `pair.begin` changes the wall, and what a client gets back
    /// is whether it changed — the digits are on the screen in the room.
    #[test]
    fn the_code_is_drawn_and_never_returned() {
        let mut state = capture();
        let changed = state.apply(Intent::PairBegin).expect("pairing opens");
        assert!(changed.changed(), "a code appeared on the wall");
        let digits = state.pairing().expect("a code").digits();
        let line = crate::control::wire::reply(crate::control::wire::Answer::Changed(Ok(
            crate::control::Changed(changed.changed()),
        )));
        assert!(
            !line.contains(&digits),
            "INV-08: pair.begin's answer carries the code: {line}"
        );
        // And the panel's words are the digits, in the ticket's form.
        assert_eq!(
            state.pairing().expect("a code").words(),
            format!("PAIR {} {}", &digits[..3], &digits[3..])
        );
    }

    /// **The payload is the digits and the relay** (#270), exactly: one function, so the QR beside the words
    /// cannot carry a different code from the one the panel draws.
    ///
    /// The falsifiers the ticket names: a payload that does not name the digits on the glass, and a QR drawn with
    /// no relay configured — a picture that carries no address fails on the phone, so the digits are drawn alone.
    #[test]
    fn the_payload_is_the_drawn_digits_and_the_relays_address_or_nothing() {
        let mut state = capture();
        state.relay_url = Some("tcp://relay.arcons.test:7030".to_string());
        state.apply(Intent::PairBegin).expect("pairing opens");
        let pairing = state.pairing().expect("a code on the wall");
        let digits = pairing.digits();
        assert_eq!(digits.len(), 6, "the code is six digits, zero-padded");
        assert_eq!(
            payload(&digits, state.relay_url.as_deref()).as_deref(),
            Some("arcons://pair?relay=tcp://relay.arcons.test:7030&code=481902"),
            "the payload is `arcons://pair?relay=<relay>&code=<the six digits the panel draws>`"
        );
        assert!(
            pairing.qr().is_some(),
            "a wall with a relay draws a matrix for the code"
        );
        // **No relay, no QR** (#270): the digits are the whole picture, which is what a phone with no camera
        // reads anyway.
        let mut bare = capture();
        bare.apply(Intent::PairBegin)
            .expect("pairing opens on a wall with no relay");
        let bare = bare.pairing().expect("a code on the wall");
        assert_eq!(
            payload(&bare.digits(), None),
            None,
            "a QR carrying no address is a picture that fails on the phone: the payload is `None` and the panel \
             draws the digits alone"
        );
        assert!(
            bare.qr().is_none(),
            "a wall with no relay drew a matrix: the panel would draw a QR nobody can use"
        );
        assert!(
            bare.words().starts_with("PAIR "),
            "the digits are still drawn without a relay — they are the fallback, not a second surface"
        );
        assert_eq!(
            payload(&bare.digits(), Some("   ")),
            None,
            "a blank relay is a relay nobody meant to point at"
        );
    }

    /// **A payload no version can hold is a code without a QR, not a wall that falls over** (#270): the digits
    /// are still drawn, which is what a phone with no camera needs and what an operator watches while they fix
    /// the relay's address. The falsifier is an `unwrap` on the encoder's result — a panic on the wall.
    #[test]
    fn an_unencodable_payload_draws_the_digits_without_a_qr() {
        let mut state = capture();
        state.relay_url = Some(format!("tcp://{}", "a".repeat(4_000)));
        state
            .apply(Intent::PairBegin)
            .expect("the code still opens");
        let pairing = state.pairing().expect("a code on the wall");
        assert!(
            pairing.qr().is_none(),
            "a payload past every version of the standard was drawn as a QR: `Matrix::encode` said no and the \
             panel believed it — the digits are what is left"
        );
        assert!(
            pairing.words().starts_with("PAIR "),
            "an unencodable payload took the digits with it"
        );
        assert!(
            payload(&pairing.digits(), state.relay_url.as_deref()).is_some(),
            "the payload still has a form, it is the encoder that cannot hold it"
        );
    }

    /// **The encoding is standard, not clever** (#270): a phone decodes the value with Dart's `Uri.parse`, so
    /// `tcp://host:port` and `tls://host:port` round-trip, and the bytes a query value must escape — `%`, `&`,
    /// `=`, `#`, `+`, space — are escaped.
    #[test]
    fn a_relay_round_trips_through_the_percent_encoding() {
        for relay in [
            "tcp://relay.example.test:7030",
            "tls://relay.example.test:7030",
            "wss://relay.example.test/arcons",
            "unix:/run/arcons relay/a&b=c+d#e%f",
        ] {
            let text = payload("481902", Some(relay)).expect("a configured relay makes a payload");
            let (decoded, code) = decode(&text);
            assert_eq!(
                decoded, relay,
                "`{relay}` did not round-trip: the payload is `{text}`, and a phone decodes it with the \
                 standard's percent-encoding"
            );
            assert_eq!(code, "481902", "the code is the digits, unescaped");
        }
        // The bytes that matter, one at a time: a value carrying each is escaped, and a `&` or an `=` inside a
        // relay cannot end the value it is in.
        let text = payload("000001", Some("a b&c=d#e+f%g")).expect("a payload");
        assert_eq!(
            text,
            "arcons://pair?relay=a%20b%26c%3Dd%23e%2Bf%25g&code=000001"
        );
        assert_eq!(
            decode(&text),
            ("a b&c=d#e+f%g".to_string(), "000001".to_string())
        );
    }

    /// **The encoder's version and level for a realistic payload** (#270), and **a recorded digest of the grid**:
    /// an encoder bump is a red test, not a picture nobody looked at.
    ///
    /// The digest is SHA-256 over the modules row by row, one byte per module (`1` dark, `0` light), each row
    /// followed by `\n` — the grid as the number it is, not as the renderer paints it.
    #[test]
    fn a_realistic_payload_is_encoded_at_quartile_with_a_recorded_grid() {
        let text = payload("481902", Some(crate::scene::PAIRING_RELAY)).expect("a payload");
        let matrix = Matrix::encode(&text).expect("a realistic payload is encodable");
        assert_eq!(
            matrix.correction(),
            'Q',
            "the pairing QR is encoded at Quartile: a wall is a screen in a room, not a print"
        );
        assert_eq!(
            matrix.version(),
            5,
            "the encoder chose version {} for `{text}` and {} modules",
            matrix.version(),
            matrix.size()
        );
        assert_eq!(
            digest(&matrix),
            "fcc64d758a294bcc7f83a6734ed2a96aaaba8d69610648c41db8c24419fdfcbb",
            "the module grid changed: an encoder bump is a red test here rather than a picture nobody looked at \
             (#270). If the change is deliberate, record the new digest and say so in the PR"
        );
    }

    /// The payload as a phone reads it: the scheme, then `relay` and `code` out of the query, unescaped. A
    /// **test's** decoder — this repository has no business shipping a second one.
    fn decode(text: &str) -> (String, String) {
        let query = text
            .strip_prefix(PAIR_SCHEME)
            .and_then(|rest| rest.strip_prefix('?'))
            .unwrap_or_else(|| panic!("`{text}` is not a pairing payload"));
        let (mut relay, mut code) = (None, None);
        for field in query.split('&') {
            let (name, value) = field
                .split_once('=')
                .unwrap_or_else(|| panic!("`{field}` is not a field"));
            match name {
                "relay" => relay = Some(unescape(value)),
                "code" => code = Some(unescape(value)),
                other => panic!("`{other}` is not a field of a pairing payload"),
            }
        }
        (
            relay.expect("the payload names its relay"),
            code.expect("the payload names its code"),
        )
    }

    /// The standard's `%XX`, decoded: what Dart's `Uri.parse` does to the value.
    fn unescape(value: &str) -> String {
        let bytes = value.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'%' {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).expect("two hex digits");
                out.push(u8::from_str_radix(hex, 16).expect("two hex digits"));
                at += 3;
            } else {
                out.push(bytes[at]);
                at += 1;
            }
        }
        String::from_utf8(out).expect("a relay is UTF-8")
    }

    /// The matrix as a digest: one byte per module, row-major, and `\n` at the end of each row.
    fn digest(matrix: &Matrix) -> String {
        use sha2::{Digest as _, Sha256};

        let mut hasher = Sha256::new();
        for y in 0..matrix.size() {
            for x in 0..matrix.size() {
                hasher.update([u8::from(matrix.module(x, y))]);
            }
            hasher.update(b"\n");
        }
        format!("{:x}", hasher.finalize())
    }
}
