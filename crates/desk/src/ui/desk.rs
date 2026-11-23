//! **The desk's working surface** — the vessel list, the batch tray, and where the operator is in them.
//!
//! ADR-0009's decision 2 is the shape: one row per vessel, and a batch is those rows, one request each.
//! Nothing here is a second store. The standing picture is [`Picker`], the draft is [`Batch`], and this
//! type holds only the two things a screen adds — which hull is selected, and where the keyboard is.
//!
//! **No clock and no arithmetic.** An age is the age Minos counted, carried on
//! [`LastKnown`](crate::picker::LastKnown) and put into words by it; a row's regime is
//! [`crate::time::verdict`]'s answer, asked in the same place the field asks. The only counting below is
//! how many rows are on the screen for the chips and the footer, which is not a claim about when anything
//! happened.
//!
//! **A hull is never hidden because the desk guessed it cannot report.** No reportable-set filter exists
//! here, because Minos publishes none for a shore account (decision 4). The chips filter what the operator
//! asked to see; they never filter what the operator may report.

use arcons_cc::ui::tokens;
use rune::{El, col, el, row, spacer, text};

use crate::batch::{Batch, Row};
use crate::field::zoned;
use crate::picker::{Picker, Vessel};
use crate::time::verdict;
use crate::ui::{State, WIB_OFFSET_MS, caption};

/// The four chips above the list. `All` is the picture; the other three narrow it and never add to it.
///
/// A never-reported hull is **not stale** — [`Vessel::is_stale`] says so, because silence has no age to
/// have crossed a line — so `Stale` and `Never` are two different questions and the list answers both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Stale,
    Never,
    InBatch,
}

impl Filter {
    /// Every chip, in the wireframe's order.
    pub const ALL: [Self; 4] = [Self::All, Self::Stale, Self::Never, Self::InBatch];

    /// The word on the chip and in the filter's accessible name.
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Stale => "Stale",
            Self::Never => "Never",
            Self::InBatch => "In batch",
        }
    }

    /// The key the chip is drawn with, so a capture and a test name the same control.
    pub fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Stale => "stale",
            Self::Never => "never",
            Self::InBatch => "in-batch",
        }
    }

    /// Whether this hull is in the filter. The batch is the other argument because "in batch" is a fact
    /// about the draft, not about the picture.
    pub fn matches(self, vessel: &Vessel, batch: &Batch) -> bool {
        match self {
            Self::All => true,
            // The wall's one threshold, not a second opinion: `units::STALE_AFTER` is 120 s.
            Self::Stale => vessel.is_stale(arcons_cc::units::STALE_AFTER.as_secs_f64()),
            Self::Never => vessel.last.is_none(),
            Self::InBatch => batch.get(vessel.id_unit).is_some(),
        }
    }

    /// How many hulls the chip counts, over the whole picture rather than the current query.
    ///
    /// A chip that counted only what the query left would say "All 3" while the standing picture held a
    /// hundred, and the operator would read the filter as the fleet.
    pub fn count(self, counts: Counts) -> usize {
        match self {
            Self::All => counts.all,
            Self::Stale => counts.stale,
            Self::Never => counts.never,
            Self::InBatch => counts.in_batch,
        }
    }
}

/// The picture counted four ways, for the chips and the rail's footer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub stale: usize,
    pub never: usize,
    pub in_batch: usize,
}

/// The coordinates the selected hull's row would carry: the batch's row when there is one, else what the
/// picture last knew, else nothing.
///
/// It is a value rather than three options on [`Desk`] because the inspector asks one question — "what
/// would be sent for this hull" — and the answer has to come from one place.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Draft {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub speed_kn: Option<f64>,
    pub course_deg: Option<f64>,
    pub accuracy_m: Option<f64>,
}

/// **The screen's state, not its drawing**: the picture, the draft, and the operator's place in them.
pub struct Desk {
    picker: Picker,
    batch: Batch,
    selected: Option<u64>,
    search: rune::TextEdit,
    filter: Filter,
    /// The row the keyboard is on, as an index into [`Desk::visible`]. `None` is no list focus.
    cursor: Option<usize>,
    /// The tray's last answer. The send says one thing this ticket can honestly say: #278 brings it.
    notice: Option<String>,
}

impl Default for Desk {
    fn default() -> Self {
        Self::new(Picker::new(Vec::new()))
    }
}

impl Desk {
    /// A desk looking at `picker`, with an empty draft and nothing selected.
    pub fn new(picker: Picker) -> Self {
        Self {
            picker,
            batch: Batch::new(),
            selected: None,
            search: rune::TextEdit::default(),
            filter: Filter::All,
            cursor: None,
            notice: None,
        }
    }

    // -----------------------------------------------------------------------
    // The picture
    // -----------------------------------------------------------------------

    /// The standing picture, read once and searched locally; no request per keystroke.
    pub fn picker(&self) -> &Picker {
        &self.picker
    }

    pub fn vessel(&self, id_unit: u64) -> Option<&Vessel> {
        self.picker.get(id_unit)
    }

    pub fn search(&self) -> &rune::TextEdit {
        &self.search
    }

    pub fn search_mut(&mut self) -> &mut rune::TextEdit {
        &mut self.search
    }

    /// The query as typed. Rune's field owns the text; the picker only reads it.
    pub fn query_text(&self) -> &str {
        &self.search.text
    }

    /// Put a query in the field without a keyboard, for a scene or a test.
    pub fn set_query(&mut self, query: &str) {
        self.search.text.clear();
        self.search.text.push_str(query);
        let caret = self.search.text.len();
        self.search.anchor = caret;
        self.search.focus = caret;
        self.reposition();
    }

    /// Escape: forget the query and put the whole picture back. `false` when there was nothing to clear,
    /// so the form's Escape can revert the time instead of doing nothing at all.
    pub fn clear_query(&mut self) -> bool {
        if self.search.text.is_empty() {
            return false;
        }
        self.search.text.clear();
        self.search.anchor = 0;
        self.search.focus = 0;
        self.reposition();
        true
    }

    pub fn filter(&self) -> Filter {
        self.filter
    }

    /// Choose a chip. The keyboard's place follows the selection when it is still on the screen; a filter
    /// that moved the cursor to a hull the operator did not point at would select by accident.
    pub fn set_filter(&mut self, filter: Filter) {
        self.filter = filter;
        self.reposition();
    }

    /// The hulls the rail shows: the query's answer, narrowed by the chip. **Order is the picker's**: the
    /// standing picture's age order when the query is empty, and the ranking's when it is not.
    pub fn visible(&self) -> Vec<&Vessel> {
        self.picker
            .query(self.query_text())
            .into_iter()
            .map(|hit| hit.vessel)
            .filter(|vessel| self.filter.matches(vessel, &self.batch))
            .collect()
    }

    pub fn list_is_empty(&self) -> bool {
        self.visible().is_empty()
    }

    /// The whole picture counted for the chips. The query is deliberately not applied.
    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        for vessel in self.picker.vessels() {
            counts.all += 1;
            if vessel.is_stale(arcons_cc::units::STALE_AFTER.as_secs_f64()) {
                counts.stale += 1;
            }
            if vessel.last.is_none() {
                counts.never += 1;
            }
            if self.batch.get(vessel.id_unit).is_some() {
                counts.in_batch += 1;
            }
        }
        counts
    }

    // -----------------------------------------------------------------------
    // Selection and the keyboard
    // -----------------------------------------------------------------------

    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    pub fn selected_vessel(&self) -> Option<&Vessel> {
        self.selected.and_then(|id_unit| self.picker.get(id_unit))
    }

    /// Select a hull that the picture holds. The field's prefill is the screen's job (it owns the time),
    /// so this only moves the selection and the keyboard's place.
    pub fn select(&mut self, id_unit: u64) {
        if self.picker.get(id_unit).is_none() {
            return;
        }
        self.selected = Some(id_unit);
        self.cursor = self
            .visible()
            .iter()
            .position(|vessel| vessel.id_unit == id_unit);
    }

    /// Enter: select the row the keyboard is on. `None` when the keyboard is not in the list.
    pub fn select_cursor(&mut self) -> Option<u64> {
        let id_unit = self.cursor_vessel()?.id_unit;
        self.select(id_unit);
        Some(id_unit)
    }

    /// The row the keyboard is on, clamped to what the list currently shows.
    pub fn cursor_vessel(&self) -> Option<&Vessel> {
        let cursor = self.cursor?;
        self.visible().into_iter().nth(cursor)
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    /// Tab (`delta` +1) or Shift-Tab (-1): walk the list and wrap at its ends.
    ///
    /// Wrapping is deliberate: a cursor that stops at the last row and cannot reach the first is a list
    /// the keyboard cannot get out of the way it came in.
    pub fn tab(&mut self, delta: i32) {
        let len = self.visible().len();
        if len == 0 {
            self.cursor = None;
            return;
        }
        let current = self.cursor.unwrap_or(0).min(len - 1) as i32;
        self.cursor = Some((current + delta).rem_euclid(len as i32) as usize);
    }

    /// Put the keyboard back on the selection when the list changed under it, or nowhere when the
    /// selection is filtered out.
    pub fn reposition(&mut self) {
        if let Some(id_unit) = self.selected {
            self.cursor = self
                .visible()
                .iter()
                .position(|vessel| vessel.id_unit == id_unit);
        }
    }

    // -----------------------------------------------------------------------
    // The batch
    // -----------------------------------------------------------------------

    pub fn batch(&self) -> &Batch {
        &self.batch
    }

    pub fn in_batch(&self, id_unit: u64) -> bool {
        self.batch.get(id_unit).is_some()
    }

    /// The badge's number: one row, or none. The model guarantees it cannot be two.
    pub fn batch_count(&self, id_unit: u64) -> usize {
        self.batch.count(id_unit)
    }

    /// Where the hull sits in the tray, for "1 of 2 vessels". The tray is built in insertion order.
    pub fn batch_position(&self, id_unit: u64) -> Option<usize> {
        self.batch
            .rows()
            .iter()
            .position(|row| row.id_unit == id_unit)
    }

    /// Put a row in the batch, replacing the hull's row if it already had one. The batch's one-row rule
    /// is the model's; this is only the screen's door into it.
    pub fn add_row(&mut self, row: Row) {
        self.batch.insert(row);
    }

    /// `x` and the card's remove control: take the hull's row out of the draft.
    pub fn remove(&mut self, id_unit: u64) -> Option<Row> {
        self.batch.remove(id_unit)
    }

    /// Decision 6's note is the batch's, so it lives and dies with the draft.
    pub fn set_source_note(&mut self, note: &str) {
        self.batch.source_note = Some(note.to_string());
    }

    /// What the send control says: the one true thing this ticket's send can say.
    pub fn say_send_arrives(&mut self) {
        self.notice = Some("the send arrives with #278".to_string());
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The instant and offset the selected hull's time should open at: the draft's row when there is one,
    /// else the picture's last known time on the desk's clock.
    pub fn time_of(&self, id_unit: u64) -> Option<(i64, i32)> {
        if let Some(row) = self.batch.get(id_unit) {
            return Some((row.fix.recorded_at_ms, row.fix.offset_ms));
        }
        self.picker
            .get(id_unit)
            .and_then(|vessel| vessel.last.as_ref())
            .map(|last| (last.recorded_at_ms, WIB_OFFSET_MS))
    }

    /// What the inspector's coordinate fields read: the batch's row first, then the picture, then nothing.
    pub fn draft(&self, id_unit: u64) -> Draft {
        if let Some(row) = self.batch.get(id_unit) {
            return Draft {
                latitude: row.fix.latitude,
                longitude: row.fix.longitude,
                speed_kn: row.fix.speed_kn,
                course_deg: row.fix.course_deg,
                accuracy_m: row.fix.accuracy_m,
            };
        }
        let Some(last) = self
            .picker
            .get(id_unit)
            .and_then(|vessel| vessel.last.as_ref())
        else {
            return Draft::default();
        };
        Draft {
            latitude: Some(last.latitude),
            longitude: Some(last.longitude),
            speed_kn: last.speed_kn,
            course_deg: last.course_deg,
            accuracy_m: last.accuracy_m,
        }
    }
}

// ---------------------------------------------------------------------------
// The drawing. Every colour and every size is the wall's (ADR-0009, decision 5).
// ---------------------------------------------------------------------------

/// The left rail: the search, the chips, the list, and what the list is made of.
pub(super) fn rail(state: &State) -> El {
    col()
        .key("vessel-rail")
        .w(tokens::DESK_LIST)
        .h_full()
        .min_h(tokens::DESK_SCROLL_MIN_H)
        .gap(tokens::GAP)
        .child(search_field(state))
        .child(chips(state))
        .child(list(state))
        .child(rail_footer(state))
}

/// The search field is Rune's `TextEdit`, so the caret, the selection and the input method behave the way an
/// operator's keyboard expects. Typing only narrows the copy of the picture already in memory: there is no
/// request here, and a lost connection does not empty the list mid-sentence.
fn search_field(state: &State) -> El {
    rune::text_input(state.desk.search(), tokens::LABEL)
        .key("vessel-search")
        .role("searchbox")
        .label("Search hull or number")
        .single_line()
        .w_full()
        .h(tokens::NAME_H + tokens::GAP)
        .px(tokens::GAP)
        .fill(tokens::bg())
        .radius(tokens::GAP)
        .border(tokens::HAIRLINE, tokens::card_edge())
        .placeholder("Search hull or number", tokens::ink_dim())
        .caret(tokens::accent(), tokens::name_plate())
        .on_edit(|state: &mut State| state.desk.search_mut())
        .on_input_change(|state: &mut State, _tx: &mut rune::Tx| state.desk.reposition())
        // Escape inside the field clears the query and keeps the keyboard where it is. Letting the editor
        // blur instead would make the second Escape mean something else.
        .on_input_key(
            |state: &mut State, event: &rune::KeyEvent, _tx: &mut rune::Tx| {
                event.key == rune::Key::Escape && state.desk.clear_query()
            },
        )
}

/// The chips are a choice, not a switch: one is on, the others are not, and `role("radio")` says so to a
/// reader who cannot see the filled pill.
fn chips(state: &State) -> El {
    let counts = state.desk.counts();
    row()
        .key("filter-chips")
        .role("radiogroup")
        .label("Filter vessels")
        .items_center()
        .gap(tokens::CHIP_GAP)
        .children(Filter::ALL.into_iter().map(move |filter| {
            let active = state.desk.filter() == filter;
            row()
                .key(format!("filter-{}", filter.key()))
                .h(tokens::FILTER_ROW)
                .px(tokens::CHIP_PAD)
                .items_center()
                .gap(tokens::CHIP_GAP)
                .radius(tokens::FILTER_ROW / 2.0)
                .fill(if active {
                    tokens::name_plate()
                } else {
                    tokens::clear()
                })
                .border(
                    tokens::HAIRLINE,
                    if active {
                        tokens::focus_edge()
                    } else {
                        tokens::hairline()
                    },
                )
                .role("radio")
                .checked(active)
                .label(format!("{} {}", filter.name(), filter.count(counts)))
                .on_press(move |state: &mut State| state.desk.set_filter(filter))
                .child(text(filter.name()).font(tokens::LABEL).color(if active {
                    tokens::ink()
                } else {
                    tokens::ink_dim()
                }))
                .child(
                    text(filter.count(counts).to_string())
                        .font(tokens::LABEL)
                        .color(tokens::ink_dim()),
                )
        }))
}

/// The list: one scroll viewport, so the rail never hand-rolls an offset and a wheel works the way it does
/// everywhere else.
fn list(state: &State) -> El {
    let visible = state.desk.visible();
    let len = visible.len();
    let mut list = col()
        .key("vessel-list")
        .role("listbox")
        .label("Vessels")
        .w_full()
        .grow(1.0)
        // A scroll viewport needs a bounded height in every flex ancestor or it grows to its content and
        // never scrolls (`El::scroll`'s own rule, the same one the filter panel keeps).
        .min_h(tokens::DESK_SCROLL_MIN_H)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .gap(tokens::HAIRLINE)
        // **The list owns Tab when it has focus.** Rune offers a focused element's keys to its ancestors
        // first, so a row's Tab reaches here; the same function is the window's fallback when nothing is
        // focused, which is what makes the keyboard rule one rule.
        .focusable(true)
        .on_key(super::list_key);
    if visible.is_empty() {
        list = list.child(
            text(if state.desk.picker().is_empty() {
                "The standing picture is empty."
            } else {
                "No vessel matches."
            })
            .key("vessel-list-empty")
            .font(tokens::LABEL)
            .color(tokens::ink_dim()),
        );
    }
    for (index, vessel) in visible.into_iter().enumerate() {
        list = list.child(vessel_row(state, vessel, index, len));
    }
    list
}

/// One row: the name, the hull, and the age with its marks. The label is one sentence — name, hull number,
/// age — because a reader who hears the row as three separate nodes hears a list of nouns.
///
/// Selection is a filled row with a bright edge; the keyboard's place is a duller edge. They are different
/// facts: Tab walks, Enter selects, and a screen that drew them the same would make Enter look like it did
/// nothing.
fn vessel_row(state: &State, vessel: &Vessel, index: usize, len: usize) -> El {
    let id_unit = vessel.id_unit;
    let selected = state.desk.selected() == Some(id_unit);
    let cursor = state.desk.cursor() == Some(index);
    let backfilled = vessel.last.as_ref().is_some_and(|last| last.backfilled);
    let age = vessel
        .last
        .as_ref()
        .map_or_else(|| "never".to_string(), |last| last.age_words());
    let count = state.desk.batch_count(id_unit);

    let mut label = format!("{}, hull {}, {}", vessel.name, vessel.hull_number, age);
    if backfilled {
        label.push_str(", backfilled");
    }
    if count > 0 {
        label.push_str(", in the batch");
    }

    let edge = if selected {
        tokens::focus_edge()
    } else if cursor {
        tokens::ink_dim()
    } else {
        tokens::hairline()
    };

    let mut marks = row()
        .items_center()
        .gap(tokens::CHIP_GAP)
        .child(text(age).font(tokens::LABEL).color(tokens::ink_dim()));
    if backfilled {
        marks = marks.child(mark(
            format!("bf-{id_unit}"),
            "bf",
            "backfilled",
            tokens::ink_dim(),
        ));
    }
    if count > 0 {
        marks = marks.child(mark(
            format!("batch-badge-{id_unit}"),
            &format!("{count}"),
            "in the batch",
            tokens::ink(),
        ));
    }

    row()
        .key(format!("vessel-row-{id_unit}"))
        .role("option")
        .checked(selected)
        .position_in_set(index + 1, len)
        .label(label)
        .w_full()
        .h(tokens::DESK_ROW_H)
        .px(tokens::CHIP_PAD)
        .items_center()
        .gap(tokens::GAP)
        .fill(if selected {
            tokens::name_plate()
        } else {
            tokens::clear()
        })
        .border(tokens::HAIRLINE, edge)
        .on_press(move |state: &mut State| {
            state.focus = crate::ui::Focus::List;
            state.select(id_unit);
        })
        .child(
            col()
                .grow(1.0)
                .gap(tokens::CARD_GAP)
                .child(
                    text(vessel.name.clone())
                        .font(tokens::TITLE)
                        .color(tokens::ink()),
                )
                .child(
                    // The hull number is the operator's strongest handle on a ship. The class is not in the
                    // picture the desk reads, so the row does not pretend to have one.
                    text(format!("Hull {}", vessel.hull_number))
                        .font(tokens::LABEL)
                        .color(tokens::ink_dim()),
                ),
        )
        .child(marks)
}

/// A small word in a pill: `bf`, or the batch's count. The mark is drawn and also said, so a reader who
/// cannot see the pill still knows the hull is backfilled or in the draft.
fn mark(key: String, words: &str, said: &str, ink: rune::Color) -> El {
    el().key(key)
        .role("note")
        .label(said)
        .h(tokens::CHIP_H)
        .px(tokens::CHIP_PAD / 2.0)
        .radius(tokens::CHIP_H / 2.0)
        .items_center()
        .justify_center()
        .fill(tokens::name_plate())
        .child(text(words.to_string()).font(tokens::LABEL).color(ink))
}

/// What the list is made of, under the list: how much of the picture is showing, and the one sentence this
/// ticket can give the operator about a hull with no position.
fn rail_footer(state: &State) -> El {
    let counts = state.desk.counts();
    let shown = state.desk.visible().len();
    col()
        .key("rail-foot")
        .w_full()
        .gap(tokens::CARD_GAP)
        .child(caption(format!(
            "{shown} of {} shown · ages from Minos",
            counts.all
        )))
        .child(caption("Select a vessel, then confirm the fix."))
        .child(caption("never reported · nothing to prefill"))
}

/// The batch tray: the draft, and the one request each row will become.
pub(super) fn tray(state: &State) -> El {
    let mut cards = row()
        .key("tray-cards")
        .w_full()
        .items_start()
        .gap(tokens::GAP);
    for row in state.desk.batch().rows() {
        cards = cards.child(card(state, row));
    }
    if state
        .desk
        .selected()
        .is_some_and(|id_unit| !state.desk.in_batch(id_unit))
    {
        cards = cards.child(add_vessel());
    }

    let mut header = row()
        .w_full()
        .items_center()
        .gap(tokens::GAP)
        .child(caption("THE BATCH"));
    if let Some(notice) = state.desk.notice() {
        header = header.child(spacer()).child(caption(notice.to_string()));
    }

    col()
        .key("batch-tray")
        .w_full()
        .gap(tokens::GAP)
        .padding(tokens::PAD)
        .radius(tokens::PANEL_RADIUS)
        .fill(tokens::panel())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .child(header)
        .child(cards)
        .child(footer(state))
}

/// One vessel in the draft: the ship, the hull, the regime the row's time earns, and a way to drop it.
fn card(state: &State, held: &Row) -> El {
    let id_unit = held.id_unit;
    let hull = state
        .desk
        .vessel(id_unit)
        .map_or_else(|| "—".to_string(), |vessel| vessel.hull_number.clone());
    let regime = verdict(held.fix.recorded_at_ms, Some(state.server_now_ms)).regime;
    let civil = zoned::at_offset(held.fix.recorded_at_ms, held.fix.offset_ms);

    col()
        .key(format!("tray-card-{id_unit}"))
        .role("group")
        .label(format!(
            "{}, hull {}, {}",
            held.name,
            hull,
            regime.sentence()
        ))
        .w(tokens::DESK_CARD_W)
        .h(tokens::DESK_CARD_H)
        .padding(tokens::CHIP_PAD)
        .gap(tokens::CARD_GAP)
        .radius(tokens::GAP)
        .fill(tokens::bg())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .child(
            row()
                .w_full()
                .items_center()
                .gap(tokens::GAP)
                .child(
                    text(held.name.clone())
                        .font(tokens::TITLE)
                        .color(tokens::ink()),
                )
                .child(spacer())
                .child(remove_button(id_unit, held.name.clone())),
        )
        .child(
            text(format!(
                "Hull {hull} · {} · {:02}:{:02}:{:02} WIB",
                regime.word(),
                civil.hour,
                civil.minute,
                civil.second
            ))
            .font(tokens::LABEL)
            .color(tokens::ink_dim()),
        )
}

/// The card's remove control. The glyph is the wall's dismissal mark; the words are in its name, because a
/// glyph nobody can name is a button nobody presses.
fn remove_button(id_unit: u64, name: String) -> El {
    el().key(format!("remove-{id_unit}"))
        .role("button")
        .label(format!("Remove {name} from the batch"))
        .size(tokens::PANE_BUTTON, tokens::PANE_BUTTON)
        .radius(tokens::GAP)
        .items_center()
        .justify_center()
        .hover(tokens::name_plate())
        .on_press(move |state: &mut State| {
            state.desk.remove(id_unit);
        })
        .child(text("×").font(tokens::TITLE).color(tokens::ink_dim()))
}

/// `+ Add vessel`, in the tray, only when the selected hull is not already in it. The same action as the
/// inspector's button, because there is one draft and one way into it.
fn add_vessel() -> El {
    el().key("add-vessel")
        .role("button")
        .label("Add the selected vessel to the batch")
        .w(tokens::DESK_CARD_W)
        .h(tokens::DESK_CARD_H)
        .items_center()
        .justify_center()
        .radius(tokens::GAP)
        .fill(tokens::clear())
        .border(tokens::HAIRLINE, tokens::focus_edge())
        .border_dash(tokens::CHIP_PAD, tokens::CARD_GAP)
        .on_press(|state: &mut State| state.add_selected())
        .child(
            text("+ Add vessel")
                .font(tokens::LABEL)
                .color(tokens::ink()),
        )
}

/// The footer: how many vessels, how many fixes, how many requests, and the primary control.
///
/// **One number.** `vessels` is the batch's length, and one row is one vessel and one request, so the
/// footer and the button read the same value rather than three counts that could drift apart. After a send the
/// left-hand sentence is **each vessel's own answer** (`State::send_line`), because a batch of three that recorded
/// two and refused one has no single word that is true of it.
fn footer(state: &State) -> El {
    let vessels = state.desk.batch().len();
    let words = state.send_line().unwrap_or_else(|| {
        format!(
            "{} {} · {} {} · nothing sent",
            vessels,
            plural(vessels, "vessel", "vessels"),
            vessels,
            plural(vessels, "fix", "fixes")
        )
    });
    let send = if state.sending() {
        "Sending…".to_string()
    } else {
        format!("Send {vessels} {}", plural(vessels, "report", "reports"))
    };
    let enabled = vessels > 0 && !state.sending();
    // "One request each" is a claim about the wire, and with nothing in the draft there is no each.
    let mut row = el()
        .key("tray-footer")
        .w_full()
        .items_center()
        .gap(tokens::GAP)
        .child(caption(words).key("tray-foot"));
    if vessels > 0 && !state.sending() && state.sent.is_none() {
        row = row.child(caption("one request each"));
    }
    row.child(spacer()).child(
        el().key("send-reports")
            .role("button")
            .label(send.clone())
            .disabled(!enabled)
            .px(tokens::PAD)
            .py(tokens::CHIP_PAD)
            .radius(tokens::GAP)
            .fill(if enabled {
                tokens::accent()
            } else {
                tokens::name_plate()
            })
            .items_center()
            .justify_center()
            .on_press(|state: &mut State| state.send())
            .child(text(send).font(tokens::LABEL).color(if enabled {
                tokens::bg()
            } else {
                tokens::ink_dim()
            })),
    )
}

/// `1 vessel`, `2 vessels`: a footer that said "1 vessels" is a footer nobody proof-read.
fn plural<'a>(count: usize, one: &'a str, many: &'a str) -> &'a str {
    if count == 1 { one } else { many }
}
