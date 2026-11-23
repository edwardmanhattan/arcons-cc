//! **The desk's screen** — the vessel list, the batch tray, and the time field (ADR-0009, decisions 2–5).
//!
//! The rules are not here. A regime is [`crate::time::verdict`]'s answer, a calendar is
//! [`crate::field::calendar`]'s grid, a typed time is [`crate::field::segment`]'s, a pasted one is
//! [`crate::field::paste`]'s, and a wall clock becomes an instant through [`crate::field::zoned`]. The
//! picture, the batch and the operator's place in them are [`desk::Desk`]'s. This module composes them
//! into elements and nothing else: no boundary arithmetic, no calendar arithmetic and no list filtering,
//! so a colour or a rule that changes has one place to change.
//!
//! **Nothing here reads a clock or does I/O.** `State` carries the server's clock and "today" as
//! values a scene fixes, which is what makes `--snap` byte-identical between runs (`INV-11`'s
//! rule, applied to the desk). The real clock and the OS zone arrive with the client (#278);
//! until then the field's zone is WIB and its "now" is the sample the scene supplies.
//!
//! **The field owns the keyboard.** Rune routes a key to the focused control first, so a button
//! that kept focus would take Tab, Return and Space away from the segments; every press here calls
//! [`rune::Tx::blur`] after its action, handing the keyboard back to the form. The buttons are
//! still exposed to assistive technology as buttons, by role and by name.

use arcons_cc::ui::tokens;
use rune::{Cx, El, col, el, row, spacer, text};

use arcons_cc::map::geometry::Camera;

use crate::batch::{Fix, Readiness, Row};
use crate::chart::Chart;
use crate::field::calendar::Month;
use crate::field::segment::{Field, Segments};
use crate::field::zoned::{self, Resolution};
use crate::field::{CivilTime, FieldError, civil, paste};
use crate::picker::Vessel;
use crate::time::{self, Regime, Verdict};

pub mod chart;
pub mod desk;

use desk::Desk;

/// The window title: one window, one application, one name.
pub const TITLE: &str = "Arcons Desk";

/// **The field's zone until the client carries the OS zone**: WIB, the clock the operator reads
/// (ADR-0009, decision 3). It is a time value, not a size, so it lives here rather than in the
/// wall's tokens.
pub const WIB_OFFSET_MS: i32 = 7 * 60 * 60 * 1000;

/// The hedge the field draws when the verdict is close to a line it could cross while it is read.
const HEDGE: &str = "near a line — this can change before it is sent";

/// `CivilTime` from literal fields, for the sample and its tests. Not arithmetic: a constructor.
fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> CivilTime {
    CivilTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
    }
}

/// Everything the screen draws from. A scene fixes it; the view only reads it.
pub struct State {
    /// The six segments as the operator has typed them.
    pub segments: Segments,
    /// The last time Enter (or the batch button) accepted, and the instant it names.
    pub committed: CivilTime,
    pub epoch_ms: i64,
    pub verdict: Verdict,
    /// The server's clock, fixed by the caller. `None` would be no verdict, never "now".
    pub server_now_ms: i64,
    /// The day a bare pasted `14:03` belongs to, fixed by the caller.
    pub today: CivilTime,
    /// The month the calendar is showing, which the arrows can move away from the committed one.
    pub month: Month,
    pub calendar_open: bool,
    /// A refused commit, shown on the field with the segment it names.
    pub error: Option<FieldError>,
    /// A paste's refusal or acceptance, in the field's own words.
    pub notice: Option<String>,
    /// The picture, the batch, the selection and the list's keyboard (#277).
    pub desk: Desk,
    /// Which surface owns Tab, Enter, Up/Down and Escape, while both want them.
    pub focus: Focus,
    /// The chart's camera and the rectangle it projects through (ADR-0009, decision 4).
    pub chart: Chart,
    /// The camera a pan started on, so the preview is absolute rather than a sum of deltas.
    pub chart_origin: Option<Camera>,
    /// The keyboard's placement: set by `Place`, moved by the arrows, closed by Enter or Escape.
    pub arming: bool,
    /// Where that crosshair is, in chart points.
    pub crosshair: (f64, f64),
}

impl State {
    /// **The wireframe's sample.** 14:03:22 WIB read on a clock the server's is three seconds
    /// behind: 07:03:22Z, three seconds ahead of the server, which is inside the five-second
    /// tolerance and therefore live — and near the future line, so the hedge shows.
    ///
    /// Every value is a value, not a reading: the capture is the same picture on every machine.
    pub fn sample() -> Self {
        let recorded = at(2026, 11, 20, 14, 3, 22);
        let epoch_ms = zoned::epoch_ms_of(recorded, WIB_OFFSET_MS);
        let server_now_ms = epoch_ms - 3_000;
        Self {
            segments: Segments::new(recorded),
            committed: recorded,
            epoch_ms,
            verdict: time::verdict(epoch_ms, Some(server_now_ms)),
            server_now_ms,
            today: recorded,
            month: Month::of_days(civil::days_from_civil(2026, 11, 20)),
            calendar_open: true,
            focus: Focus::Field,
            chart: Chart::new((-6.10, 106.88), 8.5, (720.0, 520.0)),
            chart_origin: None,
            arming: false,
            crosshair: (360.0, 260.0),
            error: None,
            notice: None,
            desk: crate::scene::sample_desk(epoch_ms),
        }
    }

    /// The day the calendar highlights: the date the field now reads, when the calendar is
    /// showing that month. A day is not selected in a month it does not belong to.
    pub fn selected_day(&self) -> Option<u32> {
        let civil = self.segments.commit().unwrap_or(self.committed);
        (civil.year == self.month.year() && civil.month == self.month.month()).then_some(civil.day)
    }

    /// **A press on the chart.** The point under the pointer becomes the selected hull's claim, with the
    /// field's time: a drop is an observation, never a guess at the future and never a send.
    pub fn place_at(&mut self, x: f64, y: f64) {
        let Some(id_unit) = self.desk.selected() else {
            self.notice = Some("select a vessel before placing it".to_string());
            return;
        };
        let (lat, lon) = self.chart.place_at(x, y);
        self.place(id_unit, lat, lon);
    }

    /// Put a hull at a coordinate, in the draft, at the time the field holds.
    ///
    /// The batch keeps one row per vessel, so placing twice replaces the row rather than adding a second:
    /// the second press is a correction, which is what a second fix for one hull always is.
    pub fn place(&mut self, id_unit: u64, lat: f64, lon: f64) {
        let Some(name) = self.desk.vessel(id_unit).map(|vessel| vessel.name.clone()) else {
            return;
        };
        let draft = self.desk.draft(id_unit);
        let row = Row {
            id_unit,
            name,
            fix: Fix {
                recorded_at_ms: self.epoch_ms,
                offset_ms: WIB_OFFSET_MS,
                latitude: Some(lat),
                longitude: Some(lon),
                speed_kn: draft.speed_kn,
                course_deg: draft.course_deg,
                accuracy_m: draft.accuracy_m,
            },
        };
        self.desk.add_row(row);
        self.notice = Some(format!("placed at {lat:.5}, {lon:.5} — not sent"));
    }

    /// Arm the keyboard's placement: a crosshair in the middle of the chart, moved by the arrows.
    pub fn arm(&mut self) {
        let viewport = self.chart.viewport();
        self.crosshair = (viewport.w / 2.0, viewport.h / 2.0);
        self.arming = true;
        self.focus = Focus::Chart;
        self.notice = Some("arrows move the crosshair, Enter places, Escape cancels".to_string());
    }

    pub fn disarm(&mut self) {
        self.arming = false;
        self.focus = Focus::Field;
    }

    /// Move the crosshair, staying on the chart: a placement off the water is not a placement.
    pub fn nudge(&mut self, dx: f64, dy: f64) {
        let viewport = self.chart.viewport();
        self.crosshair.0 = (self.crosshair.0 + dx).clamp(0.0, viewport.w);
        self.crosshair.1 = (self.crosshair.1 + dy).clamp(0.0, viewport.h);
    }

    /// Enter while armed: the crosshair becomes the claim.
    pub fn commit_placement(&mut self) {
        let (x, y) = self.crosshair;
        self.place_at(x, y);
        self.disarm();
    }

    /// A digit from the keyboard or a script.
    pub fn type_digit(&mut self, digit: u32) {
        self.segments.push_digit(digit);
        self.refresh();
    }

    /// Up or Down: step the active segment, wrapping inside it.
    pub fn step(&mut self, delta: i32) {
        self.segments.step(delta);
        self.refresh();
    }

    pub fn left(&mut self) {
        self.segments.left();
    }

    pub fn right(&mut self) {
        self.segments.right();
    }

    pub fn backspace(&mut self) {
        self.segments.backspace();
        self.refresh();
    }

    /// A press outside the calendar: close it and change nothing else.
    ///
    /// The catcher and Escape both come here, so dismissing a surface cannot be a second path that also
    /// commits a half-typed value. Escape additionally reverts the segments (see [`State::revert`]),
    /// which is a different promise: "forget what I typed", not "stop covering the field".
    pub fn dismiss_calendar(&mut self) {
        self.calendar_open = false;
    }

    /// Clear every segment. The banner stays the last committed value's until something valid is
    /// typed, because an empty field is not a time to have a verdict about.
    pub fn clear(&mut self) {
        self.segments.clear();
        self.error = None;
    }

    /// Escape: put the last accepted time back, and close the calendar.
    pub fn revert(&mut self) {
        self.segments = Segments::new(self.committed);
        self.error = None;
        self.notice = None;
        self.calendar_open = false;
        self.refresh();
    }

    /// Enter: the one place a `FieldError` is shown, and the one place a typed time is accepted.
    /// A refusal names the segment to change.
    pub fn enter(&mut self) {
        match self.segments.commit() {
            Ok(civil) => {
                self.committed = civil;
                self.preview(civil);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    /// **Select a hull, and open its row.** The field is prefilled from the batch's row when the hull has
    /// one, else from the picture's last known position, else left at the time the operator already had —
    /// a hull that never reported has no time to offer, and the field is still the operator's clock reading.
    ///
    /// Nothing here asks the network: the picture is already in memory, which is why a lost connection does
    /// not empty the screen the operator is writing into.
    pub fn select(&mut self, id_unit: u64) {
        let Some(time) = self.desk.time_of(id_unit) else {
            // A hull with no last known and no row: select it, keep the time on the field.
            self.desk.select(id_unit);
            return;
        };
        self.desk.select(id_unit);
        let civil = zoned::at_offset(time.0, time.1);
        self.segments = Segments::new(civil);
        self.committed = civil;
        self.month = Month::of_days(civil::days_from_civil(civil.year, civil.month, civil.day));
        self.error = None;
        self.notice = None;
        self.epoch_ms = time.0;
        self.verdict = time::verdict(time.0, Some(self.server_now_ms));
    }

    /// Enter in the list: select the row the keyboard is on. Returns the hull it selected, or `None` when
    /// the keyboard is not in the list.
    pub fn select_cursor(&mut self) -> Option<u64> {
        let id_unit = self.desk.cursor_vessel()?.id_unit;
        self.select(id_unit);
        Some(id_unit)
    }

    /// **Put the selected hull's draft in the batch** — the inspector's button and the tray's `+ Add
    /// vessel` are the same action. The row is built from the draft (the batch's row when there is one,
    /// else the picture) and the field's accepted time, which is what the operator is looking at.
    ///
    /// A row with no coordinates is still added: the ADR keeps a row that cannot be sent on the board with
    /// its reason, because a `403` or a missing position is a fact about the vessel, not a reason to hide it.
    pub fn add_selected(&mut self) {
        let Some(id_unit) = self.desk.selected() else {
            return;
        };
        let Some(name) = self.desk.vessel(id_unit).map(|vessel| vessel.name.clone()) else {
            return;
        };
        let Some(fix) = self.draft_fix(id_unit) else {
            return;
        };
        self.desk.add_row(Row { id_unit, name, fix });
    }

    /// `x` and a card's remove control: take the hull's row out of the draft. The selection stays, so the
    /// operator can look at the card's removal without also losing the hull they were reading.
    pub fn remove_from_batch(&mut self, id_unit: u64) {
        self.desk.remove(id_unit);
    }

    /// Escape: clear the search first, and only then revert the field. A query is the thing the operator
    /// just typed; the time is the thing they accepted, and it should not be thrown away by the key that
    /// was aimed at the query.
    pub fn clear_search(&mut self) -> bool {
        self.desk.clear_query()
    }

    /// The fix the selected hull's row would carry right now: the draft's coordinates, the field's
    /// accepted instant, and WIB as the offset the time was read at.
    pub fn draft_fix(&self, id_unit: u64) -> Option<Fix> {
        self.desk.vessel(id_unit)?;
        let draft = self.desk.draft(id_unit);
        Some(Fix {
            recorded_at_ms: self.epoch_ms,
            offset_ms: WIB_OFFSET_MS,
            latitude: draft.latitude,
            longitude: draft.longitude,
            speed_kn: draft.speed_kn,
            course_deg: draft.course_deg,
            accuracy_m: draft.accuracy_m,
        })
    }

    /// What would happen if the selected hull's draft were sent now, through the batch model's own rule.
    ///
    /// The inspector says this in words; the tray's plan says it per row. Both ask [`Row::readiness`], so
    /// the empty state and the send cannot disagree about whether a row is sendable.
    pub fn readiness(&self) -> Option<Readiness> {
        let id_unit = self.desk.selected()?;
        let name = self.desk.vessel(id_unit)?.name.clone();
        let fix = self.draft_fix(id_unit)?;
        Some(Row { id_unit, name, fix }.readiness(Some(self.server_now_ms)))
    }

    /// The calendar's day: it sets the whole date, because a calendar that changed only the day
    /// would leave the operator's month and year behind what they clicked.
    pub fn pick_day(&mut self, day: u32) {
        self.segments.select(Field::Day);
        self.segments.set(Field::Year, self.month.year() as u32);
        self.segments.set(Field::Month, self.month.month());
        self.segments.set(Field::Day, day);
        self.calendar_open = false;
        self.refresh();
    }

    /// The calendar's arrows. The selection follows the committed date, so a month the date is
    /// not in simply highlights nothing.
    pub fn shift_month(&mut self, months: i32) {
        self.month = self.month.shift(months);
    }

    /// Today: the sample's "now", never a reading of a clock.
    pub fn today(&mut self) {
        self.month = Month::of_days(civil::days_from_civil(
            self.today.year,
            self.today.month,
            self.today.day,
        ));
        self.segments = Segments::new(self.today);
        self.refresh();
    }

    /// Paste, through [`crate::field::paste`]. A time that carried another zone is read as that
    /// zone and shown on the field's clock: the operator pasted an instant, and the field's own
    /// zone is WIB until the client can ask the OS.
    pub fn paste(&mut self, text: &str) {
        match paste::parse(text, self.today) {
            Ok(pasted) => {
                let civil = match pasted.offset_ms {
                    Some(offset) if offset != WIB_OFFSET_MS => {
                        zoned::at_offset(zoned::epoch_ms_of(pasted.civil, offset), WIB_OFFSET_MS)
                    }
                    _ => pasted.civil,
                };
                self.segments = Segments::new(civil);
                self.notice = Some(format!("pasted {civil}"));
                self.refresh();
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
    }

    /// Keep the banner and the derived readout in step with what the segments now read, when they
    /// read. **Escape reverts to [`State::committed`]**, so a preview is not an acceptance: only
    /// Enter (or the batch button) moves the accepted value.
    ///
    /// Called after every edit. A half-typed value is not a refusal — the error appears on Enter,
    /// where the operator asked for a verdict — so an invalid state leaves the last preview alone
    /// instead of shouting at every keystroke.
    fn refresh(&mut self) {
        self.error = None;
        if let Ok(civil) = self.segments.commit() {
            self.preview(civil);
        }
    }

    /// Resolve a wall clock through [`zoned`] and ask [`time::verdict`] what Minos will do with
    /// the instant. A constant offset cannot gap or fold, so the other resolutions are unreachable
    /// in this build and are still handled rather than asserted away.
    fn preview(&mut self, civil: CivilTime) {
        match zoned::resolve(civil, |_| WIB_OFFSET_MS) {
            Resolution::Unique { epoch_ms, .. } => {
                self.epoch_ms = epoch_ms;
                self.verdict = time::verdict(epoch_ms, Some(self.server_now_ms));
            }
            Resolution::Gap => {
                self.notice = Some("that wall clock never happened in WIB".to_string());
            }
            Resolution::Ambiguous { .. } => {
                self.notice = Some("that wall clock happened twice in WIB".to_string());
            }
        }
    }
}

/// Open the display and run until it closes. Rune's own flags (`--snap`, `--scene`, `--at`, ...)
/// are handed through untouched, because a capture that swallowed them would succeed at producing
/// nothing — the wall's `run` has the same shape (#9).
pub fn run(passthrough: Vec<String>) -> std::process::ExitCode {
    let spec = rune::AppSpec::<State> {
        min_size: Some(tokens::MIN_WINDOW),
        on_key: Some(on_key),
        scenes: crate::scene::SCENES,
        poll: Some(poll),
        ..rune::AppSpec::new(TITLE, tokens::DESK_WINDOW, State::sample, view)
    };
    let result = rune::try_run_windows_with_args(
        rune::RunOptions::default(),
        passthrough,
        move |_displays| [rune::WindowSpec::from_app("main", rune::App::new(spec))],
    );
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("desk: cannot open a window: {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// **What changed while nobody was pressing anything** (#277).
///
/// Rune runs the view once and then leaves the screen alone until something asks for another frame. On
/// this screen the map's thread is that something: it finishes a frame asynchronously, so the pass that
/// draws it has to be requested. Without this the chart says `the chart is opening` for the life of the
/// window, with a finished picture sitting in the handle it is not drawing.
fn poll(_state: &mut State, _elapsed: f64, _tx: &mut rune::Tx) -> bool {
    #[cfg(feature = "map")]
    {
        crate::chart::engine::frame_arrived()
    }
    #[cfg(not(feature = "map"))]
    {
        false
    }
}

/// **The list's keys, in one function**, reached two ways: the window's global key handler when nothing is
/// focused, and the list's own `on_key` when a row or the list is. Two copies of "Tab walks, Enter selects"
/// would drift the first time one of them changed.
///
/// The list yields the key when it has nothing to do with it, so the form's own keys still work: an empty
/// picture gives Tab back to the six segments, and Escape with no query reverts the time.
/// The list's keys are shared with the list's own `on_key`, so the window's fallback and the focused
/// container cannot disagree about what Tab does.
pub(super) fn list_key(state: &mut State, event: &rune::KeyEvent, _tx: &mut rune::Tx) -> bool {
    match &event.key {
        rune::Key::Tab => {
            if state.desk.list_is_empty() {
                return false;
            }
            state.desk.tab(if event.mods.shift { -1 } else { 1 });
            true
        }
        rune::Key::Enter => {
            if state.desk.cursor().is_none() {
                return false;
            }
            state.select_cursor();
            true
        }
        rune::Key::Text(words) if words.eq_ignore_ascii_case("x") => {
            let Some(id_unit) = state.desk.selected() else {
                return false;
            };
            state.remove_from_batch(id_unit);
            true
        }
        rune::Key::Escape => state.clear_search(),
        _ => false,
    }
}

/// **The keys, in one place.** Digits fill the active segment, Up and Down step it, Left and
/// Right move between segments, Backspace eats a digit, Tab moves on (Shift-Tab back), Enter
/// commits, Escape reverts, and Cmd/Ctrl+V pastes through the model's parser.
///
/// The function a window's key loop calls is the one a test calls: a rule that exists only in an
/// event handler is a rule nobody can test.
/// **Which surface owns the keys that two of them want.**
///
/// The field wants Tab (next segment), Enter (commit), Up/Down (step) and Escape (revert); the vessel
/// list wants Tab (next row), Enter (select), `x` (drop) and Escape (clear the search). Both cannot win,
/// and a rule chosen per key would be four rules. One focus is the smallest thing that is still honest:
/// **Tab leaves the field for the list** from its last segment, Shift-Tab comes back, and a press on a
/// row or a segment moves the focus to the surface that was pressed. Digits always reach the field,
/// because they mean nothing to a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    #[default]
    Field,
    List,
    /// An armed placement: the arrows move the crosshair, Enter places, Escape cancels.
    Chart,
}

pub fn on_key(state: &mut State, event: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
    if event.is_command('v') {
        // The read answers on a later frame, off the UI thread; nothing here waits for it.
        tx.read_clipboard(
            |state: &mut State, content: &rune::ClipboardContent, _tx: &mut rune::Tx| {
                state.paste(content.text());
            },
        );
        return true;
    }
    // **An armed placement owns the arrows**, because moving a crosshair and stepping a segment cannot both
    // be what Up means. Enter places and Escape cancels: the two things a modal placement must answer.
    if state.arming {
        let step = crate::ui::chart::nudge_step(event.mods.shift);
        match &event.key {
            rune::Key::Up => state.nudge(0.0, -step),
            rune::Key::Down => state.nudge(0.0, step),
            rune::Key::Left => state.nudge(-step, 0.0),
            rune::Key::Right => state.nudge(step, 0.0),
            rune::Key::Enter => state.commit_placement(),
            rune::Key::Escape => state.disarm(),
            _ => {}
        }
        return true;
    }
    // Tab is the only key that crosses the boundary, and it crosses in one direction: out of the field
    // from its last segment, and back with Shift.
    if matches!(event.key, rune::Key::Tab) {
        let forward = !event.mods.shift;
        match (state.focus, forward) {
            (Focus::Chart, _) => state.disarm(),
            (Focus::List, true) => state.desk.tab(1),
            // Shift-Tab walks back through the rows and leaves for the field from the first one, so the
            // list is a ring rather than a trap.
            (Focus::List, false) if state.desk.cursor() == Some(0) => state.focus = Focus::Field,
            (Focus::List, false) => state.desk.tab(-1),
            (Focus::Field, true) => state.focus = Focus::List,
            // Inside the field, Shift-Tab walks the six segments backwards; the segments' forward walk is
            // the Right arrow, which never leaves the surface the operator is typing into.
            (Focus::Field, false) => state.left(),
        }
        return true;
    }
    if state.focus == Focus::List && list_key(state, event, tx) {
        return true;
    }
    match &event.key {
        rune::Key::Text(words) => {
            let mut chars = words.chars();
            match (chars.next(), chars.next()) {
                (Some(digit), None) if digit.is_ascii_digit() => {
                    state.type_digit(digit as u32 - u32::from(b'0'));
                    true
                }
                _ => false,
            }
        }
        rune::Key::Up => {
            state.step(1);
            true
        }
        rune::Key::Down => {
            state.step(-1);
            true
        }
        rune::Key::Left => {
            state.left();
            true
        }
        rune::Key::Right => {
            state.right();
            true
        }
        rune::Key::Backspace => {
            state.backspace();
            true
        }
        rune::Key::Tab => {
            // Reached only for a scene with no list to move to: the crossing itself is handled above.
            if event.mods.shift {
                state.left();
            } else {
                state.right();
            }
            true
        }
        rune::Key::Enter => {
            state.enter();
            true
        }
        rune::Key::Escape => {
            state.revert();
            true
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// The drawing. Every colour and every size is the wall's (ADR-0009, decision 5).
// ---------------------------------------------------------------------------

/// **The screen**: the rail, the chart's place and the tray, with the inspector at the wireframe's width
/// and the calendar as an overlay.
///
/// **The chart is a flat panel this commit.** The map is the next commit of #277; drawing one here would
/// make a capture a reviewer reads as a finished picture. The panel says what is missing in one grey line,
/// so the screen cannot be mistaken for a chart whose sea was left blank.
pub fn view(state: &State, _cx: &mut Cx<State>) -> El {
    let body = col()
        .w_full()
        .h_full()
        .gap(tokens::GAP)
        .padding(tokens::PAD)
        .fill(tokens::bg())
        .child(title_bar())
        .child(
            row()
                .w_full()
                .grow(1.0)
                .items_stretch()
                .gap(tokens::GAP)
                .child(desk::rail(state))
                .child(
                    col()
                        .grow(1.0)
                        .h_full()
                        .gap(tokens::GAP)
                        .child(crate::ui::chart::view(state, _cx.env().scale))
                        .child(desk::tray(state)),
                )
                .child(form(state)),
        );
    calendar_overlay(state, body)
}

/// The wireframe's header: the application's name, and the room this desk is in.
fn title_bar() -> El {
    row()
        .key("title-bar")
        .w_full()
        .items_center()
        .gap(tokens::GAP)
        .child(
            text("ARCONS DESK")
                .font(tokens::WORDMARK)
                .color(tokens::ink()),
        )
        .child(spacer())
        .child(caption("OPERATIONS · JAKARTA"))
}

/// **The calendar is a popover, not a column.** It is attached at the root, after its anchor, because a
/// popover laid out as a sibling takes a share of the window instead of covering it — and then the field
/// moves when the calendar opens, which is the layout version of "the answer changed while I was reading
/// the question".
///
/// Rune places it below the segments and flips it above when the window has no room below, the same
/// mechanism the wall's filter panel uses. The catcher is a transparent press over the whole window: a
/// press outside dismisses the calendar and does nothing else, so dismissing a surface never also edits
/// the field underneath it.
fn calendar_overlay(state: &State, body: El) -> El {
    if !state.calendar_open {
        return body;
    }
    let panel = calendar(state).anchored(
        "segments",
        rune::Side::Bottom,
        rune::Align::Start,
        tokens::GAP,
    );
    body.child(
        el().key("calendar-catcher")
            .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
            .on_press(|state: &mut State| state.dismiss_calendar())
            .child(panel),
    )
}

pub(super) fn caption(words: impl Into<String>) -> El {
    text(words).font(tokens::LABEL).color(tokens::ink_dim())
}

pub(super) fn line(words: impl Into<String>) -> El {
    text(words).font(tokens::LABEL).color(tokens::ink())
}

pub(super) fn hairline() -> El {
    el().h(tokens::HAIRLINE).w_full().fill(tokens::hairline())
}

fn separator(mark: &'static str) -> El {
    text(mark).font(tokens::TITLE).color(tokens::ink_dim())
}

/// The inspector: the panel the field has always been, now driven by the selected hull and its draft.
///
/// **What it shows is what would be sent.** The coordinates are the row's (or the picture's, before the row
/// exists), the time is the field's accepted value, and the line under the field is [`Row::readiness`]'s
/// answer in words. A hull with no last known gets the empty state rather than a plausible position: the
/// whole point of this screen is that a guess at a position is worse than a blank.
fn form(state: &State) -> El {
    let selected = state.desk.selected_vessel();
    let name = selected.map_or_else(
        || "No vessel selected".to_string(),
        |vessel| vessel.name.clone(),
    );
    let subtitle = selected.map_or_else(String::new, |vessel| {
        let state_word = if vessel.last.is_none() {
            "never reported"
        } else if state.desk.in_batch(vessel.id_unit) {
            "in the batch"
        } else {
            "prefilled"
        };
        format!("Hull {} · {state_word}", vessel.hull_number)
    });
    let last_known_label = if selected.and_then(|vessel| vessel.last.as_ref()).is_some() {
        "LAST KNOWN"
    } else {
        "LAST KNOWN · NONE"
    };

    let mut form = col()
        .key("inspector")
        .w(tokens::DESK_PANEL)
        .h_full()
        .gap(tokens::GAP)
        .child(caption(report_header(state)).key("report-header"))
        .child(
            text(name)
                .key("report-name")
                .font(tokens::TITLE)
                .color(tokens::ink()),
        )
        .child(caption(subtitle))
        .child(hairline())
        .child(caption(last_known_label).key("last-known-label"))
        .child(last_known(selected))
        .child(hairline());
    // **Nothing selected is not an empty form.** A time and a regime under "No vessel selected" would be
    // a verdict about nobody: the field belongs to a hull, and with no hull the panel says what to do
    // instead of drawing six segments and a banner that cannot be sent anywhere.
    if selected.is_none() {
        return form
            .child(caption("Select a vessel to report").key("no-selection"))
            .child(spacer());
    }
    form = form
        .child(caption("THIS REPORT · ONE FIX"))
        .child(coordinates(state))
        .child(measurements(state))
        .child(hairline())
        .child(
            row()
                .key("observed-at")
                .w_full()
                .gap(tokens::GAP)
                .items_center()
                .child(caption("OBSERVED AT"))
                .child(spacer())
                .child(quiet_button("Now", "now", |state, tx| {
                    state.today();
                    tx.blur();
                }))
                .child(caption("WIB")),
        )
        .child(segments(state));
    if let Some(error) = &state.error {
        form = form.child(
            text(format!("{}: {error}", error.field()))
                .key("field-error")
                .role("alert")
                .font(tokens::LABEL)
                .color(tokens::ink()),
        );
    }
    if let Some(notice) = &state.notice {
        form = form.child(
            text(notice)
                .key("field-notice")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }
    let banner_colour = match state.verdict.regime {
        Regime::Live => tokens::accent(),
        Regime::Backfill | Regime::Refused | Regime::Future => tokens::ink(),
        Regime::Unknown => tokens::ink_dim(),
    };
    let utc = zoned::utc_civil(state.epoch_ms);
    // **The banner, not a line of small print.** This sentence is the whole reason the field exists:
    // it says whether the time will be kept or replaced. The wireframe draws it as a filled block, and a
    // block is also what a reader can find at arm's length without hunting for one grey line among five.
    form = form
        .child(
            el().key("regime-banner")
                .w_full()
                .padding(tokens::CHIP_PAD)
                .border(tokens::HAIRLINE, banner_colour)
                .radius(tokens::PANEL_RADIUS)
                .child(
                    text(state.verdict.regime.sentence())
                        .key("regime")
                        .role("status")
                        .font(tokens::TITLE)
                        .color(banner_colour),
                ),
        )
        .child(
            text(format!(
                "= {:02}:{:02}:{:02}Z",
                utc.hour, utc.minute, utc.second
            ))
            .key("utc-readout")
            .font(tokens::LABEL)
            .color(tokens::ink_dim()),
        );
    if state.verdict.near_boundary {
        form = form.child(
            text(HEDGE)
                .key("regime-hedge")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }
    form.child(readiness_line(state))
        .child(hairline())
        .child(caption("SOURCE · KEPT HERE, NEVER SENT"))
        .child(source_note(state))
        .child(spacer())
        .child(update_button(state))
}

/// `REPORT · 1 OF 2 VESSELS` when the hull has a row, `NOT IN THE BATCH` when it has not, and the empty
/// word when nothing is selected. The position is the tray's, so the two panels count the same draft.
fn report_header(state: &State) -> String {
    let Some(vessel) = state.desk.selected_vessel() else {
        return "REPORT · NO VESSEL SELECTED".to_string();
    };
    match state.desk.batch_position(vessel.id_unit) {
        Some(position) => format!(
            "REPORT · {} OF {} VESSELS",
            position + 1,
            state.desk.batch().len()
        ),
        None => "REPORT · NOT IN THE BATCH".to_string(),
    }
}

/// The picture's last position in words, or the sentence that says there is none.
///
/// Absence is not zero: a hull that never reported has no coordinates, and the line says so instead of
/// drawing `0.00000, 0.00000` — a real place in the Gulf of Guinea and a false fact about the ship.
fn last_known(selected: Option<&Vessel>) -> El {
    let Some(last) = selected.and_then(|vessel| vessel.last.as_ref()) else {
        return line("no position in the standing picture").key("last-known");
    };
    let civil = zoned::at_offset(last.recorded_at_ms, WIB_OFFSET_MS);
    let mut words = format!(
        "{:.5}, {:.5} · {:02}:{:02}:{:02} WIB · {}",
        last.latitude,
        last.longitude,
        civil.hour,
        civil.minute,
        civil.second,
        last.age_words()
    );
    if last.backfilled {
        words.push_str(" · backfilled");
    }
    if let Some(speed) = last.speed_kn {
        words.push_str(&format!(" · {speed:.1} kn"));
    }
    if let Some(course) = last.course_deg {
        words.push_str(&format!(" · {course:.0}°"));
    }
    line(words).key("last-known")
}

/// The two coordinate fields. A filled one is a solid box with the value the row would carry; an empty one
/// is dashed and says `type or place`, because the map's drop is the next commit and the desk does not
/// invent a coordinate to fill the gap.
fn coordinates(state: &State) -> El {
    let draft = state
        .desk
        .selected()
        .map(|id_unit| state.desk.draft(id_unit))
        .unwrap_or_default();
    row()
        .w_full()
        .gap(tokens::GAP)
        .items_end()
        .child(coordinate("LATITUDE", "latitude", draft.latitude))
        .child(coordinate("LONGITUDE", "longitude", draft.longitude))
}

fn coordinate(label: &'static str, key: &'static str, value: Option<f64>) -> El {
    numeric_field(
        label,
        format!("coordinate-{key}"),
        value,
        5,
        "type or place",
    )
}

/// The three measured fields, from the same draft the coordinates come from.
///
/// They are drawn **dashed and saying `absent`** when the source gave nothing, because the difference
/// between "not measured" and "measured zero" is the difference between a ship nobody heard from and a
/// ship stopped dead — and the payload carries the first as a missing field, not as `0`.
fn measurements(state: &State) -> El {
    let draft = state
        .desk
        .selected()
        .map(|id_unit| state.desk.draft(id_unit))
        .unwrap_or_default();
    row()
        .w_full()
        .gap(tokens::GAP)
        .items_end()
        .child(numeric_field(
            "SPEED · KN",
            "measure-speed_kn".to_string(),
            draft.speed_kn,
            1,
            "absent",
        ))
        .child(numeric_field(
            "COURSE · DEG",
            "measure-course_deg".to_string(),
            draft.course_deg,
            0,
            "absent",
        ))
        .child(numeric_field(
            "ACCURACY · M",
            "measure-accuracy_m".to_string(),
            draft.accuracy_m,
            0,
            "absent",
        ))
}

fn numeric_field(
    label: &'static str,
    key: String,
    value: Option<f64>,
    places: usize,
    placeholder: &'static str,
) -> El {
    let filled = value.is_some();
    let words = value.map_or_else(
        || placeholder.to_string(),
        |value| format!("{value:.places$}"),
    );
    let mut field = el()
        .key(key)
        .role("textbox")
        .read_only(true)
        .label(format!("{label}, {words}"))
        .w_full()
        .h(tokens::NAME_H + tokens::GAP)
        .px(tokens::GAP)
        .items_center()
        .radius(tokens::GAP)
        .fill(tokens::bg())
        .border(
            tokens::HAIRLINE,
            if filled {
                tokens::card_edge()
            } else {
                tokens::ink_dim()
            },
        )
        .child(text(words).font(tokens::LABEL).color(if filled {
            tokens::ink()
        } else {
            tokens::ink_dim()
        }));
    if !filled {
        field = field.border_dash(tokens::CHIP_PAD, tokens::CARD_GAP);
    }
    col()
        .grow(1.0)
        .gap(tokens::CARD_GAP)
        .child(caption(label))
        .child(field)
}

/// The row's verdict in words, from the batch model's own rule. `nothing to send yet` is not a disabled
/// form: it is the honest answer to "what would leave if I pressed send", with the reason beside it.
fn readiness_line(state: &State) -> El {
    let words = match state.readiness() {
        None => "nothing to send yet — select a vessel".to_string(),
        Some(Readiness::Sendable) => "ready — one fix, one request".to_string(),
        Some(Readiness::Blocked(reason)) => format!("nothing to send yet — {}", reason.sentence()),
    };
    text(words)
        .key("readiness")
        .role("status")
        .font(tokens::LABEL)
        .color(tokens::ink_dim())
}

/// Decision 6's note is the desk's own record: kept here, never sent, and empty until the operator types
/// one. An empty note says so rather than drawing an `"x"` that would look like evidence.
fn source_note(state: &State) -> El {
    let note = state
        .desk
        .batch()
        .source_note
        .as_deref()
        .unwrap_or("no note yet");
    el().key("source-note")
        .role("note")
        .label(format!("Source note, kept on this desk: {note}"))
        .w_full()
        .padding(tokens::CHIP_PAD)
        .border_sides([0.0, 0.0, tokens::HAIRLINE, 0.0], tokens::card_edge())
        .child(line(note))
}

/// The inspector's way into the draft. The same action as the tray's `+ Add vessel`, with the word the
/// panel has always used for replacing a row the operator is editing.
fn update_button(state: &State) -> El {
    let enabled = state.desk.selected().is_some();
    // **`Add` is not `Update`.** A hull that is not in the draft has no row to replace, and a button that
    // said "Update" over "NOT IN THE BATCH" would be the panel contradicting its own header.
    let label = match state.desk.selected() {
        Some(id_unit) if state.desk.in_batch(id_unit) => "Update in batch",
        Some(_) => "Add to batch",
        None => "nothing to send yet",
    };
    el().key("update-batch")
        .role("button")
        .label(label)
        .disabled(!enabled)
        .px(tokens::GAP)
        .py(tokens::CHIP_PAD)
        .radius(tokens::GAP)
        .fill(if enabled {
            tokens::accent()
        } else {
            tokens::name_plate()
        })
        .items_center()
        .justify_center()
        .on_press_tx(|state: &mut State, tx: &mut rune::Tx| {
            state.add_selected();
            tx.blur();
        })
        .child(text(label).font(tokens::LABEL).color(if enabled {
            tokens::bg()
        } else {
            tokens::ink_dim()
        }))
}

/// `dd / mm / yyyy` and `hh:mm:ss`, each segment its own press target so a click can put the
/// caret where the keyboard will type. The group's label names the field and its value, because a
/// reader who cannot see the six cells still has to know what they say.
fn segments(state: &State) -> El {
    col()
        .key("segments")
        .role("group")
        .label(format!("Observed at {}", state.segments.text()))
        .gap(tokens::CARD_GAP)
        .child(
            row()
                .gap(tokens::GAP)
                .items_center()
                .child(segment(state, Field::Day))
                .child(separator("/"))
                .child(segment(state, Field::Month))
                .child(separator("/"))
                .child(segment(state, Field::Year)),
        )
        .child(
            row()
                .gap(tokens::GAP)
                .items_center()
                .child(segment(state, Field::Hour))
                .child(separator(":"))
                .child(segment(state, Field::Minute))
                .child(separator(":"))
                .child(segment(state, Field::Second)),
        )
}

/// One segment: its digits, and a caret under the one the keyboard is on. The caret is a rule,
/// not a glyph, so it cannot be mistaken for a typed character.
fn segment(state: &State, field: Field) -> El {
    let active = state.segments.active() == field;
    let mut cell = col()
        .items_center()
        .gap(tokens::CARD_GAP)
        .on_press_tx(move |state: &mut State, tx: &mut rune::Tx| {
            state.focus = Focus::Field;
            state.segments.select(field);
            tx.blur();
        })
        .child(
            text(state.segments.segment_text(field))
                .font(tokens::TITLE)
                .color(if active {
                    tokens::ink()
                } else {
                    tokens::ink_dim()
                }),
        );
    if active {
        cell = cell.child(el().h(tokens::HAIRLINE).w_full().fill(tokens::accent()));
    }
    cell
}

fn quiet_button(
    words: &'static str,
    key: &'static str,
    press: impl Fn(&mut State, &mut rune::Tx) + 'static,
) -> El {
    el().key(key)
        .role("button")
        .label(words)
        .px(tokens::GAP)
        .py(tokens::CHIP_PAD)
        .radius(tokens::GAP)
        .fill(tokens::name_plate())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .items_center()
        .justify_center()
        .on_press_tx(press)
        .child(text(words).font(tokens::LABEL).color(tokens::ink()))
}

/// A month arrow: the glyph on screen, the words in the accessible name.
fn glyph_button(
    glyph: &'static str,
    name: &'static str,
    key: &'static str,
    press: impl Fn(&mut State, &mut rune::Tx) + 'static,
) -> El {
    el().key(key)
        .role("button")
        .label(name)
        .size(tokens::PANE_BUTTON, tokens::PANE_BUTTON)
        .radius(tokens::GAP)
        .fill(tokens::name_plate())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .items_center()
        .justify_center()
        .on_press_tx(press)
        .child(text(glyph).font(tokens::TITLE).color(tokens::ink()))
}

/// The calendar, drawn open beside the field. Its grid is [`Month::weeks`]'s, so the view never
/// decides where a month starts or how many rows it has.
fn calendar(state: &State) -> El {
    let month = state.month;
    col()
        .key("calendar-popover")
        .role("dialog")
        .label("Choose a date")
        .w(tokens::SETTINGS_W)
        .padding(tokens::PAD)
        .gap(tokens::GAP)
        .radius(tokens::PANEL_RADIUS)
        .fill(tokens::panel())
        .border(tokens::HAIRLINE, tokens::card_edge())
        .child(
            row()
                .w_full()
                .gap(tokens::GAP)
                .items_center()
                .child(glyph_button(
                    "‹",
                    "Previous month",
                    "previous-month",
                    |state, tx| {
                        state.shift_month(-1);
                        tx.blur();
                    },
                ))
                .child(spacer())
                .child(
                    text(format!("{} {}", month.name(), month.year()))
                        .key("calendar-title")
                        .font(tokens::TITLE)
                        .color(tokens::ink()),
                )
                .child(spacer())
                .child(glyph_button(
                    "›",
                    "Next month",
                    "next-month",
                    |state, tx| {
                        state.shift_month(1);
                        tx.blur();
                    },
                )),
        )
        .child(weekday_header(month))
        .child(week_grid(state))
        .child(
            row()
                .gap(tokens::GAP)
                .child(quiet_button("Today", "calendar-today", |state, tx| {
                    state.today();
                    tx.blur();
                }))
                .child(quiet_button("Clear", "calendar-clear", |state, tx| {
                    state.clear();
                    tx.blur();
                })),
        )
}

fn weekday_header(month: Month) -> El {
    let mut header = row()
        .key("calendar-weekdays")
        .label(format!("Weekdays, {} starting Monday", month.name()));
    for name in ["M", "T", "W", "T", "F", "S", "S"] {
        header = header.child(
            el().w(tokens::MENU_ROW)
                .items_center()
                .justify_center()
                .child(text(name).font(tokens::LABEL).color(tokens::ink_dim())),
        );
    }
    header
}

fn week_grid(state: &State) -> El {
    let month = state.month;
    let mut grid = col()
        .key("calendar-grid")
        .role("grid")
        .label(format!("{} {}", month.name(), month.year()))
        .gap(tokens::CHIP_GAP);
    for week in month.weeks() {
        let mut cells = row().role("row");
        for day in week {
            cells = cells.child(match day {
                Some(day) => day_cell(state, day),
                None => el()
                    .role("gridcell")
                    .a11y_hidden()
                    .size(tokens::MENU_ROW, tokens::MENU_ROW),
            });
        }
        grid = grid.child(cells);
    }
    grid
}

fn day_cell(state: &State, day: u32) -> El {
    let month = state.month;
    let selected = state.selected_day() == Some(day);
    let name = format!(
        "{day} {} {}{}",
        month.name(),
        month.year(),
        if selected { ", selected" } else { "" }
    );
    el().key(format!("day-{day:02}"))
        .role("gridcell")
        .label(name)
        .size(tokens::MENU_ROW, tokens::MENU_ROW)
        .items_center()
        .justify_center()
        .radius(tokens::GAP)
        .fill(if selected {
            tokens::name_plate()
        } else {
            tokens::clear()
        })
        .on_press_tx(move |state: &mut State, tx: &mut rune::Tx| {
            state.pick_day(day);
            tx.blur();
        })
        .child(
            text(format!("{day:02}"))
                .font(tokens::LABEL)
                .color(if selected {
                    tokens::ink()
                } else {
                    tokens::ink_dim()
                }),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene;

    fn frame_once(state: State) -> rune::Runtime<State> {
        let mut runtime = rune::Runtime::new(
            state,
            view,
            tokens::DESK_WINDOW,
            rune::Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        // Rune's window installs `AppSpec::on_key` on the runtime it builds; a bare `Runtime::new`
        // does not, so the test installs the same function the window does.
        runtime.on_key = Some(on_key);
        runtime.frame(0.0);
        runtime
    }

    fn element(runtime: &rune::Runtime<State>, key: &str) -> rune::ElementInfo {
        runtime
            .elements()
            .into_iter()
            .find(|element| element.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("nothing is drawn at {key}"))
    }

    #[test]
    fn the_sample_screen_draws_the_field_the_calendar_and_the_batch_button() {
        let runtime = frame_once(scene::state_for("time-calendar"));
        for key in [
            "segments",
            "regime",
            "utc-readout",
            "regime-hedge",
            "calendar-popover",
            "calendar-grid",
            "update-batch",
        ] {
            assert!(
                runtime.find(key).is_some(),
                "the screen has no {key}: the wireframe's panel is incomplete"
            );
        }
    }

    #[test]
    fn the_segments_are_a_group_whose_label_names_the_field_and_its_value() {
        let runtime = frame_once(scene::state_for("time-field"));
        let group = element(&runtime, "segments");
        assert_eq!(group.role, "group");
        let label = group.label.as_deref().unwrap_or_default();
        assert!(
            label.contains("Observed at") && label.contains("20 / 11 / 2026"),
            "the group is announced as {label:?}, which does not name the field and its value"
        );
    }

    #[test]
    fn the_calendar_is_a_grid_of_labelled_cells_with_the_twentieth_selected() {
        let runtime = frame_once(scene::state_for("time-calendar"));
        let grid = element(&runtime, "calendar-grid");
        assert_eq!(grid.role, "grid");
        let cells: Vec<_> = runtime
            .elements()
            .into_iter()
            .filter(|element| element.role == "gridcell" && element.label.is_some())
            .collect();
        assert_eq!(
            cells.len(),
            30,
            "November 2026 has thirty days, and the grid has no cell for another"
        );
        let twentieth = element(&runtime, "day-20");
        assert_eq!(twentieth.role, "gridcell");
        assert!(
            twentieth
                .label
                .as_deref()
                .is_some_and(|label| label == "20 November 2026, selected"),
            "the twentieth is announced as {:?}",
            twentieth.label
        );
    }

    /// **A popover, not a column.** The claim is structural and behavioural: there is a catcher for a
    /// press outside, and that press cannot commit a half-typed time on its way past.
    #[test]
    fn the_calendar_is_a_popover_and_dismissing_it_commits_nothing() {
        let runtime = frame_once(scene::state_for("time-calendar"));
        assert!(
            runtime.find("calendar-catcher").is_some(),
            "the calendar has no catcher: a press outside would leave it covering the field"
        );
        assert!(runtime.find("calendar-popover").is_some());

        let mut state = scene::state_for("time-field");
        let committed = state.committed;
        state.type_digit(0);
        state.dismiss_calendar();
        assert!(!state.calendar_open);
        assert_eq!(
            state.committed, committed,
            "dismissing a surface must not commit the digits typed under it"
        );
    }

    /// **The anchor, measured.** Rune reports each element's rect, so this is not a description of the
    /// layout but a reading of it: with the calendar open the field is in the same place, and the
    /// popover's top edge is at or below the field's bottom edge.
    #[test]
    fn opening_the_calendar_does_not_move_the_field() {
        let mut closed_state = scene::state_for("time-field");
        closed_state.calendar_open = false;
        let mut open_state = scene::state_for("time-field");
        open_state.calendar_open = true;

        let closed = frame_once(closed_state);
        let open = frame_once(open_state);
        let field_closed = element(&closed, "segments").rect;
        let field_open = element(&open, "segments").rect;
        assert_eq!(
            field_closed, field_open,
            "the field moved when the calendar opened: {field_closed:?} -> {field_open:?}"
        );

        // Rune resolves the placement in the headless runtime too, so this is a measurement and not a
        // description: the popover is one gap below the row that names the time, left-aligned with it.
        let anchor = element(&open, "segments").rect;
        let popover = element(&open, "calendar-popover").rect;
        assert_eq!(
            popover[0], anchor[0],
            "the popover is not left-aligned with its anchor: anchor {anchor:?}, popover {popover:?}"
        );
        assert_eq!(
            popover[1],
            anchor[1] + anchor[3] + tokens::GAP,
            "the popover is not one gap below the field it belongs to: field {anchor:?}, popover {popover:?}"
        );
    }

    #[test]
    fn the_primary_button_and_the_calendar_arrows_are_buttons_in_words() {
        let runtime = frame_once(scene::state_for("time-calendar"));
        for (key, name) in [
            // The sample selects a hull that is **not** in the draft, so the button says what it will do.
            ("update-batch", "Add to batch"),
            ("previous-month", "Previous month"),
            ("next-month", "Next month"),
            ("calendar-today", "Today"),
            ("calendar-clear", "Clear"),
        ] {
            let button = element(&runtime, key);
            assert_eq!(button.role, "button", "{key} is not a button");
            assert_eq!(
                button.label.as_deref(),
                Some(name),
                "{key} is announced by a different name"
            );
        }
    }

    #[test]
    fn the_regime_line_is_the_live_one_and_the_hedge_is_drawn() {
        let runtime = frame_once(scene::state_for("time-field"));
        let regime = element(&runtime, "regime");
        assert_eq!(regime.role, "status");
        assert!(
            regime
                .text
                .as_deref()
                .is_some_and(|text| text.starts_with("live")),
            "the sample is three seconds ahead of the server, which is live: {:?}",
            regime.text
        );
        assert!(
            runtime.find("regime-hedge").is_some(),
            "three seconds is inside the near-boundary band, so the hedge must be drawn"
        );
        assert_eq!(
            element(&runtime, "utc-readout").text.as_deref(),
            Some("= 07:03:22Z"),
            "14:03:22 WIB is 07:03:22Z, and the readout is the only place it is said"
        );
    }

    #[test]
    fn an_impossible_date_is_an_alert_that_names_the_day() {
        let mut state = scene::state_for("time-field");
        state.segments.select(Field::Day);
        state.segments.set(Field::Day, 31);
        state.segments.set(Field::Month, 2);
        state.enter();
        let runtime = frame_once(state);
        let alert = runtime
            .elements()
            .into_iter()
            .find(|element| element.role == "alert")
            .expect("an impossible date is drawn as an alert");
        let text = alert.text.as_deref().unwrap_or_default();
        assert!(
            text.starts_with("day:"),
            "the alert must name the segment to change: {text:?}"
        );
    }

    #[test]
    fn picking_a_day_sets_the_date_the_calendar_is_showing() {
        let mut state = scene::state_for("time-field");
        state.shift_month(1);
        assert_eq!(state.month.name(), "December");
        assert_eq!(state.selected_day(), None, "20 November is not in December");
        state.pick_day(3);
        let committed = state.segments.commit().expect("3 December is a date");
        assert_eq!(
            (committed.year, committed.month, committed.day),
            (2026, 12, 3)
        );
        assert!(!state.calendar_open, "a picked day closes the popover");
        assert_eq!(state.selected_day(), Some(3));
    }

    #[test]
    fn escape_puts_the_accepted_time_back() {
        let mut state = scene::state_for("time-field");
        state.clear();
        state.type_digit(1);
        state.type_digit(2);
        state.revert();
        assert_eq!(
            state
                .segments
                .commit()
                .expect("the committed time is whole"),
            at(2026, 11, 20, 14, 3, 22),
            "Escape reverts to the last accepted time, not to an empty field"
        );
    }

    #[test]
    fn a_pasted_time_in_another_zone_is_shown_on_the_fields_clock() {
        let mut state = scene::state_for("time-field");
        state.paste("2026-11-20T14:03:22+05:30");
        let committed = state
            .segments
            .commit()
            .expect("the converted time is whole");
        assert_eq!(
            (committed.hour, committed.minute),
            (15, 33),
            "14:03:22 at +05:30 is 15:33:22 in WIB"
        );
        assert_eq!(committed.day, 20);
    }

    #[test]
    fn a_paste_that_is_not_one_of_the_four_forms_says_so_on_the_field() {
        let mut state = scene::state_for("time-field");
        state.paste("yesterday");
        let notice = state.notice.as_deref().unwrap_or_default();
        assert!(
            notice.contains("2026-11-20T14:03:22Z") && notice.contains("20/11/2026 14:03"),
            "the refusal must repeat the accepted forms: {notice}"
        );
    }

    #[test]
    fn every_key_the_form_claims_reaches_the_model() {
        // The keys' own routing is Rune's (`on_key` is the window's); this is the mapping the
        // form promises: digits type, arrows step and move, Backspace eats, Tab advances.
        let mut state = scene::state_for("time-field");
        state.segments.clear();
        state.type_digit(2);
        state.type_digit(0);
        assert_eq!(state.segments.value(Field::Day), Some(20));
        assert_eq!(state.segments.active(), Field::Month);
        state.right();
        assert_eq!(state.segments.active(), Field::Year);
        state.left();
        assert_eq!(state.segments.active(), Field::Month);
        let mut eaten = scene::state_for("time-field");
        eaten.segments.select(Field::Month);
        eaten.backspace();
        assert_eq!(
            eaten.segments.value(Field::Month),
            Some(1),
            "backspace eats one digit of 11, leaving 1"
        );
        eaten.backspace();
        assert_eq!(eaten.segments.value(Field::Month), None);
    }

    /// **The keys reach the model through Rune's own route**, not only through the state methods:
    /// `Runtime::key` is the event the window's loop would deliver, so this is the claim a person
    /// at the desk would make — type two digits, press Escape, then type a day that is not one.
    ///
    /// The refusal is driven through the **day** segment on purpose: `Tab` belongs to the vessel list
    /// while a row is selected (#277), so a test that tabbed into the month would be testing which
    /// surface owns the key that day, not whether a refusal names its segment.
    #[test]
    fn the_keyboard_reaches_the_field_through_the_windows_own_route() {
        let mut runtime = frame_once(scene::state_for("time-field"));
        let press = |runtime: &mut rune::Runtime<State>, key: rune::Key| {
            runtime.key(rune::KeyEvent {
                key,
                mods: rune::Mods::default(),
            });
            runtime.frame(0.0);
        };

        press(&mut runtime, rune::Key::Text("0".into()));
        press(&mut runtime, rune::Key::Text("3".into()));
        let label = element(&runtime, "segments").label.unwrap_or_default();
        assert!(
            label.contains("03 / 11 / 2026"),
            "the digits did not reach the day segment: {label}"
        );

        press(&mut runtime, rune::Key::Escape);
        let label = element(&runtime, "segments").label.unwrap_or_default();
        assert!(
            label.contains("20 / 11 / 2026"),
            "Escape must restore the accepted time: {label}"
        );

        press(&mut runtime, rune::Key::Text("3".into()));
        press(&mut runtime, rune::Key::Text("2".into()));
        press(&mut runtime, rune::Key::Enter);
        let alert = runtime
            .elements()
            .into_iter()
            .find(|element| element.role == "alert")
            .expect("a day of 32 draws an alert");
        assert!(
            alert
                .text
                .as_deref()
                .unwrap_or_default()
                .starts_with("day:"),
            "the alert must name the segment to change: {:?}",
            alert.text
        );
    }

    // -----------------------------------------------------------------------
    // The working surface (#277)
    // -----------------------------------------------------------------------

    /// A press the way an operator makes one: the point the layout gave the element, pressed and released.
    fn press(rt: &mut rune::Runtime<State>, key: &str) {
        let (x, y) = rt
            .find(key)
            .unwrap_or_else(|| panic!("nothing keyed {key:?} is drawn"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    }

    fn key(rt: &mut rune::Runtime<State>, key: rune::Key) {
        rt.key(rune::KeyEvent {
            key,
            mods: rune::Mods::default(),
        });
        rt.frame(0.0);
    }

    #[test]
    fn the_surface_draws_the_rail_the_chart_the_tray_and_the_inspector() {
        let runtime = frame_once(scene::state_for("batch"));
        for key in [
            "vessel-search",
            "vessel-list",
            "vessel-row-1",
            "vessel-row-3",
            "batch-tray",
            "tray-card-1",
            "tray-card-2",
            "add-vessel",
            "send-reports",
            "chart",
            "zoom-in",
            "zoom-out",
            "arm-placement",
            "inspector",
        ] {
            assert!(
                runtime.find(key).is_some(),
                "the desk's surface has no {key}: the wireframe's screen is incomplete"
            );
        }
        // The chart draws the picture's hulls: one dot per hull that has ever reported, and no dot for a
        // hull that never has — a marker at 0,0 would be a ship in the Gulf of Guinea.
        assert!(
            runtime.find("hull-1").is_some(),
            "the live hull is on the chart"
        );
        assert!(
            runtime.find("hull-2").is_none(),
            "a hull that never reported has no place on the chart"
        );
    }

    #[test]
    fn a_row_is_announced_as_one_label_naming_the_vessel_its_hull_and_its_age() {
        let runtime = frame_once(scene::state_for("batch"));
        let row = element(&runtime, "vessel-row-1");
        let label = row.label.as_deref().unwrap_or_default();
        assert_eq!(row.role, "option");
        for needed in ["KRI DIPONEGORO", "368", "23 s"] {
            assert!(
                label.contains(needed),
                "the row is announced as {label:?}, which does not name {needed:?}"
            );
        }
        let never = element(&runtime, "vessel-row-2");
        assert!(
            never
                .label
                .as_deref()
                .is_some_and(|label| label.contains("never")),
            "a hull that never reported says so instead of drawing an age: {:?}",
            never.label
        );
    }

    #[test]
    fn selecting_a_hull_prefills_the_inspector_and_the_field_from_the_picture() {
        let state = scene::state_for("batch");
        assert_eq!(state.desk.selected(), Some(3), "the scene selects Halasan");
        let runtime = frame_once(state);
        assert_eq!(
            element(&runtime, "report-name").text.as_deref(),
            Some("KRI HALASAN")
        );
        assert_eq!(
            element(&runtime, "report-header").text.as_deref(),
            Some("REPORT · NOT IN THE BATCH"),
            "the selected hull is not in the draft, and the header must not pretend it is"
        );
        let known = element(&runtime, "last-known").text.unwrap_or_default();
        assert!(
            known.contains("-6.10230") && known.contains("5 h") && known.contains("backfilled"),
            "the last known line is {known:?}"
        );
        assert!(
            element(&runtime, "coordinate-latitude")
                .label
                .as_deref()
                .is_some_and(|label| label.contains("-6.10230")),
            "the coordinate is prefilled from the picture, not left blank"
        );
        assert!(
            element(&runtime, "readiness")
                .text
                .as_deref()
                .is_some_and(|words| words.starts_with("ready")),
            "a hull with a position and a backfilled time would be sent"
        );
    }

    #[test]
    fn a_hull_that_never_reported_has_the_empty_state_and_the_reason_in_words() {
        let mut state = scene::state_for("batch");
        state.select(2);
        let runtime = frame_once(state);
        assert_eq!(
            element(&runtime, "last-known-label").text.as_deref(),
            Some("LAST KNOWN · NONE")
        );
        let placeholders = runtime
            .elements()
            .into_iter()
            .filter(|element| element.text.as_deref() == Some("type or place"))
            .count();
        assert_eq!(
            placeholders, 2,
            "a hull with no position gets two dashed fields, not one and not none"
        );
        let readiness = element(&runtime, "readiness").text.unwrap_or_default();
        assert!(
            readiness.starts_with("nothing to send yet")
                && readiness.contains("both coordinates are needed"),
            "the empty state must say why, in the batch model's words: {readiness:?}"
        );
    }

    #[test]
    fn a_never_reported_hull_is_listed_and_is_not_stale() {
        let mut state = scene::state_for("batch");
        state.desk.set_filter(desk::Filter::Never);
        let never: Vec<u64> = state
            .desk
            .visible()
            .iter()
            .map(|vessel| vessel.id_unit)
            .collect();
        assert!(
            never.contains(&2) && never.contains(&7) && never.contains(&8),
            "every silent hull is listed under Never: {never:?}"
        );
        state.desk.set_filter(desk::Filter::Stale);
        assert!(
            !state
                .desk
                .visible()
                .iter()
                .any(|vessel| vessel.id_unit == 2),
            "a hull that never reported has no age to have crossed the stale line"
        );
    }

    #[test]
    fn the_batch_shows_one_card_per_hull_and_a_badge_in_the_list_not_a_second_row() {
        let runtime = frame_once(scene::state_for("batch"));
        let cards = runtime
            .elements()
            .into_iter()
            .filter(|element| {
                element
                    .key
                    .as_deref()
                    .is_some_and(|key| key.starts_with("tray-card-"))
            })
            .count();
        assert_eq!(cards, 2, "one card per vessel, and the draft holds two");
        assert!(
            runtime.find("batch-badge-1").is_some() && runtime.find("batch-badge-2").is_some(),
            "a hull in the draft shows its count beside its row"
        );
        assert!(
            runtime.find("batch-badge-3").is_none(),
            "the selected hull is not in the draft, so it has no badge"
        );
        assert_eq!(
            element(&runtime, "tray-foot").text.as_deref(),
            Some("2 vessels · 2 fixes · nothing sent")
        );
        assert_eq!(
            element(&runtime, "send-reports").label.as_deref(),
            Some("Send 2 reports"),
            "one request per vessel means the send names the same number as the footer"
        );
    }

    #[test]
    fn the_send_says_that_nothing_is_sent_until_278() {
        let mut runtime = frame_once(scene::state_for("batch"));
        press(&mut runtime, "send-reports");
        assert!(
            runtime
                .elements()
                .into_iter()
                .filter_map(|element| element.text)
                .any(|words| words.contains("#278")),
            "the send control must not pretend to send"
        );
    }

    #[test]
    fn tab_walks_the_list_enter_selects_x_removes_and_escape_clears_the_search() {
        let mut state = scene::state_for("batch");
        state.select(1);
        let mut runtime = frame_once(state);

        // One Tab leaves the field for the list, the second moves to the next row (#277's focus rule).
        key(&mut runtime, rune::Key::Tab);
        key(&mut runtime, rune::Key::Tab);
        key(&mut runtime, rune::Key::Enter);
        assert_eq!(
            element(&runtime, "report-name").text.as_deref(),
            Some("KRI HALASAN"),
            "Tab moved to the next row and Enter selected it"
        );

        runtime.key(rune::KeyEvent {
            key: rune::Key::Tab,
            mods: rune::Mods {
                shift: true,
                ..rune::Mods::default()
            },
        });
        runtime.frame(0.0);
        key(&mut runtime, rune::Key::Enter);
        assert_eq!(
            element(&runtime, "report-name").text.as_deref(),
            Some("KRI DIPONEGORO"),
            "Shift-Tab walked back"
        );

        key(&mut runtime, rune::Key::Text("x".into()));
        assert!(
            runtime.find("tray-card-1").is_none(),
            "x removed the selected hull's row from the batch"
        );
        assert!(
            runtime.find("batch-badge-1").is_none(),
            "and the list's badge went with it"
        );

        let mut state = scene::state_for("batch");
        state.desk.set_query("makassar");
        // The search is the list's, so the list has the focus: Escape clears a search or reverts a time,
        // and which one it does is exactly what `Focus` is for.
        state.focus = Focus::List;
        let mut runtime = frame_once(state);
        assert!(
            runtime.find("vessel-row-1").is_none(),
            "the query narrowed the list to one hull"
        );
        key(&mut runtime, rune::Key::Escape);
        assert!(
            runtime.find("vessel-row-1").is_some(),
            "Escape cleared the search and put the picture back"
        );
    }

    #[test]
    fn a_selected_row_is_filled_while_its_neighbours_are_not() {
        let runtime = frame_once(scene::state_for("batch"));
        let selected = element(&runtime, "vessel-row-3");
        let other = element(&runtime, "vessel-row-1");
        assert_eq!(
            selected.fill,
            tokens::name_plate(),
            "the selected row is the filled one"
        );
        assert_ne!(
            other.fill,
            tokens::name_plate(),
            "an unselected row must not look selected"
        );
    }

    #[test]
    fn the_empty_desk_has_nothing_selected_and_a_disabled_send() {
        let runtime = frame_once(scene::state_for("empty"));
        assert_eq!(
            element(&runtime, "report-header").text.as_deref(),
            Some("REPORT · NO VESSEL SELECTED")
        );
        assert_eq!(
            element(&runtime, "tray-foot").text.as_deref(),
            Some("0 vessels · 0 fixes · nothing sent")
        );
        assert_eq!(
            element(&runtime, "send-reports").label.as_deref(),
            Some("Send 0 reports")
        );
        assert!(
            runtime.find("add-vessel").is_none(),
            "there is no selection to add"
        );
    }
}
