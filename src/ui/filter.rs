//! What a pane hides (#112): the filter's value, its panel rows, and the one place a press changes it.
//!
//! **The panel is a projection of the backend's vocabulary, not a list written here.** The tactical
//! objects' domains and categories arrive from `GET /tactical-objects/facets` with their counts and their
//! `sort_order`; the installation types arrive from `GET /installation-types`. No domain, category or type
//! name appears in this crate, so a backend that adds one shows it — in the panel **and on the wall** —
//! without a release.
//!
//! **Exclusions, not inclusions.** Every set is of things to *hide*, so the default (`Filter::default`,
//! every field empty) draws exactly what the wall drew before the filter existed. A list of what to show
//! would have to be complete on the day it was written and would silently drop whatever the backend added
//! the next morning.
//!
//! **Counts, honestly.** A domain's and a category's count is the **server's**: a client holding one page
//! cannot count the whole catalogue, and one that counted only what it had fetched would draw a total that
//! changed as the operator scrolled. An installation type's count is the **cache's**: `GET /installations`
//! is unpaginated, so the wall holds the complete set after its init read. A count the client invents is a
//! filter that lies about what is on the wall.
//!
//! **Precedence, stated once:** a symbol is hidden when its layer is off, **or** its domain is hidden,
//! **or** its category is hidden. Turning a domain off hides everything under it; a category toggled
//! inside a hidden domain cannot bring one symbol back, which is what keeps the rule explainable rather
//! than a set of exceptions.

use std::collections::BTreeSet;

use crate::data::types::{
    Installation, InstallationType, TacticalObjectCategoryFacet, TacticalObjectDomainFacet,
    TacticalObjectFacets,
};
use crate::ui::{State, ViewId};

/// What **this pane** hides. Empty is everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    /// The live feed's layer.
    pub hulls: bool,
    /// The installations layer as a whole (#109 draws it; the filter is the promise it keeps).
    pub installations: bool,
    /// The tactical-objects layer as a whole.
    pub objects: bool,
    /// Installation type ids, from `/installation-types`.
    pub hidden_types: BTreeSet<i64>,
    /// Tactical-object domain ids, from `/tactical-objects/facets`.
    pub hidden_domains: BTreeSet<i64>,
    /// Tactical-object category ids, from `/tactical-objects/facets`.
    pub hidden_categories: BTreeSet<i64>,
}

impl Filter {
    /// Whether this pane draws no hulls at all.
    pub fn hides_hulls(&self) -> bool {
        self.hulls
    }

    /// Whether this pane draws no installations at all.
    pub fn hides_installations(&self) -> bool {
        self.installations
    }

    /// Whether this pane draws no tactical objects at all.
    pub fn hides_objects(&self) -> bool {
        self.objects
    }

    /// Whether **this** symbol is hidden: its layer, its domain, or its category.
    pub fn hides_object_in(&self, id_domain: i64, id_category: i64) -> bool {
        self.objects
            || self.hidden_domains.contains(&id_domain)
            || self.hidden_categories.contains(&id_category)
    }

    /// Whether **this** facility is hidden: its layer, or its type.
    ///
    /// #109's layer is the caller. It is written here rather than there because the question is the
    /// filter's, and a second copy of "domain and category" is how the two layers drift apart.
    pub fn hides_installation_type(&self, id_type: i64) -> bool {
        self.installations || self.hidden_types.contains(&id_type)
    }

    /// Flip one row of the panel. The only way a filter changes.
    pub fn toggle(&mut self, target: Target) {
        match target {
            Target::Layer(Layer::Hulls) => self.hulls = !self.hulls,
            Target::Layer(Layer::Installations) => self.installations = !self.installations,
            Target::Layer(Layer::Objects) => self.objects = !self.objects,
            Target::Type(id) => toggle_id(&mut self.hidden_types, id),
            Target::Domain(id) => toggle_id(&mut self.hidden_domains, id),
            Target::Category(id) => toggle_id(&mut self.hidden_categories, id),
        }
    }
}

fn toggle_id(set: &mut BTreeSet<i64>, id: i64) {
    if !set.remove(&id) {
        set.insert(id);
    }
}

/// The three layers a pane draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Hulls,
    Installations,
    Objects,
}

/// What a press on a row changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Layer(Layer),
    Type(i64),
    Domain(i64),
    Category(i64),
}

/// One row of the panel, resolved from the vocabulary and the pane's own filter.
///
/// A value rather than an element, so a test can ask what the panel *says* without a window: the labels,
/// their order and their counts are the backend's, and the state is the pane's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The press this row makes.
    pub target: Target,
    /// The stable part of the element key (`hulls`, `domain-2`, `category-3`).
    pub key: String,
    /// The backend's `name`, or the layer's word.
    pub label: String,
    /// The count the backend stated (a domain or category) or the cache holds (an installation type).
    /// `None` on a layer row: a layer says `on` or `off`, which is the whole of what it means.
    pub count: Option<u64>,
    /// Whether this row is currently hiding what it names.
    pub hidden: bool,
    /// A category sits under its domain; a domain and a type do not.
    pub depth: u8,
}

impl Row {
    /// The row's right-hand word: the count and the state for a vocabulary row, the state for a layer.
    ///
    /// Meaning in words where there is room (`AGENTS.md`): an operator should not have to infer "hidden"
    /// from a dimmer colour, because a wall is read from across a room.
    pub fn value(&self) -> String {
        let state = if self.hidden { "off" } else { "on" };
        match self.count {
            Some(count) => format!("{count} {state}"),
            None => state.to_string(),
        }
    }
}

/// The panel's rows for one pane: the layers, then the two vocabularies in the backend's order.
///
/// A domain's categories are grouped under it, because `facets.categories` is flat on the wire with an
/// `id_domain` linking it — the backend hands over a vocabulary, not a tree, and grouping it is the
/// panel's job. A category whose domain the payload did not carry has nowhere to be drawn and is left
/// out; the server's own contract says the two agree, so that case is a broken answer, not a choice.
pub fn rows(
    filter: &Filter,
    facets: Option<&TacticalObjectFacets>,
    types: &[InstallationType],
    installations: &[Installation],
) -> Vec<Row> {
    let mut rows = vec![
        layer_row(Layer::Hulls, "Hulls", filter.hides_hulls()),
        layer_row(
            Layer::Installations,
            "Installations",
            filter.hides_installations(),
        ),
        layer_row(Layer::Objects, "Tactical objects", filter.hides_objects()),
    ];

    // The facility vocabulary, in the order the backend draws its picker. Its **count comes from the
    // complete cache**, not from a page: `/installations` is unpaginated, so the wall holds every active
    // facility after its init read.
    let mut types: Vec<&InstallationType> = types.iter().collect();
    types.sort_by_key(|kind| (kind.sort_order, kind.id));
    for kind in types {
        let count = installations
            .iter()
            .filter(|installation| installation.id_installation_type == kind.id)
            .count() as u64;
        rows.push(Row {
            target: Target::Type(kind.id),
            key: format!("type-{}", kind.id),
            label: kind.name.clone(),
            count: Some(count),
            hidden: filter.hidden_types.contains(&kind.id),
            depth: 0,
        });
    }

    // The tactical-object vocabulary, with the counts the **server** stated for the catalogue.
    if let Some(facets) = facets {
        let mut domains: Vec<&TacticalObjectDomainFacet> = facets.domains.iter().collect();
        domains.sort_by_key(|domain| (domain.sort_order, domain.id));
        for domain in domains {
            rows.push(Row {
                target: Target::Domain(domain.id),
                key: format!("domain-{}", domain.id),
                label: domain.name.clone(),
                count: Some(domain.count),
                hidden: filter.hidden_domains.contains(&domain.id),
                depth: 0,
            });
            let mut categories: Vec<&TacticalObjectCategoryFacet> = facets
                .categories
                .iter()
                .filter(|category| category.id_domain == domain.id)
                .collect();
            categories.sort_by_key(|category| (category.sort_order, category.id));
            for category in categories {
                rows.push(Row {
                    target: Target::Category(category.id),
                    key: format!("category-{}", category.id),
                    label: category.name.clone(),
                    count: Some(category.count),
                    hidden: filter.hidden_categories.contains(&category.id),
                    depth: 1,
                });
            }
        }
    }

    rows
}

fn layer_row(layer: Layer, label: &str, hidden: bool) -> Row {
    Row {
        target: Target::Layer(layer),
        key: match layer {
            Layer::Hulls => "hulls".to_string(),
            Layer::Installations => "installations".to_string(),
            Layer::Objects => "objects".to_string(),
        },
        label: label.to_string(),
        count: None,
        hidden,
        depth: 0,
    }
}

/// A press on a row: **that pane's** filter changes, and no other's.
///
/// One function behind every row, so the panel cannot grow a row that changes the wall instead of the
/// pane — which is the bug #99 fixed for the selection, in its filter costume.
pub(crate) fn toggle_in(state: &mut State, pane: ViewId, target: Target) -> bool {
    let Some(pane) = state.pane_mut(pane) else {
        return false;
    };
    pane.view.filter.toggle(target);
    state.layout_dirty = true;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::Envelope;
    use crate::map::geometry::Camera;

    fn fixture(name: &str) -> String {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/rest")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    fn facets() -> TacticalObjectFacets {
        let envelope: Envelope<TacticalObjectFacets> =
            serde_json::from_str(&fixture("tactical-objects-facets.json"))
                .expect("the facets fixture is the wire shape");
        envelope.data
    }

    fn types() -> Vec<InstallationType> {
        let envelope: Envelope<Vec<InstallationType>> =
            serde_json::from_str(&fixture("installation-types.json"))
                .expect("the installation-types fixture is the wire shape");
        envelope.data
    }

    fn installations() -> Vec<Installation> {
        let envelope: Envelope<Vec<Installation>> =
            serde_json::from_str(&fixture("installations.json"))
                .expect("the installations fixture is the wire shape");
        envelope.data
    }

    /// A domain hides its categories; a category hides only itself. The precedence is one rule, and a
    /// filter that let a category bring a symbol back inside a hidden domain would be a second one.
    #[test]
    fn a_domain_hides_its_categories_and_a_category_only_itself() {
        let mut filter = Filter::default();
        assert!(
            !filter.hides_object_in(2, 2),
            "empty is everything: the default hides nothing"
        );

        filter.hidden_domains.insert(2);
        assert!(
            filter.hides_object_in(2, 2),
            "a hidden domain hides a category under it"
        );
        assert!(
            !filter.hides_object_in(3, 3),
            "a hidden domain hides nothing outside it"
        );

        let mut filter = Filter::default();
        filter.hidden_categories.insert(2);
        assert!(
            filter.hides_object_in(2, 2),
            "a hidden category hides its own symbols"
        );
        assert!(
            !filter.hides_object_in(2, 1),
            "a hidden category does not hide its sibling"
        );
        assert!(
            !filter.hides_object_in(3, 3),
            "a hidden category hides nothing in another domain"
        );
    }

    /// The layer switches are separate from the vocabulary: turning a layer off hides everything in it,
    /// and turning a type off hides only facilities of that type — the same shape on the other layer.
    #[test]
    fn a_layer_hides_its_whole_set_and_a_type_only_its_own() {
        let mut filter = Filter::default();
        assert!(!filter.hides_installation_type(3));
        filter.hidden_types.insert(3);
        assert!(filter.hides_installation_type(3));
        assert!(!filter.hides_installation_type(4));
        filter.installations = true;
        assert!(
            filter.hides_installation_type(4),
            "the layer hides every type, including ones nobody toggled"
        );

        let filter = Filter {
            objects: true,
            ..Filter::default()
        };
        assert!(
            filter.hides_object_in(2, 2),
            "the objects layer hides every domain and category"
        );
        assert!(!filter.hides_hulls(), "and only the objects layer");
    }

    /// **The panel is the backend's vocabulary**: its order is `sort_order`, its labels are `name`, and
    /// its counts are the payload's. A paused edit that renames a domain on the server changes the row
    /// here — which is what "no domain name appears in our source" means as a test.
    #[test]
    fn the_panel_is_the_backends_vocabulary_in_its_order() {
        let facets = facets();
        let filter = Filter::default();
        let panel = super::rows(&filter, Some(&facets), &[], &[]);

        let mut expected: Vec<(String, u64)> = Vec::new();
        let mut domains: Vec<&TacticalObjectDomainFacet> = facets.domains.iter().collect();
        domains.sort_by_key(|domain| (domain.sort_order, domain.id));
        for domain in domains {
            expected.push((domain.name.clone(), domain.count));
            let mut categories: Vec<&TacticalObjectCategoryFacet> = facets
                .categories
                .iter()
                .filter(|category| category.id_domain == domain.id)
                .collect();
            categories.sort_by_key(|category| (category.sort_order, category.id));
            for category in categories {
                expected.push((category.name.clone(), category.count));
            }
        }
        let drawn: Vec<(String, u64)> = panel
            .iter()
            .filter(|row| matches!(row.target, Target::Domain(_) | Target::Category(_)))
            .map(|row| {
                (
                    row.label.clone(),
                    row.count.expect("a vocabulary row has a count"),
                )
            })
            .collect();
        assert_eq!(
            drawn, expected,
            "the rows are the wire, in the wire's order"
        );

        // A category is drawn under its domain, and a domain's count is the server's total for it. The
        // numbers below come from the payload, so this test cannot agree with a name written here.
        let recorded = facets
            .domains
            .iter()
            .find(|domain| domain.id == 2)
            .expect("the recorded maritime domain");
        let maritime = panel
            .iter()
            .position(|row| row.target == Target::Domain(recorded.id))
            .expect("the domain's row");
        assert_eq!(
            panel[maritime].count,
            Some(recorded.count),
            "the server's count, not ours"
        );
        let first_category = facets
            .categories
            .iter()
            .filter(|category| category.id_domain == recorded.id)
            .min_by_key(|category| (category.sort_order, category.id))
            .expect("the recorded domain has categories");
        assert_eq!(panel[maritime + 1].depth, 1, "its categories are indented");
        assert_eq!(
            panel[maritime + 1].label,
            first_category.name,
            "the label is the backend's name, not one written here"
        );

        // Rename the domain on the wire and the row follows: a name written in our source would not.
        let mut renamed = facets.clone();
        renamed.domains[1].name = "Not A Recorded Name".to_string();
        let renamed_rows = super::rows(&filter, Some(&renamed), &[], &[]);
        assert!(
            renamed_rows
                .iter()
                .any(|row| row.label == "Not A Recorded Name"),
            "the panel draws the backend's name, whatever it is"
        );
    }

    /// A facility's count comes from the **complete cache**, not from a page of it: two facilities of one
    /// type are `2`, and a type nobody has built on is `0` — an absence that is a count, not a missing row.
    #[test]
    fn an_installation_type_is_counted_from_the_whole_cache() {
        let wire = r#"{"status_code":200,"message":"ok","data":[
            {"id":1,"id_installation_type":3,"type_name":"Pangkalan","id_service_branch":1,"name":"A","is_active":true},
            {"id":2,"id_installation_type":3,"type_name":"Pangkalan","id_service_branch":1,"name":"B","is_active":true},
            {"id":3,"id_installation_type":1,"type_name":"Posko","id_service_branch":1,"name":"C","is_active":true}]}"#;
        let envelope: Envelope<Vec<Installation>> =
            serde_json::from_str(wire).expect("the cache fixture is the wire shape");
        let rows = rows(&Filter::default(), None, &types(), &envelope.data);
        let count = |id: i64| {
            rows.iter()
                .find(|row| row.target == Target::Type(id))
                .and_then(|row| row.count)
        };
        assert_eq!(count(3), Some(2), "both Pangkalan rows are in the cache");
        assert_eq!(count(1), Some(1));
        assert_eq!(
            count(5),
            Some(0),
            "a type with no facilities is 0, not absent"
        );
        assert!(
            !installations()
                .iter()
                .any(|row| row.id_installation_type == 3),
            "the recorded cache is empty, so the count above is the cache's and not the fixture's"
        );
    }

    /// A toggle twice is the state you started in, and the default hides nothing: a filter is a set of
    /// exclusions, so "everything" is the empty value rather than a list somebody must keep complete.
    #[test]
    fn empty_is_everything_and_a_toggle_twice_is_nothing() {
        let mut filter = Filter::default();
        for target in [
            Target::Layer(Layer::Hulls),
            Target::Layer(Layer::Installations),
            Target::Layer(Layer::Objects),
            Target::Type(3),
            Target::Domain(2),
            Target::Category(2),
        ] {
            let before = filter.clone();
            filter.toggle(target);
            assert_ne!(filter, before, "{target:?} changes the filter");
            filter.toggle(target);
            assert_eq!(
                filter, before,
                "{target:?} twice is the state it started in"
            );
        }
        assert_eq!(
            filter,
            Filter::default(),
            "the default draws exactly what the wall drew before the filter existed"
        );
    }

    /// **The filter is the pane's** (#99, again). Two panes on one wall hold two filters: a press in one
    /// changes it and leaves its neighbour exactly as it was, which is the bug #99 fixed for the selection.
    #[test]
    fn a_filter_belongs_to_the_pane_that_was_pressed() {
        let mut state = State::default();
        let first = state.focused();
        let Some(second) = state.split_pane(first, Camera::START) else {
            panic!("the wall can hold two panes");
        };
        assert!(
            toggle_in(&mut state, first, Target::Domain(2)),
            "the press reaches the pane"
        );
        let filter_of =
            |state: &State, id: ViewId| state.pane(id).expect("pane").view.filter.clone();
        assert!(filter_of(&state, first).hides_object_in(2, 2));
        assert_eq!(
            filter_of(&state, second),
            Filter::default(),
            "the neighbour hides nothing: a wall-wide filter is the selection's old bug in a new place"
        );
    }

    /// The panel grows with its vocabulary and **stops at the token's maximum**, where it scrolls: a
    /// backend with more domains than the panel has room for is the ordinary case, not an error.
    #[test]
    fn the_panel_stops_growing_at_its_maximum() {
        let (width, short) = crate::ui::tokens::filter_size(3);
        assert_eq!(width, crate::ui::tokens::FILTER_WIDTH);
        assert!(
            (short - (crate::ui::tokens::FILTER_HEAD + 3.0 * crate::ui::tokens::FILTER_ROW)).abs()
                < 0.001,
            "three layer rows are three rows tall, got {short}"
        );
        let (_, tall) = crate::ui::tokens::filter_size(17);
        assert_eq!(
            tall,
            crate::ui::tokens::FILTER_MAX_H,
            "a taller vocabulary scrolls instead of growing over the picture"
        );
        let total = rows(
            &Filter::default(),
            Some(&facets()),
            &types(),
            &installations(),
        )
        .len();
        assert!(
            total > 12,
            "the recorded vocabulary is longer than the panel's box: {total} rows"
        );
    }
}
