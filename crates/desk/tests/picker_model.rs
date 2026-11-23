//! The picker's search, without a window: every hull, ranked, deterministically.
//!
//! These are ADR-0009's decision 4 as claims. The tray and the list render the result; they do not decide
//! which hull the operator meant.

use desk::picker::{LastKnown, Picker, Rank, Vessel};

const NOW: i64 = 1_700_000_000_000;

/// A hull, reported or never-reported. `age: None` is silence, not a position at zero.
fn vessel(id_unit: u64, name: &str, hull_number: &str, age: Option<f64>) -> Vessel {
    Vessel {
        id_unit,
        name: name.to_string(),
        hull_number: hull_number.to_string(),
        last: age.map(|age_seconds| LastKnown {
            recorded_at_ms: NOW - (age_seconds * 1_000.0) as i64,
            age_seconds,
            backfilled: age_seconds > 60.0,
            latitude: -6.2,
            longitude: 106.8,
            speed_kn: None,
            course_deg: None,
            accuracy_m: None,
        }),
    }
}

fn ids(picker: &Picker, query: &str) -> Vec<u64> {
    picker
        .query(query)
        .iter()
        .map(|hit| hit.vessel.id_unit)
        .collect()
}

#[test]
fn an_exact_hull_number_outranks_a_name_match() {
    let picker = Picker::new(vec![
        vessel(1, "SOMETHING ELSE", "365", Some(10.0)),
        vessel(2, "365 DIPONEGORO", "999", Some(10.0)),
    ]);

    let hits = picker.query("365");
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].vessel.id_unit, 1);
    assert_eq!(hits[0].score, Rank::ExactHull);
    assert_eq!(hits[1].score, Rank::NamePrefix);
    assert!(
        hits[0].score < hits[1].score,
        "the hull number the operator typed is a better handle than a name that starts with it"
    );
}

#[test]
fn dip_365_matches_across_two_fields_in_either_order() {
    let picker = Picker::new(vec![
        vessel(1, "KRI DIPONEGORO", "365", Some(10.0)),
        vessel(2, "KRI BUNG TOMO", "999", Some(10.0)),
    ]);

    assert_eq!(ids(&picker, "dip 365"), vec![1]);
    assert_eq!(
        ids(&picker, "365 dip"),
        vec![1],
        "tokens are unordered: the operator may type the hull first"
    );
    assert!(
        ids(&picker, "dip 999").is_empty(),
        "every term must match the same hull; two hulls are not a hit"
    );
}

#[test]
fn a_never_reported_hull_is_listed_without_a_position_and_is_not_stale() {
    let picker = Picker::new(vec![
        vessel(1, "KRI SILENT", "001", None),
        vessel(2, "KRI LATE", "002", Some(7_200.0)),
    ]);

    let hits = picker.query("");
    assert_eq!(hits.len(), 2, "silence stays visible in the list");
    let silent = hits
        .iter()
        .find(|hit| hit.vessel.id_unit == 1)
        .expect("the never-reported hull is listed");
    assert!(
        silent.vessel.last.is_none(),
        "never reported is no position, not a position of zero"
    );
    assert!(
        !silent.vessel.is_stale(3_600.0),
        "silent is not stale: there is no age to cross the line"
    );
    assert!(
        !silent.vessel.is_stale(0.0),
        "even a zero window cannot make silence stale"
    );

    let late = picker
        .query("")
        .into_iter()
        .find(|hit| hit.vessel.id_unit == 2)
        .expect("the reported hull is listed");
    assert!(late.vessel.is_stale(3_600.0));
    assert!(
        !late.vessel.is_stale(7_200.0),
        "exactly at the line is not stale"
    );
}

#[test]
fn a_diacritic_in_the_name_is_found_by_the_unaccented_query() {
    let picker = Picker::new(vec![
        vessel(1, "KRI NÜRNBERG ÉLODIE", "331", Some(10.0)),
        vessel(2, "SÃO GONÇALO", "440", Some(10.0)),
    ]);

    assert_eq!(ids(&picker, "nurnberg elodie"), vec![1]);
    assert_eq!(
        ids(&picker, "sao goncalo"),
        vec![2],
        "ç and ã are in the desk's table, not in Unicode normalisation"
    );
}

#[test]
fn ranks_are_ordered_as_documented() {
    assert!(Rank::ExactHull < Rank::HullPrefix);
    assert!(Rank::HullPrefix < Rank::NamePrefix);
    assert!(Rank::NamePrefix < Rank::NameWord);
    assert!(Rank::NameWord < Rank::NameInside);
    assert!(Rank::NameInside < Rank::HullInside);

    let picker = Picker::new(vec![
        vessel(1, "ZULU", "dip", Some(10.0)),
        vessel(2, "ZULU TWO", "dip 365", Some(10.0)),
        vessel(3, "DIPONEGORO", "900", Some(10.0)),
        vessel(4, "KRI DIPONEGORO", "901", Some(10.0)),
        vessel(5, "KRI XDIPX", "902", Some(10.0)),
        vessel(6, "ZULU SIX", "39dip5", Some(10.0)),
    ]);

    let hits = picker.query("dip");
    assert_eq!(
        hits.iter()
            .map(|hit| hit.vessel.id_unit)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6],
        "one hull per rank, strongest first"
    );
    assert_eq!(
        hits.iter().map(|hit| hit.score).collect::<Vec<_>>(),
        vec![
            Rank::ExactHull,
            Rank::HullPrefix,
            Rank::NamePrefix,
            Rank::NameWord,
            Rank::NameInside,
            Rank::HullInside,
        ]
    );
}

#[test]
fn the_same_query_twice_returns_the_same_order() {
    let picker = Picker::new(vec![
        vessel(12, "KRI DIPONEGORO", "902", Some(10.0)),
        vessel(11, "KRI DIPONEGORO", "901", Some(10.0)),
        vessel(20, "KRI BUNG TOMO", "999", Some(10.0)),
    ]);

    let first = ids(&picker, "dip");
    let second = ids(&picker, "dip");
    assert_eq!(
        first, second,
        "a list that reshuffles is re-read every time"
    );
    assert_eq!(
        first,
        vec![11, 12],
        "ties on name fall back to id_unit, not to insertion luck"
    );
}

#[test]
fn an_empty_query_keeps_the_input_order() {
    let picker = Picker::new(vec![
        vessel(3, "ZULU", "003", Some(10.0)),
        vessel(1, "ALPHA", "001", Some(10.0)),
        vessel(2, "MIKE", "002", None),
    ]);

    assert_eq!(ids(&picker, ""), vec![3, 1, 2]);
    assert_eq!(
        ids(&picker, "   \t  "),
        vec![3, 1, 2],
        "whitespace is no terms, and no terms is not a filter"
    );
    assert!(
        picker
            .query("")
            .iter()
            .all(|hit| hit.score == Rank::HullInside),
        "an empty query earned no rank; the weakest score is not evidence of a match"
    );
}

#[test]
fn a_punctuation_only_query_returns_nothing_rather_than_everything() {
    let picker = Picker::new(vec![
        vessel(1, "KRI DIPONEGORO", "365", Some(10.0)),
        vessel(2, "KRI BUNG TOMO", "999", None),
    ]);

    for query in ["!!!", "—", "-- ?? ,,"] {
        assert!(
            picker.query(query).is_empty(),
            "{query:?} is a term nobody's name or number contains, not an empty filter"
        );
    }
}

#[test]
fn a_query_longer_than_every_name_returns_nothing_and_an_empty_picker_is_safe() {
    let picker = Picker::new(vec![vessel(1, "KRI DIPONEGORO", "365", Some(10.0))]);
    let long = "diponegoro".repeat(200);
    assert!(picker.query(&long).is_empty());

    let empty = Picker::new(Vec::new());
    assert!(empty.query("dip").is_empty());
    assert!(empty.query("").is_empty());
}
