//! The field lists this client models, one per schema it reads from the pinned spec.
//!
//! These are **the model contract**, not a decoder: the types in [`crate::data::types`] implement
//! them, and `tests/spec_drift.rs` fails when the snapshot makes required a field that is not listed
//! here, or drops a property that is. The lists are deliberately written by hand — a generated list
//! would agree with the generator, not with the server.
//!
//! Two schemas the display consumes are *not* JSON objects in the spec and so are absent here: the
//! `live-feed` publication is a bare `FeedPositionEvent` (described in `realtime-protocol.md` §5),
//! and the connect-frame credential is `{"token": "…"}`.

/// `GET /live-feed/positions` → one entry per hull, `position` absent when it never reported.
pub const FEED_POSITION: &[&str] = &["id_unit", "name", "hull_number", "position"];

/// The fix inside a `FeedPosition`. The four nullable measurements are `Option` (`INV-01`), and
/// `backfilled` is the flag that must never set the "here now" glyph (`INV-03`).
pub const FEED_FIX: &[&str] = &[
    "latitude",
    "longitude",
    "speed_kn",
    "speed_ms",
    "course_deg",
    "accuracy_m",
    "recorded_at",
    "received_at",
    "age_seconds",
    "backfilled",
];

/// The envelope `GET /live-feed/positions` returns.
pub const FEED_POSITION_LIST_ENVELOPE: &[&str] = &["status_code", "message", "data"];

/// `POST /auth/login` and `/auth/refresh`.
pub const AUTH_ENVELOPE: &[&str] = &["status_code", "message", "data"];

/// The pair inside [`AUTH_ENVELOPE`]. `refresh_token` is present only because the wall asks for it
/// (`return_refresh_token: true`).
pub const TOKEN_PAIR: &[&str] = &["access_token", "token_type", "expires_in", "refresh_token"];

/// Every 4xx/5xx body.
pub const ERROR_RESPONSE: &[&str] = &["status_code", "message", "data"];

/// The `game:<id>:positions` payload — bare, no `type`, at `pub.data.positions` (v0.2, #11). The
/// ticket that specified `tests/spec_drift.rs` called this `GamePositionEvent`; there is no such
/// schema in the snapshot, and this is the object that carries `positions`.
pub const GAME_POSITION_LIST: &[&str] = &["assumed_time", "positions", "from_unit"];

/// `GET /units/{id}`.
pub const UNIT_DETAIL: &[&str] = &["id", "name", "hull_number", "unit_class", "unit_status"];

/// A lookup row (`unit_status`, `service_branch`, a category).
pub const HELPER: &[&str] = &["id", "name", "id_name", "is_system", "description_en"];

/// A hull's class, with the category it belongs to.
pub const UNIT_CLASS_SUMMARY: &[&str] = &["id", "name", "id_name", "unit_type", "category"];

/// `GET /units/{id}/commander` — one assignment; the current one has no `to_at`.
pub const UNIT_COMMANDER_SUMMARY: &[&str] = &[
    "id_unit",
    "id_user",
    "user_name",
    "from_at",
    "to_at",
    "assigned_by",
    "note",
];

/// `GET /units/{id}/specifications` — an immutable, append-only version.
pub const UNIT_SPECIFICATION: &[&str] = &[
    "version",
    "is_current",
    "loa_m",
    "beam_m",
    "draft_m",
    "displacement_standard_t",
    "displacement_full_t",
    "range_nm",
    "endurance_days",
    "crew_personnel",
    "embarked_personnel",
    "turn_rate_max_deg_s",
    "turn_rate_is_derived",
    "spec",
];

// ---------------------------------------------------------------------------------------------
// The operational resources (#107). Field names are the spec's, verbatim; every field the server
// omits in a real response is modelled (and decoded as `Option`) rather than dropped.
// ---------------------------------------------------------------------------------------------

/// `GET /viewpoint` — both coordinates, or neither. The schema has **no** required list, so
/// `tests/spec_drift.rs` reads it with a dedicated test rather than through [`MODELLED`].
pub const VIEWPOINT_SUMMARY: &[&str] = &["latitude", "longitude"];

/// `GET /installation-types`. `symbol_code` is a code into the tactical-object catalogue, not an id.
pub const INSTALLATION_TYPE_SUMMARY: &[&str] = &[
    "id",
    "name",
    "id_name",
    "description_en",
    "sort_order",
    "is_system",
    "symbol_code",
];

/// `GET /installations` — the position is optional but indivisible.
pub const INSTALLATION_SUMMARY: &[&str] = &[
    "id",
    "id_installation_type",
    "type_name",
    "id_service_branch",
    "name",
    "id_name",
    "description_en",
    "note",
    "latitude",
    "longitude",
    "is_active",
];

/// One entry of `GET /tactical-objects`: the library, not a map.
pub const TACTICAL_OBJECT: &[&str] = &[
    "id",
    "code",
    "name",
    "id_name",
    "description",
    "category",
    "sort_order",
    "is_active",
    "is_seeded",
    "asset_source",
    "asset_url",
];

/// The category a symbol belongs to, with its domain.
pub const TACTICAL_OBJECT_CATEGORY_REF: &[&str] =
    &["id", "name", "id_name", "sort_order", "domain"];

/// The domain a symbol's category belongs to.
pub const TACTICAL_OBJECT_DOMAIN_REF: &[&str] = &["id", "name", "id_name", "sort_order"];

/// `GET /tactical-objects/manifest` — unpaginated on purpose.
pub const TACTICAL_OBJECT_MANIFEST: &[&str] = &["version", "symbols"];

/// One manifest entry. A signing failure omits `asset_url`, not the entry.
pub const TACTICAL_OBJECT_MANIFEST_ENTRY: &[&str] = &[
    "code",
    "name",
    "id_name",
    "category",
    "domain",
    "asset_key",
    "asset_url",
    "sort_order",
];

/// `GET /tactical-objects/facets` — the filter vocabulary and its counts.
pub const TACTICAL_OBJECT_FACETS: &[&str] = &["total", "domains", "categories"];

/// One domain row; `count` is the sum of its categories' counts.
pub const TACTICAL_OBJECT_DOMAIN_FACET: &[&str] = &["id", "name", "id_name", "sort_order", "count"];

/// One category row. Zero is returned, not omitted.
pub const TACTICAL_OBJECT_CATEGORY_FACET: &[&str] =
    &["id", "id_domain", "name", "id_name", "sort_order", "count"];

/// `GET /tactical-object-placements` — one symbol at one coordinate, embedding the symbol.
pub const TACTICAL_OBJECT_PLACEMENT: &[&str] = &[
    "id",
    "symbol",
    "latitude",
    "longitude",
    "label",
    "rotation_deg",
    "created_at",
    "updated_at",
];

/// `GET /unit-organisations` — a standing order of battle.
pub const UNIT_ORGANISATION_SUMMARY: &[&str] = &[
    "id",
    "id_service_branch",
    "name",
    "id_name",
    "description_en",
    "note",
    "is_system",
];

/// One post in a standing structure. `children` is always an array; several roots are normal.
pub const UNIT_ORGANISATION_NODE_SUMMARY: &[&str] = &[
    "id",
    "id_organisation",
    "parent_id",
    "id_unit_hierarchy",
    "echelon_name",
    "echelon_rank",
    "id_unit",
    "id_commander",
    "commander",
    "name",
    "position_title",
    "icon",
    "note",
    "children",
];

/// The holder of a node, as a tree needs them.
pub const NODE_COMMANDER_SUMMARY: &[&str] =
    &["id_user", "name", "username", "nrp", "pangkat", "jabatan"];

/// One appointment of the supreme command.
pub const TNI_COMMANDER_SUMMARY: &[&str] = &[
    "id",
    "id_user",
    "name",
    "username",
    "nrp",
    "pangkat",
    "jabatan",
    "from_at",
    "to_at",
    "is_current",
    "assigned_by",
    "note",
];

/// `GET /tni-commander` — the post; `commander` is absent when it is vacant.
pub const TNI_COMMAND_POST_SUMMARY: &[&str] = &[
    "id",
    "id_unit_hierarchy",
    "echelon_name",
    "echelon_rank",
    "position_title",
    "note",
    "commander",
];

// ---------------------------------------------------------------------------------------------
// The newsroom (#135): Minos' strategic-environment feed. The API's word for the route is
// `banglinstra`; this crate's word is the newsroom, which is what the panel is called. A row is a
// STORY, not an article — one wire story carried by seven outlets is one row with
// `source_count: 7` — and the detail is that story with the documents behind it, oldest first.
// ---------------------------------------------------------------------------------------------

/// The `country` inside a story. **Absent when the corpus carried no usable geography**, and
/// `place` is absent unless at least half the document's location rows agreed.
pub const NEWS_COUNTRY: &[&str] = &["code", "name", "place", "confidence"];

/// One row of `GET /banglinstra/stories`.
pub const NEWS_STORY: &[&str] = &[
    "id",
    "title",
    "source_count",
    "country",
    "source",
    "url",
    "first_seen",
    "last_seen",
];

/// One document inside a story. `persons`, `tone`, `event_code` and `quad_class` are all absent in
/// real responses; `flagged` marks a domain on neither the allow nor the deny list.
pub const NEWS_ARTICLE: &[&str] = &[
    "id",
    "title",
    "domain",
    "url",
    "flagged",
    "persons",
    "tone",
    "event_code",
    "quad_class",
    "seen_at",
];

/// `GET /banglinstra/stories/{id}` — the story and its sources, oldest first.
pub const NEWS_STORY_DETAIL: &[&str] = &["story", "articles"];

/// `GET /unit-organisations/{id}` — the structure's summary plus its forest of roots.
///
/// The schema is an `allOf` composition, so its `required: [nodes]` sits inside the inline half and
/// [`MODELLED`]'s text parser cannot read it. `tests/spec_drift.rs` reads it with a dedicated test.
pub const UNIT_ORGANISATION_DETAIL: &[&str] = &["id", "id_service_branch", "name", "nodes"];

/// Every schema this client models, by the name it has in `docs/spec/minos-api.yaml`.
pub const MODELLED: &[(&str, &[&str])] = &[
    ("FeedPosition", FEED_POSITION),
    ("FeedFix", FEED_FIX),
    ("FeedPositionListEnvelope", FEED_POSITION_LIST_ENVELOPE),
    ("AuthEnvelope", AUTH_ENVELOPE),
    ("TokenPair", TOKEN_PAIR),
    ("ErrorResponse", ERROR_RESPONSE),
    ("GamePositionList", GAME_POSITION_LIST),
    ("UnitDetail", UNIT_DETAIL),
    ("Helper", HELPER),
    ("UnitClassSummary", UNIT_CLASS_SUMMARY),
    ("UnitCommanderSummary", UNIT_COMMANDER_SUMMARY),
    ("UnitSpecification", UNIT_SPECIFICATION),
    ("InstallationTypeSummary", INSTALLATION_TYPE_SUMMARY),
    ("InstallationSummary", INSTALLATION_SUMMARY),
    ("TacticalObject", TACTICAL_OBJECT),
    ("TacticalObjectCategoryRef", TACTICAL_OBJECT_CATEGORY_REF),
    ("TacticalObjectDomainRef", TACTICAL_OBJECT_DOMAIN_REF),
    ("TacticalObjectManifest", TACTICAL_OBJECT_MANIFEST),
    (
        "TacticalObjectManifestEntry",
        TACTICAL_OBJECT_MANIFEST_ENTRY,
    ),
    ("TacticalObjectFacets", TACTICAL_OBJECT_FACETS),
    ("TacticalObjectDomainFacet", TACTICAL_OBJECT_DOMAIN_FACET),
    (
        "TacticalObjectCategoryFacet",
        TACTICAL_OBJECT_CATEGORY_FACET,
    ),
    ("TacticalObjectPlacement", TACTICAL_OBJECT_PLACEMENT),
    ("UnitOrganisationSummary", UNIT_ORGANISATION_SUMMARY),
    (
        "UnitOrganisationNodeSummary",
        UNIT_ORGANISATION_NODE_SUMMARY,
    ),
    ("NodeCommanderSummary", NODE_COMMANDER_SUMMARY),
    ("TniCommanderSummary", TNI_COMMANDER_SUMMARY),
    ("TniCommandPostSummary", TNI_COMMAND_POST_SUMMARY),
    ("BanglinstraCountry", NEWS_COUNTRY),
    ("BanglinstraStory", NEWS_STORY),
    ("BanglinstraArticle", NEWS_ARTICLE),
    ("BanglinstraStoryDetail", NEWS_STORY_DETAIL),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_modelled_schema_names_at_least_one_field() {
        // A typo that empties a list would make the drift test pass vacuously.
        assert!(!MODELLED.is_empty());
        for (schema, fields) in MODELLED {
            assert!(
                !fields.is_empty(),
                "{schema} models no fields, so its drift check proves nothing"
            );
        }
    }

    #[test]
    fn the_measurements_that_may_be_absent_are_not_in_a_required_list() {
        // INV-01 is a type fact before it is a rendering fact: these four names exist and are never
        // required, because "unmeasured" has to be representable.
        for (schema, fields) in [("FEED_FIX", FEED_FIX)] {
            for measurement in ["speed_kn", "speed_ms", "course_deg", "accuracy_m"] {
                assert!(
                    fields.contains(&measurement),
                    "{measurement} must be modelled in {schema}"
                );
            }
        }
    }
}
