//! The wire types, and the two rules that make them safe to draw.
//!
//! **Absence is not zero** (`INV-01`). `speed_kn`, `speed_ms`, `course_deg` and `accuracy_m` are
//! omitted — or `null` — when unmeasured, and a `0.0` on the wire is a *measured* zero. Every one is
//! `Option<f64>`, `None` means "not measured", and nothing in this crate may `unwrap_or(0.0)` one.
//!
//! **Unknown fields must not break parsing** (`docs/verification.md` rule 8). The server adds fields
//! without ceremony; `deny_unknown_fields` is how a client breaks on a release that changed nothing
//! for it. Nothing here denies them, and `data/tests` proves it with a recorded payload carrying a
//! field the spec does not have.
//!
//! Types are hand-written and small on purpose. Codegen from the OpenAPI would add a build step for
//! five endpoints, and the thing that goes wrong with this API is not the shape of a struct.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// `{status_code, message, data, metadata}` — the envelope every Minos route returns, success or failure.
///
/// `metadata` is **present only on paginated list endpoints**, and it is the difference between a client
/// that reads a register and one that reads its first page and believes it has the fleet (`#52`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Envelope<T> {
    pub status_code: i32,
    pub message: String,
    pub data: T,
    #[serde(default)]
    pub metadata: Option<Pagination>,
}

/// The pagination block of a list response.
///
/// `has_next_page` is the field a client follows; `total_records` is the field an operator reads when the
/// wall says it knows 50 hulls and the deployment has 127. Both are kept: one is the loop's condition and
/// the other is the check that the loop did its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct Pagination {
    pub total_records: u64,
    pub total_pages: u32,
    pub page_size: u32,
    pub current_page: u32,
    pub has_next_page: bool,
    #[serde(default)]
    pub has_previous_page: bool,
}

/// The fleet register, as read: every hull the server has, and what the read cost.
///
/// `truncated` is not decoration. A page cap that quietly returned the first N pages is exactly the class
/// of bug this ticket is about, so the caller is told when the cap was reached and can say so.
#[derive(Debug, Clone, Default)]
pub struct Register {
    pub units: Vec<UnitSummary>,
    pub pages: u32,
    pub total_records: Option<u64>,
    pub truncated: bool,
}

/// One hull in the standing picture, with its last known fix if it has ever reported.
///
/// A hull with `position: None` belongs in the picture and not on the map (`INV-02`). On the dev
/// feed that is 126 of 127 hulls: the common case, not the edge case.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FeedPosition {
    pub id_unit: i64,
    pub name: String,
    /// Omitted for hulls the source lists without one. Not an empty string, not a zero.
    #[serde(default)]
    pub hull_number: Option<String>,
    /// **Absent** (the key is missing, not `null`) when the hull has never reported.
    #[serde(default)]
    pub position: Option<FeedFix>,
}

/// One accepted position report.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct FeedFix {
    pub latitude: f64,
    pub longitude: f64,
    /// Knots, or `None` when unmeasured. `Some(0.0)` is a measured stop.
    #[serde(default)]
    pub speed_kn: Option<f64>,
    /// Metres per second — **the same fact as `speed_kn`**, absent exactly when it is.
    ///
    /// The HTTP read does not send it; the `live-feed` event does (`realtime-protocol.md` §5).
    /// `speed_kn` is authoritative here: this field is carried so the socket path (#4) can use the
    /// same type, and nothing converts one into the other.
    #[serde(default)]
    pub speed_ms: Option<f64>,
    /// Degrees true, or `None` when unmeasured. `Some(0.0)` is due north.
    #[serde(default)]
    pub course_deg: Option<f64>,
    /// Metres, or `None` when unmeasured.
    #[serde(default)]
    pub accuracy_m: Option<f64>,
    pub recorded_at: DateTime<Utc>,
    pub received_at: DateTime<Utc>,
    /// How old the fix is, measured against the **server's** clock at response time.
    ///
    /// This is the only safe source for age: computing it locally would require trusting this
    /// machine's clock against the server's, and on these machines they differ by months. Between
    /// reads, age is this value plus local *monotonic* elapsed time (`INV-06`).
    pub age_seconds: f64,
    /// Reconstructed from an offline backlog: real, the newest thing known, and it did not just
    /// happen. It must never set the "here now" glyph (`INV-03`).
    pub backfilled: bool,
}

/// A lookup row: `name` is the canonical English value, `id_name` the Indonesian label to show.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Helper {
    pub id: i64,
    pub name: String,
    pub id_name: String,
    pub is_system: bool,
    #[serde(default)]
    pub description_en: Option<String>,
}

/// A hull's class, with the category the class belongs to so one payload draws the whole chain.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitClassSummary {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub id_name: Option<String>,
    #[serde(default)]
    pub unit_type: Option<Helper>,
    pub category: Helper,
}

/// `GET /units/{id}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitDetail {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub hull_number: Option<String>,
    pub unit_class: UnitClassSummary,
    pub unit_status: Helper,
}

/// One row of `GET /units`: the wall's map from hull to photograph.
///
/// The live feed carries positions and nothing else, so `image_url` — the only place a hull's picture is
/// named — has to come from the unit register. One read for the whole fleet, not one per hull.
#[derive(Debug, Clone, Deserialize)]
pub struct UnitSummary {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub hull_number: Option<String>,
    /// A **presigned** URL, absent when the hull has no photograph. It changes when the picture is
    /// replaced *and* when the signature expires, so it is cached by value and refreshed, never assumed
    /// stable.
    #[serde(default)]
    pub image_url: Option<String>,
}

/// One entry of `GET /units/{id}/commander`, newest first.
///
/// **The current commander is the entry with no `to_at`.** There is no "who commands it now" route,
/// so ordering and currency cannot disagree. Nobody having ever commanded a hull is an empty list,
/// which is a valid answer and not a 404.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitCommander {
    pub id_unit: i64,
    pub id_user: i64,
    pub user_name: String,
    pub from_at: DateTime<Utc>,
    #[serde(default)]
    pub to_at: Option<DateTime<Utc>>,
    pub assigned_by: i64,
    pub note: String,
}

impl UnitCommander {
    /// Whether this assignment is the open one.
    pub fn is_current(&self) -> bool {
        self.to_at.is_none()
    }
}

/// One version of a hull's specification. Append-only: editing a figure inserts a new version.
///
/// Every measurement is nullable, and **null means unknown, not zero** — the source data carries no
/// submerged speed for any submarine, and a client that reads a missing speed as zero freezes the
/// vessel while one that reads it as unbounded lets it outrun a frigate.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitSpecification {
    pub version: i64,
    pub is_current: bool,
    #[serde(default)]
    pub loa_m: Option<f64>,
    #[serde(default)]
    pub beam_m: Option<f64>,
    #[serde(default)]
    pub draft_m: Option<f64>,
    #[serde(default)]
    pub displacement_standard_t: Option<f64>,
    #[serde(default)]
    pub displacement_full_t: Option<f64>,
    #[serde(default)]
    pub range_nm: Option<f64>,
    #[serde(default)]
    pub endurance_days: Option<i64>,
    #[serde(default)]
    pub crew_personnel: Option<i64>,
    #[serde(default)]
    pub embarked_personnel: Option<i64>,
    #[serde(default)]
    pub turn_rate_max_deg_s: Option<f64>,
    /// True when the value was derived rather than supplied by an operator. `None` when the field is
    /// absent, which is not the same as "not derived".
    #[serde(default)]
    pub turn_rate_is_derived: Option<bool>,
    /// The free-form note block (`armament`, `propulsion`, …). Kept as JSON: it is operator prose
    /// with no schema, and inventing fields for it is how a client breaks on the next entry.
    #[serde(default)]
    pub spec: Option<serde_json::Value>,
}

/// A token pair. **The tokens never print** (`INV-08`): the fields are private, the accessors are
/// explicit, and `Debug` is written by hand.
#[derive(Clone, PartialEq, Deserialize)]
pub struct TokenPair {
    access_token: String,
    #[serde(default)]
    token_type: String,
    expires_in: i64,
    #[serde(default)]
    refresh_token: Option<String>,
}

impl TokenPair {
    /// The bearer token. Named `expose` so that every use reads like one.
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// The rotated refresh token, when the server sent one.
    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    /// Lifetime in seconds, as the server stated it.
    pub fn expires_in(&self) -> i64 {
        self.expires_in
    }
}

/// Redacted by hand, not by string filtering: there is no path from a token to a log line.
impl std::fmt::Debug for TokenPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenPair")
            .field("access_token", &"<redacted>")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// The body of a failed request: `{status_code, message, data}`.
///
/// Branch on `status_code` and on the field names in `data.errors`, **never on `message`** — the
/// message is prose for a human and is free to change. The ticket that specified this module called
/// the machine-readable field `code`; the pinned spec has no such field, so this models what the
/// server actually sends and says so instead of inventing one.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ErrorResponse {
    pub status_code: i32,
    pub message: String,
    #[serde(default)]
    pub data: Option<ErrorData>,
}

/// The `data` of an [`ErrorResponse`]: `null` when the failure is not attributable to a named input.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ErrorData {
    #[serde(default)]
    pub errors: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------------------------
// The operational resources (#107): where the map opens, the facilities, the symbol library and
// the operational marks, the standing orders of battle, and the supreme command.
//
// Every field the spec lists as absent in a real response is `Option`, and `None` means the server
// did not send it — never a default (`INV-01`). The recorded fixtures carry the absences the dev
// instance actually produces (an unset viewpoint, a vacant supreme command, a facility-less
// register); the tests assert them rather than filling them in.
// ---------------------------------------------------------------------------------------------

/// A read that spans pages: the rows, and what the read cost.
///
/// The same shape the fleet register uses, because the failure it guards is the same one (`#52`): a
/// client that reads page one and believes it has the catalogue draws part of it and says nothing.
#[derive(Debug, Clone)]
pub struct Pages<T> {
    pub rows: Vec<T>,
    pub pages: u32,
    pub total_records: Option<u64>,
    pub truncated: bool,
}

impl<T> Default for Pages<T> {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            pages: 0,
            total_records: None,
            truncated: false,
        }
    }
}

/// `GET /viewpoint` — where the map opens by default.
///
/// **Both fields are absent together** when the deployment has not chosen a view. `0,0` is never
/// sent: it is a real place in the Gulf of Guinea, and a client that defaulted to it would draw a
/// chosen view where there is none. [`Viewpoint::coordinates`] is the only way to read the pair, so
/// half a position cannot be drawn by accident.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
pub struct Viewpoint {
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
}

impl Viewpoint {
    /// The configured view, or `None` when this deployment has not chosen one.
    pub fn coordinates(&self) -> Option<(f64, f64)> {
        Some((self.latitude?, self.longitude?))
    }
}

/// `GET /installation-types` — the facility vocabulary, in picker order.
///
/// `symbol_code` is a **code** into the tactical-object catalogue, not an id. The catalogue is
/// reconciled at startup, so a kind may name a code the library does not carry yet: resolve it and
/// draw a generic marker when it does not match. A read never fails over it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct InstallationType {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub id_name: Option<String>,
    #[serde(default)]
    pub description_en: Option<String>,
    pub sort_order: i32,
    #[serde(default)]
    pub is_system: Option<bool>,
    #[serde(default)]
    pub symbol_code: Option<String>,
}

/// `GET /installations` — a facility: a posko, a base, a headquarters.
///
/// Not a hull: it has no hull number, class or movement domain. **The position is optional but
/// indivisible** — both coordinates or neither — so [`Installation::coordinates`] returns the pair
/// and nothing else. `latitude: Some(0.0)` is the Equator, not "unknown".
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Installation {
    pub id: i64,
    pub id_installation_type: i64,
    /// The joined kind, so a client need not resolve it.
    #[serde(default)]
    pub type_name: Option<String>,
    pub id_service_branch: i64,
    pub name: String,
    #[serde(default)]
    pub id_name: Option<String>,
    #[serde(default)]
    pub description_en: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    pub is_active: bool,
}

impl Installation {
    /// The facility's position, or `None` when it has not been located.
    pub fn coordinates(&self) -> Option<(f64, f64)> {
        Some((self.latitude?, self.longitude?))
    }
}

/// One entry in the symbol library (`GET /tactical-objects`).
///
/// The **library**, not a map: editing an entry rewrites no placement. `code` is the stable identity;
/// `name` and `id_name` are display strings. `asset_url` is a time-limited URL and is **absent** when
/// object storage has no object at the key — a restored database without its bucket is the ordinary
/// case, not an error.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObject {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub id_name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub category: TacticalObjectCategory,
    pub sort_order: i32,
    pub is_active: bool,
    pub is_seeded: bool,
    /// The wire's word: `none`, `seed` or `upload`. Kept as a string on purpose — the wall does not
    /// branch on it, and an enum without an unknown arm would break parsing on the next source the
    /// server adds, which is exactly what rule 8 forbids.
    pub asset_source: String,
    #[serde(default)]
    pub asset_url: Option<String>,
}

/// The category a symbol belongs to, with the domain it belongs to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectCategory {
    pub id: i64,
    pub name: String,
    pub id_name: String,
    pub sort_order: i32,
    pub domain: TacticalObjectDomain,
}

/// The domain a symbol's category belongs to.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectDomain {
    pub id: i64,
    pub name: String,
    pub id_name: String,
    pub sort_order: i32,
}

/// `GET /tactical-objects/manifest` — the whole active library, for offline caching.
///
/// **Unpaginated on purpose**: a page of a cache manifest is an incomplete cache, and the symbol a
/// client is missing is exactly the one it cannot draw.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectManifest {
    pub version: i32,
    pub symbols: Vec<TacticalObjectManifestEntry>,
}

/// One manifest entry: what a client needs to draw a symbol offline.
///
/// A signing failure **omits `asset_url`, not the entry**: the map still needs to know the symbol
/// exists and what it is called.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectManifestEntry {
    pub code: String,
    pub name: String,
    pub id_name: String,
    pub category: String,
    pub domain: String,
    pub asset_key: String,
    #[serde(default)]
    pub asset_url: Option<String>,
    pub sort_order: i32,
}

/// `GET /tactical-objects/facets` — the picker's filter vocabulary, with counts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectFacets {
    pub total: u64,
    pub domains: Vec<TacticalObjectDomainFacet>,
    pub categories: Vec<TacticalObjectCategoryFacet>,
}

/// One domain row: `count` is the sum of its categories' counts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectDomainFacet {
    pub id: i64,
    pub name: String,
    pub id_name: String,
    pub sort_order: i32,
    pub count: u64,
}

/// One category row. A category with no symbols is returned with `count: 0`, not omitted.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectCategoryFacet {
    pub id: i64,
    pub id_domain: i64,
    pub name: String,
    pub id_name: String,
    pub sort_order: i32,
    pub count: u64,
}

/// One library symbol at one coordinate on the operational map.
///
/// **Not a vessel and not a track**: fixed furniture with no speed, course or commander. It
/// **embeds** its whole symbol, so drawing never needs a second lookup. `label` and `rotation_deg`
/// are absent in real responses even though the spec lists them as required, and neither defaults.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TacticalObjectPlacement {
    pub id: i64,
    pub symbol: TacticalObject,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub rotation_deg: Option<f64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// `GET /unit-organisations` — a standing order of battle, one per service branch.
///
/// Not `/hierarchy` (the echelon vocabulary) and not `/games/{id}/hierarchy` (one exercise's
/// arrangement of it).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitOrganisation {
    pub id: i64,
    pub id_service_branch: i64,
    pub name: String,
    #[serde(default)]
    pub id_name: Option<String>,
    #[serde(default)]
    pub description_en: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub is_system: Option<bool>,
}

/// `GET /unit-organisations/{id}` — the structure and its forest.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitOrganisationDetail {
    #[serde(flatten)]
    pub organisation: UnitOrganisation,
    /// The **roots** of the forest, highest level first. More than one entry is normal.
    pub nodes: Vec<UnitOrganisationNode>,
}

/// One post in a standing structure.
///
/// `echelon_name`, `echelon_rank`, `id_unit`, `id_commander`, `commander`, `position_title`, `icon`
/// and `note` are all absent in a real response, and each absence is a real state: a post with no
/// level, no hull, nobody appointed, no title. None may default — `echelon_rank: 0` is never sent.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UnitOrganisationNode {
    pub id: i64,
    pub id_organisation: i64,
    /// Absent for a root. Several roots are normal.
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub id_unit_hierarchy: Option<i64>,
    #[serde(default)]
    pub echelon_name: Option<String>,
    #[serde(default)]
    pub echelon_rank: Option<i32>,
    #[serde(default)]
    pub id_unit: Option<i64>,
    #[serde(default)]
    pub id_commander: Option<i64>,
    #[serde(default)]
    pub commander: Option<NodeCommander>,
    pub name: String,
    #[serde(default)]
    pub position_title: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    /// Always an array, never `null` — for a leaf as much as for a branch.
    pub children: Vec<UnitOrganisationNode>,
}

/// The holder of a node, as a tree needs them: a name and a rank beside a post, nothing else.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NodeCommander {
    pub id_user: i64,
    pub name: String,
    pub username: String,
    #[serde(default)]
    pub nrp: Option<String>,
    #[serde(default)]
    pub pangkat: Option<String>,
    #[serde(default)]
    pub jabatan: Option<String>,
}

/// `GET /tni-commander/history` — one appointment of the supreme command, newest first.
///
/// `to_at` is **absent for the incumbent**, and `is_current` restates that fact so no client has to
/// infer it from a nullable timestamp.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TniCommander {
    pub id: i64,
    pub id_user: i64,
    pub name: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub nrp: Option<String>,
    #[serde(default)]
    pub pangkat: Option<String>,
    #[serde(default)]
    pub jabatan: Option<String>,
    pub from_at: DateTime<Utc>,
    #[serde(default)]
    pub to_at: Option<DateTime<Utc>>,
    pub is_current: bool,
    pub assigned_by: i64,
    #[serde(default)]
    pub note: Option<String>,
}

/// `GET /tni-commander` — the supreme command, one post shared by every service branch.
///
/// **`commander` is absent when the post is vacant.** That is a real state, not an error, and the
/// same one a hull with nobody appointed to it is in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TniCommandPost {
    pub id: i64,
    pub id_unit_hierarchy: i64,
    #[serde(default)]
    pub echelon_name: Option<String>,
    pub echelon_rank: i32,
    pub position_title: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub commander: Option<TniCommander>,
}

// ---------------------------------------------------------------------------------------------
// The newsroom (#135): Minos' strategic-environment feed. The API's word for the route is
// `banglinstra`; this crate's word is the newsroom, which is what the panel is called.
//
// A row is a **story**, not an article: the same wire story that seven outlets carried is one row
// with `source_count: 7`, and the detail is that story with the documents behind it, oldest first.
//
// Every field the server omits is `Option` (`INV-01`): a story's `country` when the corpus carried
// no usable geography, that country's `place` unless half the location rows agreed, and an
// article's `persons`, `tone`, `event_code` and `quad_class`. The panel draws a dash for each;
// none of them may become `""` or `0`.
// ---------------------------------------------------------------------------------------------

/// Which tab a story is filed under: a predicate over the **event's** country, not the outlet's.
///
/// `Unknown` is a real scope and not a null — roughly 30% of documents carry no usable location —
/// so it is modelled rather than folded into an absence. The panel has no fourth tab: the wall's
/// `min_confidence` drops `unknown` stories anyway, because a story with no country carries no
/// confidence to compare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewsScope {
    International,
    Regional,
    National,
    Unknown,
}

impl NewsScope {
    /// The wire's word, for the query string.
    pub fn as_str(self) -> &'static str {
        match self {
            NewsScope::International => "international",
            NewsScope::Regional => "regional",
            NewsScope::National => "national",
            NewsScope::Unknown => "unknown",
        }
    }
}

/// The `Terkini`/`Arsip` toggle, as the wire names it.
///
/// **`Latest` is the last 24 hours OF THE DATA HELD**, anchored by the server to the newest article
/// it has — never to this machine's clock. The client therefore sends the word and computes no
/// cutoff: an air-gapped box started on a three-day-old corpus shows those three days rather than
/// an empty tab, which is the failure the rule exists for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NewsWindow {
    #[default]
    Latest,
    Archive,
}

impl NewsWindow {
    /// The wire's word, for the query string.
    pub fn as_str(self) -> &'static str {
        match self {
            NewsWindow::Latest => "latest",
            NewsWindow::Archive => "archive",
        }
    }
}

/// What the newsroom asked for: the tab, the window and the page.
///
/// The query travels **with the reply** (`Command::Stories` → `Reply::Stories`) so a page that
/// arrives after the operator changed tab or pressed Next can be compared with what is on screen
/// now and discarded. Applying "the newest arrival" would draw page 2 of the regional tab under the
/// national heading.
///
/// `min_confidence` and `page_size` are deliberately not fields: the wall makes one choice for
/// each (`http::MIN_CONFIDENCE`, `http::NEWS_PAGE_SIZE`), so no caller can ask for a page the panel
/// has no control for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewsQuery {
    pub scope: NewsScope,
    pub window: NewsWindow,
    /// 1-based, as the server counts pages.
    pub page: u32,
}

impl NewsQuery {
    /// Where the panel opens: the latest 24 hours of one tab, page one.
    pub fn latest(scope: NewsScope) -> Self {
        Self {
            scope,
            window: NewsWindow::Latest,
            page: 1,
        }
    }
}

/// The Negara/Lokasi column: the country the event happened in, and the share of the corpus behind
/// it. Kept together because half of it is misleading on its own — a weak match must read as weak.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewsCountry {
    /// ISO 3166-1 alpha-2, resolved at ingest from GDELT's FIPS 10-4 code.
    pub code: String,
    /// The country as the corpus wrote it. This is what the panel shows.
    pub name: String,
    /// The city, present **only** when it held at least half of the document's location rows.
    /// Absent means "not established", not "no city".
    #[serde(default)]
    pub place: Option<String>,
    /// The share of the document's location rows behind `code`, 0–1. A fact to show, never a
    /// filter to re-apply: the server has already applied the one the operator asked for.
    pub confidence: f64,
}

/// One row of the panel.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewsStory {
    pub id: i64,
    /// The headline as the story was first named, never rewritten as copies arrive.
    pub title: String,
    /// How many documents carried it. Seven means seven outlets ran it, which is why the panel
    /// lists stories rather than articles.
    pub source_count: u32,
    /// **Absent** when the corpus carried no usable geography. The panel draws a dash; it does not
    /// guess, and `code: ""` is never sent.
    #[serde(default)]
    pub country: Option<NewsCountry>,
    /// The lead article's registrable domain.
    pub source: String,
    /// The lead article's URL — **provenance, not a browsing affordance**: on an air-gapped wall it
    /// cannot be opened, so it is drawn as text and never as a button.
    pub url: String,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

/// One document inside a story, oldest first in the detail.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewsArticle {
    pub id: i64,
    pub title: String,
    pub domain: String,
    pub url: String,
    /// The domain is on **neither** the allow nor the deny list. Shown rather than filtered: an
    /// unclassified source is a question for an operator, and an invisible gap is worse.
    pub flagged: bool,
    #[serde(default)]
    pub persons: Option<Vec<String>>,
    /// GDELT's average document tone. Absent when the source carried none; `Some(0.0)` is a
    /// measured neutral and must still draw (`INV-01`).
    #[serde(default)]
    pub tone: Option<f64>,
    /// CAMEO code, absent for roughly 73% of documents — those are still articles.
    #[serde(default)]
    pub event_code: Option<String>,
    #[serde(default)]
    pub quad_class: Option<u32>,
    /// GDELT's own processing clock, UTC.
    pub seen_at: DateTime<Utc>,
}

/// `GET /banglinstra/stories/{id}` — the story and the documents that carried it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct NewsStoryDetail {
    pub story: NewsStory,
    /// **Oldest first**, so the list reads as provenance: who had it first, and who repeated it.
    /// On this route `story.source_count` always equals `articles.len()`, because both are derived
    /// from the same rows.
    pub articles: Vec<NewsArticle>,
}

/// One page of the list: the rows **and** the pagination block.
///
/// The read does not walk pages. `paged()` answers "the whole fleet" one page at a time at the
/// server's maximum page size, which is the register's question; the newsroom asks the operator's
/// one — page `n` of 25 stories — and a client that walked every page would make page 2
/// unreachable. `total_records` is still carried, so the panel can say how much it is not showing.
#[derive(Debug, Clone, PartialEq)]
pub struct StoryPage {
    pub rows: Vec<NewsStory>,
    pub pagination: Pagination,
}
