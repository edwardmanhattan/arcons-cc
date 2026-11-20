//! **The picker's search** — local, token-based, and deterministic.
//!
//! There is no request per keystroke: the standing picture is read once and searched here, so results keep
//! appearing when the network has gone. Ranking is part of the search, not the view, because two places
//! deciding which hull is the better answer is two places to disagree.
//!
//! Every term must match somewhere, in any order, across the name and the hull number: `dip 365` finds
//! KRI DIPONEGORO 365. A vessel's score is the **strongest** match any one term earned, so an exact hull
//! number is not buried by another term that only occurs inside the name.

use super::fold::fold;
use super::rank::Rank;
use super::vessel::Vessel;

/// A query result: a borrowed vessel and how strongly it matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit<'a> {
    pub vessel: &'a Vessel,
    pub score: Rank,
}

/// A searchable copy of the standing picture.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Picker {
    vessels: Vec<Vessel>,
}

impl Picker {
    /// Keep the picture as Minos returned it. Every hull stays, including the ones with no position.
    pub fn new(vessels: Vec<Vessel>) -> Self {
        Self { vessels }
    }

    /// Match `query` against every hull, best first.
    ///
    /// Matching and ranking are both case- and diacritic-insensitive; see [`super::fold`] for the exact
    /// table. Terms are whitespace-separated and each must match the name or the hull number, in any order,
    /// so `365 dip` and `dip 365` are the same query.
    ///
    /// Ties are broken by folded name and then `id_unit`, so the same query against the same picture
    /// returns the same order twice — a list that reshuffles itself is a list the operator re-reads.
    ///
    /// An empty or whitespace-only query is not a search: every vessel is returned **in the order given**,
    /// scored at [`Rank::HullInside`] because no term earned anything. A punctuation-only query is a
    /// search, and a term nobody's name or number contains matches nothing.
    pub fn query(&self, query: &str) -> Vec<Hit<'_>> {
        let terms: Vec<String> = query.split_whitespace().map(fold).collect();
        if terms.is_empty() {
            return self
                .vessels
                .iter()
                .map(|vessel| Hit {
                    vessel,
                    score: Rank::HullInside,
                })
                .collect();
        }

        let mut hits: Vec<Hit<'_>> = self
            .vessels
            .iter()
            .filter_map(|vessel| best_rank(vessel, &terms).map(|score| Hit { vessel, score }))
            .collect();
        hits.sort_by_cached_key(|hit| (hit.score, fold(&hit.vessel.name), hit.vessel.id_unit));
        hits
    }
}

/// The strongest rank any one term earned, or `None` if a term matched nowhere.
fn best_rank(vessel: &Vessel, terms: &[String]) -> Option<Rank> {
    let name = fold(&vessel.name);
    let hull = fold(&vessel.hull_number);
    let words: Vec<&str> = name.split_whitespace().collect();

    let mut best: Option<Rank> = None;
    for term in terms {
        let rank = term_rank(term, &name, &hull, &words)?;
        best = Some(best.map_or(rank, |current| current.min(rank)));
    }
    best
}

fn term_rank(term: &str, name: &str, hull: &str, words: &[&str]) -> Option<Rank> {
    if !hull.is_empty() && hull == term {
        return Some(Rank::ExactHull);
    }
    if !hull.is_empty() && hull.starts_with(term) {
        return Some(Rank::HullPrefix);
    }
    if name.starts_with(term) {
        return Some(Rank::NamePrefix);
    }
    if words.iter().any(|word| word.starts_with(term)) {
        return Some(Rank::NameWord);
    }
    if name.contains(term) {
        return Some(Rank::NameInside);
    }
    if !hull.is_empty() && hull.contains(term) {
        return Some(Rank::HullInside);
    }
    None
}
