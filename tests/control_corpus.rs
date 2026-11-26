//! **The exchange corpus** (#224): the wall's answers, checked against the file a client in another language
//! replays.
//!
//! The corpus is **recorded**, not written. `fixtures/control/session.jsonl` is one request/answer pair per
//! line, and it comes out of the wall's own session loop over a `Loopback` carrier — the door the unix
//! socket uses (#195) — driven by the requests in `control::corpus`. `cargo run -- --record` rewrites it;
//! no one edits it by hand, because a corpus written from our structs proves only that we agree with
//! ourselves.
//!
//! These tests are the four ways that can rot:
//!
//! - the file is not what this wall records today (a stale corpus),
//! - the file replays to different lines (a protocol change nobody re-recorded),
//! - a verb `docs/control.md` documents has no exchange, or an exchange names a verb that is gone, and
//! - a refusal a client can provoke has no example, so the reader meets it in the field first.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use arcons_cc::control::{self, REFUSALS, RefusalKind, corpus};
use arcons_cc::verb::{Intent, vocabulary};
use serde_json::Value;
use serde_json::value::RawValue;

fn corpus_path() -> PathBuf {
    corpus::session_path(&corpus::directory())
}

/// The recorded file, read — with the sentence a missing corpus earns.
fn recorded() -> Vec<corpus::Exchange> {
    let path = corpus_path();
    let exchanges = corpus::read(&path).unwrap_or_else(|error| {
        panic!(
            "{error}\n  There is no corpus a client in another language can replay (#224). \
             `cargo run -- --record` writes it from the wall."
        )
    });
    assert!(
        !exchanges.is_empty(),
        "{} is empty: a corpus with no exchange replays nothing (#224)",
        path.display()
    );
    exchanges
}

/// The verb a request line names, when it names one at all. A line that is not JSON (the corpus's own
/// `not json` probe) names none.
fn verb(send: &RawValue) -> Option<String> {
    let value: Value = serde_json::from_str(&corpus::line_of(send)).ok()?;
    value
        .get("intent")?
        .get("verb")?
        .as_str()
        .map(str::to_string)
}

/// `send`'s line as JSON, for the questions a request line answers.
fn request(send: &RawValue) -> Option<Value> {
    serde_json::from_str(&corpus::line_of(send)).ok()
}

/// The refusal an answer line carries, when it is a refusal.
fn refused(expect: &RawValue) -> Option<String> {
    let value: Value = serde_json::from_str(expect.get()).ok()?;
    match value.get("ok").and_then(Value::as_bool) {
        Some(false) => value.get("why").and_then(Value::as_str).map(str::to_string),
        _ => None,
    }
}

/// The whole answer line as JSON, for reading its `kind` or its body.
fn answer(expect: &RawValue) -> Value {
    serde_json::from_str(expect.get()).unwrap_or_else(|error| {
        panic!("the corpus carries an answer that is not JSON: {error} (#224)")
    })
}

/// **The file is the wall's, not a human's.** A corpus that stopped matching what the wall answers — a
/// renamed field, a moved refusal, a new verb's JSON shape — is red here, with the one command that fixes
/// it.
#[test]
fn a_recorded_corpus_is_what_this_wall_records_today() {
    let expected = corpus::recorded_bytes()
        .unwrap_or_else(|error| panic!("this wall cannot record its own corpus: {error}"));
    let path = corpus_path();
    let found = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}\n  `cargo run -- --record` writes the corpus from the wall (#224)",
            path.display()
        )
    });
    assert_eq!(
        found,
        expected,
        "{} is not the corpus this wall records today (#224): a wall that answers different lines than \
         the file says is a client in another language replaying the wrong contract. Re-record it with \
         `cargo run -- --record` and read the diff",
        path.display()
    );
}

/// **Every pair is what this wall answers today**, replayed through the session loop over a `Loopback`
/// carrier and compared line for line. This is the ticket's proof, and the one a client's CI mirrors.
#[test]
fn every_recorded_exchange_is_what_the_wall_answers_today() {
    let path = corpus_path();
    let answered = corpus::replay(&path).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        answered.len(),
        recorded().len(),
        "the replay answered {} exchanges and the file holds {} — a pair went missing between reading \
         the corpus and serving it (#224)",
        answered.len(),
        recorded().len()
    );
    for (exchange, answer) in &answered {
        assert_eq!(
            answer,
            exchange.expect.get(),
            "{} answered {answer:?}, and the corpus expects {:?} (#224)",
            corpus::line_of(&exchange.send),
            exchange.expect.get()
        );
    }
}

/// **Every verb in `docs/control.md` has an exchange.** The document's verbs are `Intent::verb()` (a test in
/// `tests/doc_claims.rs` says the two lists are equal), so a verb the document promises and the corpus never
/// sends is a control a phone's author cannot see answered.
#[test]
fn every_verb_in_the_document_has_an_exchange() {
    let sent: BTreeSet<String> = recorded()
        .iter()
        .filter_map(|exchange| verb(&exchange.send))
        .collect();
    for intent in vocabulary() {
        let name = intent.verb();
        assert!(
            sent.contains(name),
            "docs/control.md documents `{name}` and the corpus never sends it (#224): a client's author \
             reads the pair or guesses the answer"
        );
    }
}

/// **An exchange names a verb the wall still answers to.** The one exception is the probe for `unknown
/// verb` itself — and it has to be the exception, or the corpus could not show what a retired verb earns.
#[test]
fn every_exchange_names_a_verb_the_wall_answers_to() {
    let known: BTreeSet<&'static str> = vocabulary().iter().map(Intent::verb).collect();
    for (number, exchange) in recorded().iter().enumerate() {
        let Some(verb) = verb(&exchange.send) else {
            continue;
        };
        if known.contains(verb.as_str()) {
            continue;
        }
        assert_eq!(
            refused(&exchange.expect).as_deref(),
            Some("unknown verb"),
            "line {} sends `{verb}`, which no intent answers, and the wall did not refuse it by name \
             (#224): a renamed or retired verb left a corpus line that replays against nothing",
            number + 1
        );
    }
}

/// **Refusals that no request the corpus can send will earn**, and why. Every other line of
/// [`REFUSALS`] must appear as an answer in the corpus — a client meets the wall's "no" in the file or in
/// the field, and the file is cheaper.
///
/// The four menu refusals are the same fact: a menu is opened by a **press** (#164), and there is no intent
/// that opens one, so a client with a socket cannot reach the states those messages describe. The two
/// `busy` messages are the transport's: they mean the session loop did not answer, which is not a request.
const NOT_PROVOKABLE: &[(&str, &str)] = &[
    (
        "no row has the ring",
        "a menu is opened by a press and no intent opens one (#164); the ring only exists inside it",
    ),
    (
        "no submenu is open",
        "a menu is opened by a press and no intent opens one (#164); a waterfall only exists inside it",
    ),
    (
        "the ring is on no row",
        "a ring is clamped onto a row of an open menu, and no intent opens a menu (#164)",
    ),
    (
        "the row opens nothing",
        "the ring is always a row of an open menu, and no intent opens a menu (#164)",
    ),
    (
        "the wall did not answer",
        "the session loop timed out — the UI thread stopping is not a request a client sends",
    ),
    (
        "the wall has stopped",
        "the wall went away — a transport failure is not a request a client sends",
    ),
    (
        "that device is already paired",
        "the only way to reach it is a second claim against a name the first claim added, and a successful \
         claim's answer carries the device key (`INV-08`): a corpus that recorded one would put a key in the \
         tree. The unit test drives it instead (#194)",
    ),
    (
        "the wall has no source of chance",
        "the operating system's entropy source failing is not something a request can cause (#194)",
    ),
    (
        "no relay is configured",
        "the wall has no relay to carry a line to — a configuration fact, not a request (#196)",
    ),
    (
        "the relay is not answering",
        "the outbound carrier's transport failed: nothing a socket client's request causes (#196)",
    ),
    (
        "the relay refused the wall",
        "the relay said no to the wall's own credential at the dial, before any request exists (#196)",
    ),
    (
        "that device is not paired",
        "the phone's key is checked by the relay carrier, and a socket client never sends one: the \
         carrier's own test drives this refusal on the wire (#196)",
    ),
];

/// **Every refusal a client can provoke is in the corpus**, and every one the corpus records carries the
/// kind `REFUSALS` gives it (`INV-07`): a client decides what to do from `kind`, and a corpus without one
/// would teach it to read English.
#[test]
fn every_refusal_a_client_can_provoke_is_in_the_corpus() {
    let exchanges = recorded();
    let mut covered: BTreeSet<String> = BTreeSet::new();
    for (number, exchange) in exchanges.iter().enumerate() {
        let whole = answer(&exchange.expect);
        let Some(why) = refused(&exchange.expect) else {
            assert_eq!(
                whole.get("ok").and_then(Value::as_bool),
                Some(true),
                "line {} answers neither `ok: true` nor a named refusal: {} (#224)",
                number + 1,
                exchange.expect.get()
            );
            continue;
        };
        let kind = control::refusal_kind(&why).unwrap_or_else(|| {
            panic!(
                "line {} records the refusal {why:?}, which `control::REFUSALS` does not name (#198): a \
                 client cannot decide what to do from it",
                number + 1
            )
        });
        assert_eq!(
            whole.get("kind").and_then(Value::as_str),
            Some(kind.word()),
            "line {} records {why:?} without its kind {:?} (#198): every refusal is a name and a \
             category",
            number + 1,
            kind.word()
        );
        covered.insert(why.clone());
    }

    let unprovokable: BTreeSet<&str> = NOT_PROVOKABLE.iter().map(|(why, _)| *why).collect();
    let mut missing = Vec::new();
    for (why, _) in REFUSALS {
        if covered.contains(*why) || unprovokable.contains(*why) {
            continue;
        }
        missing.push(*why);
    }
    assert!(
        missing.is_empty(),
        "these refusals a client can provoke have no exchange in the corpus (#224): {missing:?}. A \
         client's author meets them in the field first. Record one, or add it to NOT_PROVOKABLE with the \
         reason no request can earn it"
    );

    // And the list of exclusions stays honest: an exclusion for a message `REFUSALS` no longer names is a
    // reason nobody can act on.
    for (why, reason) in NOT_PROVOKABLE {
        assert!(
            REFUSALS.iter().any(|(named, _)| named == why),
            "NOT_PROVOKABLE excludes {why:?} ({reason}), which is not a refusal this wall makes (#224)"
        );
        assert!(
            !covered.contains(*why),
            "NOT_PROVOKABLE says {why:?} cannot be provoked, and the corpus provokes it (#224): delete \
             the exclusion"
        );
    }
}

/// **A read before a change and a read after it** (#181): a client holds a projection across changes, so
/// the corpus has to show the shape it reads and the shape it reads back.
#[test]
fn the_corpus_reads_the_wall_before_and_after_a_change() {
    let exchanges = recorded();
    let is_read = |exchange: &corpus::Exchange| {
        request(&exchange.send)
            .and_then(|request| {
                request
                    .get("read")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .as_deref()
            == Some("wall")
    };
    let change = exchanges
        .iter()
        .position(|exchange| verb(&exchange.send).is_some());
    let Some(change) = change else {
        panic!(
            "the corpus changes nothing: a corpus of reads teaches a client to draw a still wall (#224)"
        );
    };
    assert!(
        exchanges[..change].iter().any(is_read),
        "the corpus changes the wall before it ever reads it (#224): a client cannot know the projection \
         it started from"
    );
    assert!(
        exchanges[change + 1..].iter().any(is_read),
        "the corpus changes the wall and never reads it again (#224): a client cannot know the projection \
         the change landed on"
    );
}

/// **The version is refused rather than negotiated**, and the corpus shows it: one exchange sends a version
/// this wall does not speak and records the `version` kind, so a client's author sees what to do about it.
#[test]
fn a_version_the_wall_does_not_speak_is_recorded() {
    let exchanges = recorded();
    let probe = exchanges.iter().find(|exchange| {
        request(&exchange.send)
            .and_then(|request| request.get("v").and_then(Value::as_u64))
            .is_some_and(|version| version != u64::from(control::PROTOCOL))
    });
    let probe = probe.unwrap_or_else(|| {
        panic!(
            "the corpus has no version probe (#224): a client from another protocol must be shown the \
             refusal it earns, not left to guess that `v` is compared"
        )
    });
    assert_eq!(
        refused(&probe.expect).as_deref(),
        Some("unsupported protocol version"),
        "a request with another version must be refused by name (#224), not read as this protocol's"
    );
    assert_eq!(
        answer(&probe.expect).get("kind").and_then(Value::as_str),
        Some(RefusalKind::Version.word()),
        "the version refusal's kind is `version` (#198): the client's action is to stop and update"
    );
}
