//! **The desk's one screen** — the time field, drawn by Rune (ADR-0009, decisions 3 and 5).
//!
//! The rules are not here. A regime is [`crate::time::verdict`]'s answer, a calendar is
//! [`crate::field::calendar`]'s grid, a typed time is [`crate::field::segment`]'s, a pasted one is
//! [`crate::field::paste`]'s, and a wall clock becomes an instant through [`crate::field::zoned`].
//! This module composes them into elements and nothing else: no boundary arithmetic, no calendar
//! arithmetic and no date parsing, so a colour or a rule that changes has one place to change.
//!
//! **Nothing here reads a clock or does I/O.** `State` carries the server's clock and "today" as
//! values a scene fixes, which is what makes `--snap` byte-identical between runs (`INV-11`'s
//! rule, applied to the desk). The real clock and the OS zone arrive with the client (#277, #278);
//! until then the field's zone is WIB and its "now" is the sample the scene supplies.
//!
//! **The field owns the keyboard.** Rune routes a key to the focused control first, so a button
//! that kept focus would take Tab, Return and Space away from the segments; every press here calls
//! [`rune::Tx::blur`] after its action, handing the keyboard back to the form. The buttons are
//! still exposed to assistive technology as buttons, by role and by name.

use arcons_cc::ui::tokens;
use rune::{Cx, El, col, el, row, spacer, text};

use crate::field::calendar::Month;
use crate::field::segment::{Field, Segments};
use crate::field::zoned::{self, Resolution};
use crate::field::{CivilTime, FieldError, civil, paste};
use crate::time::{self, Regime, Verdict};

/// The window title: one window, one application, one name.
pub const TITLE: &str = "Arcons Desk";

/// **The field's zone until the client carries the OS zone**: WIB, the clock the operator reads
/// (ADR-0009, decision 3). It is a time value, not a size, so it lives here rather than in the
/// wall's tokens.
pub const WIB_OFFSET_MS: i32 = 7 * 60 * 60 * 1000;

/// The vessel the wireframe's panel is about. A fixed sample for this ticket; the standing
/// picture and the reportable set arrive with the client (#277).
const VESSEL_NAME: &str = "KRI Sultan Hasanuddin";
const VESSEL_HULL: &str = "Hull 366 · corvette · 1 of 1";
const LAST_KNOWN: &str = "−6.10230, 106.88120 · 14:02:11 WIB · 12.4 kn · 284°";
const SOURCE_NOTE: &str = "Plot 4 · phone call 14:03";
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
            error: None,
            notice: None,
        }
    }

    /// The day the calendar highlights: the date the field now reads, when the calendar is
    /// showing that month. A day is not selected in a month it does not belong to.
    pub fn selected_day(&self) -> Option<u32> {
        let civil = self.segments.commit().unwrap_or(self.committed);
        (civil.year == self.month.year() && civil.month == self.month.month()).then_some(civil.day)
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

    /// The batch button: commit the field, and say the one thing this ticket can honestly say
    /// about the batch. The tray and the send are #277 and #278.
    pub fn hold_in_batch(&mut self) {
        self.enter();
        if self.error.is_none() {
            self.notice = Some("1 vessel in the batch — the send arrives with #278".to_string());
        }
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

/// **The keys, in one place.** Digits fill the active segment, Up and Down step it, Left and
/// Right move between segments, Backspace eats a digit, Tab moves on (Shift-Tab back), Enter
/// commits, Escape reverts, and Cmd/Ctrl+V pastes through the model's parser.
///
/// The function a window's key loop calls is the one a test calls: a rule that exists only in an
/// event handler is a rule nobody can test.
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

/// The screen: the report panel at the wireframe's width, and the calendar as an overlay.
///
/// The panel is pinned rather than grown, because that is the width the finished screen gives it: the
/// vessel list and the map take the rest (#277). The area they will take says so in one grey line, so a
/// capture of this ticket does not look like a finished application with its picture missing.
pub fn view(state: &State, _cx: &mut Cx<State>) -> El {
    let body = row()
        .w_full()
        .h_full()
        .items_start()
        .padding(tokens::PAD)
        .gap(tokens::PAD)
        .fill(tokens::bg())
        .child(
            col()
                .grow(1.0)
                .h_full()
                .items_start()
                .child(caption("The vessel list and the map arrive in #277")),
        )
        .child(form(state));
    calendar_overlay(state, body)
}

/// **The calendar is a popover, not a column.** It is attached at the root, after its anchor, because a
/// popover laid out as a sibling takes a share of the window instead of covering it — and then the field
/// moves when the calendar opens, which is the layout version of "the answer changed while I was reading
/// the question".
///
/// Rune places it below `observed-at` and flips it above when the window has no room below, the same
/// mechanism the wall's filter panel uses. The catcher is a transparent press over the whole window: a
/// press outside dismisses the calendar and does nothing else, so dismissing a surface never also edits
/// the field underneath it.
fn calendar_overlay(state: &State, body: El) -> El {
    if !state.calendar_open {
        return body;
    }
    let panel = calendar(state).anchored(
        "observed-at",
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

fn caption(words: impl Into<String>) -> El {
    text(words).font(tokens::LABEL).color(tokens::ink_dim())
}

fn line(words: impl Into<String>) -> El {
    text(words).font(tokens::LABEL).color(tokens::ink())
}

fn hairline() -> El {
    el().h(tokens::HAIRLINE).w_full().fill(tokens::hairline())
}

fn separator(mark: &'static str) -> El {
    text(mark).font(tokens::TITLE).color(tokens::ink_dim())
}

fn form(state: &State) -> El {
    let mut form = col()
        .w(tokens::DESK_PANEL)
        .h_full()
        .gap(tokens::GAP)
        .child(caption("REPORT · 1 OF 1 VESSELS"))
        .child(text(VESSEL_NAME).font(tokens::TITLE).color(tokens::ink()))
        .child(caption(VESSEL_HULL))
        .child(hairline())
        .child(caption("LAST KNOWN"))
        .child(line(LAST_KNOWN))
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
    form = form
        .child(
            text(state.verdict.regime.sentence())
                .key("regime")
                .role("status")
                .font(tokens::LABEL)
                .color(banner_colour),
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
    form.child(hairline())
        .child(caption("SOURCE · KEPT HERE, NEVER SENT"))
        .child(
            el().key("source-note")
                .role("note")
                .label(format!("Source note, kept on this desk: {SOURCE_NOTE}"))
                .w_full()
                .padding(tokens::CHIP_PAD)
                .border_sides([0.0, 0.0, tokens::HAIRLINE, 0.0], tokens::card_edge())
                .child(line(SOURCE_NOTE)),
        )
        .child(spacer())
        .child(
            el().key("update-batch")
                .role("button")
                .label("Update in batch")
                .px(tokens::GAP)
                .py(tokens::CHIP_PAD)
                .radius(tokens::GAP)
                .fill(tokens::accent())
                .items_center()
                .justify_center()
                .on_press_tx(|state: &mut State, tx: &mut rune::Tx| {
                    state.hold_in_batch();
                    tx.blur();
                })
                .child(
                    text("Update in batch")
                        .font(tokens::LABEL)
                        .color(tokens::bg()),
                ),
        )
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
        let runtime = frame_once(scene::state_for("time-field"));
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
        let runtime = frame_once(scene::state_for("time-field"));
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
        let runtime = frame_once(scene::state_for("time-field"));
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
        let anchor = element(&open, "observed-at").rect;
        let popover = element(&open, "calendar-popover").rect;
        assert_eq!(
            popover[0], anchor[0],
            "the popover is not left-aligned with its anchor: anchor {anchor:?}, popover {popover:?}"
        );
        assert_eq!(
            popover[1],
            anchor[1] + anchor[3] + tokens::GAP,
            "the popover is not one gap below the row that names the time: anchor {anchor:?}, popover {popover:?}"
        );
    }

    #[test]
    fn the_primary_button_and_the_calendar_arrows_are_buttons_in_words() {
        let runtime = frame_once(scene::state_for("time-field"));
        for (key, name) in [
            ("update-batch", "Update in batch"),
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
    /// at the desk would make — type two digits, press Escape, then Tab into the month and enter
    /// a month that does not exist.
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

        press(&mut runtime, rune::Key::Tab);
        press(&mut runtime, rune::Key::Text("1".into()));
        press(&mut runtime, rune::Key::Text("3".into()));
        press(&mut runtime, rune::Key::Enter);
        let alert = runtime
            .elements()
            .into_iter()
            .find(|element| element.role == "alert")
            .expect("a month of 13 draws an alert");
        assert!(
            alert
                .text
                .as_deref()
                .unwrap_or_default()
                .starts_with("month:"),
            "the alert must name the month: {:?}",
            alert.text
        );
    }
}
