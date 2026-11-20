//! **How well a vessel matched** — six strengths, strongest first.

/// The strength of a vessel's match to one query.
///
/// `Ord` is derived from declaration order, which is strength order: `ExactHull` is the least and
/// `HullInside` the greatest. A caller sorts ascending to get the best matches first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    /// The query term *is* the hull number.
    ExactHull,
    /// The hull number begins with the term.
    HullPrefix,
    /// The name begins with the term.
    NamePrefix,
    /// A word in the name begins with the term — "dip" finds DIPONEGORO without pretending it is the
    /// start of "KRI DIPONEGORO".
    NameWord,
    /// The term occurs inside the name, but not at the start of it or of a word.
    NameInside,
    /// The term occurs inside the hull number, but not at its start.
    HullInside,
}
