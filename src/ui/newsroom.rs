//! **The newsroom pane** (#136): three scope tabs, the stories of the last 24 hours, and one page at a
//! time.
//!
//! A row is a **story**, not an article: `source_count` is how many outlets carried it, and the domain
//! beside it is the lead article's. The data layer owns the wire types and the read; this file owns the
//! per-pane question — which tab, which page — and draws it.
//!
//! **The pane's state is the pane's** (#99): two newsroom panes may sit on two scopes and two pages, so
//! the scope, the page and the rows are on the pane (like the tree's camera) and not on the wall.
//!
//! Three rules this module exists to keep:
//!
//! - **A late page is discarded, not applied.** Every ask records the [`NewsQuery`] it asked, and the
//!   reply carries that query back (`Reply::Stories`). A reply whose query is not the one this pane is
//!   waiting for answers a question nobody is asking any more — the operator changed tab or pressed Next
//!   — so it is dropped. "Newest arrival wins" would draw page 2 of the regional tab under the national
//!   heading.
//! - **`latest` is the server's window, never this machine's clock.** The pane sends the word `latest`
//!   and computes no 24-hour cutoff: an air-gapped box holding a three-day-old corpus must show those
//!   three days rather than an empty tab.
//! - **Absence is not zero** (`INV-01`). A story with no `country` draws a dash; a country with no
//!   `place` draws a dash for the place. Nothing is defaulted to a code, a name or a coordinate.
//!
//! The times are **WIB** (`dd MMM yyyy, HH.mm WIB`), a fixed +07:00 conversion of the server's UTC — the
//! mockup asks for WIB, and a fixed offset is a conversion, not a clock: nothing here reads `now()`.

use chrono::{DateTime, FixedOffset, Utc};
use rune::{El, col, el, row, spacer, text};

use crate::data::Error;
use crate::data::types::{NewsQuery, NewsScope, NewsStory, NewsWindow, Pagination, StoryPage};
use crate::ui::{ELLIPSIS, State, ViewId, tokens};

/// The three tabs, in the order the panel reads them. **There is no `unknown` tab**, deliberately: the
/// API's fourth scope is a real one, and with `min_confidence=0.5` every story in it is dropped anyway
/// (a story with no country carries no confidence). A fourth tab is a later decision, not an oversight.
pub const SCOPES: [(NewsScope, &str); 3] = [
    (NewsScope::International, "International"),
    (NewsScope::Regional, "Regional"),
    (NewsScope::National, "National"),
];

/// WIB is UTC+7, fixed. A zone database would make a capture depend on the machine's tzdata (`INV-11`),
/// and WIB has no daylight saving to track.
const WIB_OFFSET_HOURS: i32 = 7;

/// What a read is doing, as the eye needs it: "reading the feed…" is not "no stories", and neither is a
/// failure. The last good page stays on screen through all three.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadState {
    /// A page is in flight. Rows already on screen stay there: no blank flash between tabs.
    Reading,
    /// The page on screen is the answer to the last ask.
    Ready,
    /// The read failed, with the server's (or the transport's) words.
    Failed(String),
}

/// One newsroom pane's view: the tab, the page, the rows, and the query whose reply may still land.
#[derive(Clone, Debug)]
pub struct View {
    /// Which tab. `International` by default — the panel opens on the world.
    pub scope: NewsScope,
    /// The page whose rows are **on screen**, 1-based. A pager press does not move it — the reply does —
    /// so the footer never says "page 2" while page 1's stories are still drawn. A **tab** press does
    /// reset it to 1, because a new tab starts at its first page; the old rows stay until the reply lands.
    pub page: u32,
    /// The query this pane is waiting for, or `None`. A reply is applied only when it equals this — the
    /// rule that makes a late page harmless.
    pub asked: Option<NewsQuery>,
    pub stories: Vec<NewsStory>,
    pub pagination: Option<Pagination>,
    pub state: ReadState,
    /// The row the operator pressed. The detail ticket consumes it; this ticket records it so a press is
    /// not a dead gesture.
    pub selected: Option<i64>,
}

impl Default for View {
    fn default() -> Self {
        Self {
            scope: NewsScope::International,
            page: 1,
            asked: None,
            stories: Vec::new(),
            pagination: None,
            state: ReadState::Reading,
            selected: None,
        }
    }
}

impl View {
    pub fn new() -> Self {
        Self::default()
    }

    /// The question this pane is showing: its tab, the latest window, its page.
    pub fn query(&self) -> NewsQuery {
        NewsQuery {
            scope: self.scope,
            window: NewsWindow::Latest,
            page: self.page,
        }
    }

    /// Ask for the page on screen. Returns the query to send.
    pub fn ask(&mut self) -> NewsQuery {
        self.ask_page(self.page)
    }

    /// Ask for one page without moving the footer: `page` follows the reply (`apply`), never the press.
    pub fn ask_page(&mut self, page: u32) -> NewsQuery {
        let query = NewsQuery {
            scope: self.scope,
            window: NewsWindow::Latest,
            page,
        };
        self.asked = Some(query);
        self.state = ReadState::Reading;
        query
    }

    /// **Apply a reply only if it answers this pane's question.** Returns whether anything changed, so a
    /// stale page is a no-op and cannot move a tab.
    pub fn apply(&mut self, query: NewsQuery, result: &Result<StoryPage, Error>) -> bool {
        if self.asked != Some(query) {
            return false;
        }
        self.asked = None;
        match result {
            Ok(page) => {
                self.stories.clone_from(&page.rows);
                self.pagination = Some(page.pagination);
                self.page = query.page;
                self.state = ReadState::Ready;
            }
            // The rows that are on screen are still true; what failed is the next page. Keep them, say
            // what failed, and leave the footer on the page whose rows are drawn.
            Err(error) => self.state = ReadState::Failed(error.to_string()),
        }
        true
    }

    /// Press a tab: the scope and the page change **now** (the tab is the operator's intent), the old
    /// rows stay until the reply lands. `None` when the tab pressed is the one already shown — no read
    /// for a press that changes nothing.
    pub fn press_scope(&mut self, scope: NewsScope) -> Option<NewsQuery> {
        if self.scope == scope {
            return None;
        }
        self.scope = scope;
        self.page = 1;
        Some(self.ask())
    }

    /// The page a pager button would ask for, or `None` when that button is disabled. **One function
    /// decides** both what the footer draws and what a press sends, so a disabled control cannot send a
    /// request and a request cannot come from a control that is not drawn.
    pub fn target(&self, step: i32) -> Option<u32> {
        let pagination = self.pagination?;
        let allowed = match step.signum() {
            -1 => pagination.has_previous_page,
            1 => pagination.has_next_page,
            _ => false,
        };
        if !allowed {
            return None;
        }
        let next = self.page.checked_add_signed(step)?;
        (next >= 1 && next <= pagination.total_pages).then_some(next)
    }

    /// Press a pager button. `None` when it is disabled — no request is sent for it.
    pub fn press_page(&mut self, step: i32) -> Option<NewsQuery> {
        let target = self.target(step)?;
        Some(self.ask_page(target))
    }

    /// Ask again after a failure, for the page whose rows are on screen.
    pub fn retry(&mut self) -> NewsQuery {
        self.ask()
    }

    /// The first story's title on screen, for the assertions that ask "did these two panes draw
    /// different rows?" without a text recogniser.
    pub fn first_title(&self) -> Option<&str> {
        self.stories.first().map(|story| story.title.as_str())
    }
}

// ---------------------------------------------------------------------------------------------
// The drawing.
// ---------------------------------------------------------------------------------------------

/// One pane's newsroom: the tab strip and the feed's own count, the list, and the footer.
///
/// Nothing here waits and nothing here reads a clock: the first page was asked on the frame the pane was
/// opened (`ui::open_newsroom_pane`), and the reply arrives between frames.
pub fn content(state: &State, pane: ViewId) -> El {
    let body = col()
        .key(format!("newsroom-{pane}"))
        .w_full()
        .h_full()
        // Room for the pane's name plate and its two corner buttons, the way the tree pane leaves them.
        .pt(tokens::NEWS_TOP)
        .px(tokens::NEWS_PAD)
        .pb(tokens::NEWS_PAD)
        .gap(tokens::GAP);
    let Some(view) = state.pane(pane).and_then(|pane| pane.news.as_ref()) else {
        return body;
    };
    let mut body = body.child(header(pane, view)).child(list(pane, view));
    if view.pagination.is_some() {
        body = body.child(footer(pane, view));
    }
    body
}

/// The tab strip and the count: the feed's own `total_records`, so a reader knows how much is behind the
/// page rather than counting rows.
fn header(pane: ViewId, view: &View) -> El {
    let count = view
        .pagination
        .map(|pagination| format!("{} stories", pagination.total_records))
        .unwrap_or_default();
    row()
        .key(format!("news-head-{pane}"))
        .w_full()
        .h(tokens::NEWS_TAB_H)
        .items_center()
        .gap(tokens::GAP)
        .child(tabs(pane, view))
        .child(spacer())
        .child(
            text(count)
                .key(format!("news-count-{pane}"))
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        )
}

/// The three tabs. A bespoke segmented control, because the app has no kit and every colour and size
/// lives in [`tokens`]; the accessibility shape is Rune's own (`radiogroup` / `radio` / `checked`), so a
/// screen reader reads a choice rather than three words that happen to be clickable.
fn tabs(pane: ViewId, view: &View) -> El {
    row()
        .key(format!("news-tabs-{pane}"))
        .h(tokens::NEWS_TAB_H)
        .items_center()
        .gap(tokens::GAP / 2.0)
        .role("radiogroup")
        .label("Scope")
        .children(SCOPES.iter().map(move |(scope, label)| {
            let scope = *scope;
            let active = view.scope == scope;
            text(*label)
                .key(format!("news-tab-{pane}-{}", scope.as_str()))
                .font(tokens::LABEL)
                .color(if active {
                    tokens::ink()
                } else {
                    tokens::ink_dim()
                })
                .h(tokens::NEWS_TAB_H)
                .px(tokens::GAP)
                .radius(tokens::NEWS_TAB_H / 2.0)
                // The selected tab is a filled pill; the others are words. No accent: the accent on this
                // wall means "this is happening now", and a chosen tab is not news.
                .fill(if active {
                    tokens::name_plate()
                } else {
                    tokens::clear()
                })
                .role("radio")
                .checked(active)
                .label(*label)
                .on_press(move |state: &mut State| press_scope(state, pane, scope))
        }))
}

/// The story list: a scroll viewport, so the wall never hand-rolls an offset and a keyboard or a wheel
/// works the way it does everywhere else.
fn list(pane: ViewId, view: &View) -> El {
    let mut list = col()
        .key(format!("news-list-{pane}"))
        .w_full()
        .grow(1.0)
        // A scroll viewport needs a bounded height in every flex ancestor or it grows to its content and
        // never scrolls (`El::scroll`'s own rule, the same one the filter panel keeps).
        .min_h(tokens::NEWS_SCROLL_MIN_H)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .gap(tokens::NEWS_ROW_GAP);

    // The rows are drawn whenever there are any: an ask in flight over an old page is that page, not a
    // blank. Only a pane with nothing to show says why it has nothing.
    for (index, story) in view.stories.iter().enumerate() {
        list = list.child(story_row(pane, view, index, story));
    }
    match &view.state {
        ReadState::Failed(error) => {
            list = list
                .child(note(
                    pane,
                    "failed",
                    &format!("the read failed: {error}"),
                    tokens::ink(),
                ))
                .child(pager(pane, "retry", Press::Retry, true));
        }
        _ if view.stories.is_empty() => {
            let message = if view.state == ReadState::Reading {
                "reading the feed…"
            } else {
                "no stories in the last 24 hours"
            };
            list = list.child(note(pane, "empty", message, tokens::ink_dim()));
        }
        _ => {}
    }
    list
}

/// One story: the time and the place, the headline, and how many outlets carried it.
///
/// The press records the row — the detail ticket consumes it — and focuses the pane, so a press in one
/// pane never moves the keys out of another by accident.
fn story_row(pane: ViewId, view: &View, index: usize, story: &NewsStory) -> El {
    let id = story.id;
    let selected = view.selected == Some(id);
    col()
        .key(format!("news-row-{pane}-{index}"))
        .w_full()
        .h(tokens::NEWS_ROW_H)
        .gap(tokens::CARD_GAP)
        .justify_center()
        .px(tokens::GAP / 2.0)
        .radius(tokens::HAIRLINE * 4.0)
        .fill(if selected {
            tokens::name_plate()
        } else {
            tokens::clear()
        })
        .on_press(move |state: &mut State| select(state, pane, id))
        .child(
            text(when_and_where(story))
                .key(format!("news-when-{pane}-{index}"))
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        )
        .child(
            text(headline(&story.title))
                .key(format!("news-title-{pane}-{index}"))
                .font(tokens::LABEL)
                .color(tokens::ink()),
        )
        .child(
            text(sources(story))
                .key(format!("news-sources-{pane}-{index}"))
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        )
}

/// The footer: `previous`, the page the rows belong to, and `next`.
///
/// The page number is the **server's** `current_page` — the page whose rows are on screen — so it moves
/// only when a reply lands. A disabled button draws dimmer and carries no press at all.
fn footer(pane: ViewId, view: &View) -> El {
    let Some(pagination) = view.pagination else {
        return el();
    };
    row()
        .key(format!("news-foot-{pane}"))
        .w_full()
        .h(tokens::NEWS_FOOTER_H)
        .items_center()
        .gap(tokens::GAP)
        .child(pager(
            pane,
            "previous",
            Press::Page(-1),
            view.target(-1).is_some(),
        ))
        .child(spacer())
        .child(
            text(format!(
                "page {} of {} · {} stories",
                pagination.current_page, pagination.total_pages, pagination.total_records
            ))
            .key(format!("news-page-{pane}"))
            .font(tokens::LABEL)
            .color(tokens::ink_dim()),
        )
        .child(spacer())
        .child(pager(
            pane,
            "next",
            Press::Page(1),
            view.target(1).is_some(),
        ))
}

/// What a footer button does when it is pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// Ask the page `step` pages away — always ±1, and refused unless [`View::target`] allows it.
    Page(i32),
    /// Ask again for the page already on screen, after a failure.
    Retry,
}

/// One word button: `previous`, `next`, or `retry`.
///
/// A disabled one has no press handler **and** says so to assistive technology: a control that looks
/// inert and answers a press is worse than one that does nothing.
fn pager(pane: ViewId, label: &'static str, press: Press, enabled: bool) -> El {
    let mut button = text(label)
        .key(format!("news-{}-{pane}", label.replace(' ', "-")))
        .font(tokens::LABEL)
        .color(if enabled {
            tokens::ink()
        } else {
            tokens::ink_dim()
        })
        .h(tokens::NEWS_TAB_H)
        .px(tokens::GAP)
        .radius(tokens::NEWS_TAB_H / 2.0)
        .fill(if enabled {
            tokens::name_plate()
        } else {
            tokens::clear()
        })
        .disabled(!enabled);
    if enabled {
        button = button.on_press(move |state: &mut State| match press {
            Press::Page(step) => press_page(state, pane, step),
            Press::Retry => press_retry(state, pane),
        });
    }
    button
}

/// A line of words where the list would be: reading, empty, or failed.
fn note(pane: ViewId, kind: &'static str, message: &str, colour: rune::Color) -> El {
    row()
        .key(format!("news-{kind}-{pane}"))
        .w_full()
        .h(tokens::NEWS_ROW_H)
        .items_center()
        .child(text(message.to_string()).font(tokens::LABEL).color(colour))
}

// ---------------------------------------------------------------------------------------------
// The presses: one function each, all of them through the pane's own view.
// ---------------------------------------------------------------------------------------------

/// A press on a scope tab: focus the pane, change the scope, and ask page 1 — off the UI thread.
fn press_scope(state: &mut State, pane: ViewId, scope: NewsScope) {
    state.layout.focus(pane);
    let query = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .and_then(|view| view.press_scope(scope));
    if let Some(query) = query {
        ask(state, query);
    }
    state.layout_dirty = true;
}

/// A press on `previous`/`next`: the target comes from [`View::target`], so a disabled button cannot
/// send a request.
fn press_page(state: &mut State, pane: ViewId, step: i32) {
    state.layout.focus(pane);
    let query = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .and_then(|view| view.press_page(step));
    if let Some(query) = query {
        ask(state, query);
    }
    state.layout_dirty = true;
}

/// A press on `retry`: ask the page whose rows are on screen again.
fn press_retry(state: &mut State, pane: ViewId) {
    state.layout.focus(pane);
    let query = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .map(|view| view.retry());
    if let Some(query) = query {
        ask(state, query);
    }
    state.layout_dirty = true;
}

/// A press on a row: remember it in **this pane** and focus the pane. The detail read is the next ticket.
fn select(state: &mut State, pane: ViewId, story: i64) {
    state.layout.focus(pane);
    if let Some(view) = state.pane_mut(pane).and_then(|pane| pane.news.as_mut()) {
        view.selected = Some(story);
    }
    state.layout_dirty = true;
}

/// Send one ask, if there is a backend to send it to. Never blocks: this is the channel the view loop
/// writes to between frames.
fn ask(state: &mut State, query: NewsQuery) {
    if let Some(backend) = &state.backend {
        backend.request(crate::data::Command::Stories(query));
    }
}

// ---------------------------------------------------------------------------------------------
// The words.
// ---------------------------------------------------------------------------------------------

/// `dd MMM yyyy, HH.mm WIB`: the mockup's format, from the server's UTC.
pub fn wib(at: &DateTime<Utc>) -> String {
    match FixedOffset::east_opt(WIB_OFFSET_HOURS * 3600) {
        Some(offset) => at
            .with_timezone(&offset)
            .format("%d %b %Y, %H.%M WIB")
            .to_string(),
        // Unreachable: +07:00 is a valid offset. Not a panic, because a wall must not die over a label.
        None => at.format("%d %b %Y, %H.%M UTC").to_string(),
    }
}

/// The row's first line: when the story was learned (WIB), and where it happened — with a dash for each
/// part the corpus could not establish (`INV-01`).
pub fn when_and_where(story: &NewsStory) -> String {
    let place = match &story.country {
        None => "—".to_string(),
        Some(country) => format!(
            "{} · {}",
            country.name,
            country.place.as_deref().unwrap_or("—")
        ),
    };
    format!("{} · {place}", wib(&story.first_seen))
}

/// How many outlets carried it, and the lead one: "7 sources · en.antaranews.com", "1 source · …".
pub fn sources(story: &NewsStory) -> String {
    match story.source_count {
        1 => format!("1 source · {}", story.source),
        count => format!("{count} sources · {}", story.source),
    }
}

/// The headline, cut to the newsroom's own width with a drawn ellipsis. Not the pane-name rule: a name is
/// short by construction and a headline is not.
pub fn headline(title: &str) -> String {
    let trimmed = title.trim();
    let mut shown: String = trimmed.chars().take(tokens::NEWS_TITLE_MAX_CHARS).collect();
    if trimmed.chars().count() > tokens::NEWS_TITLE_MAX_CHARS {
        shown.push(ELLIPSIS);
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn page(
        from: &str,
        total: u64,
        pages: u32,
        current: u32,
        next: bool,
        back: bool,
        rows: usize,
    ) -> StoryPage {
        let mut stories = Vec::new();
        for index in 0..rows {
            stories.push(NewsStory {
                id: (current as i64) * 100 + index as i64,
                title: format!("{from} story {index}"),
                source_count: 1,
                country: None,
                source: "en.antaranews.com".to_string(),
                url: "https://en.antaranews.com/news/x".to_string(),
                first_seen: Utc.with_ymd_and_hms(2026, 11, 11, 5, 10, 0).unwrap(),
                last_seen: Utc.with_ymd_and_hms(2026, 11, 11, 5, 10, 0).unwrap(),
            });
        }
        StoryPage {
            rows: stories,
            pagination: Pagination {
                total_records: total,
                total_pages: pages,
                page_size: 25,
                current_page: current,
                has_next_page: next,
                has_previous_page: back,
            },
        }
    }

    /// **A reply is applied only when it answers the question this pane is waiting for.** This is the
    /// rule a late page depends on: the operator changed tab, the regional page arrived, and it must not
    /// draw under the national heading.
    #[test]
    fn a_late_page_is_discarded_and_a_current_one_is_applied() {
        let mut view = View::new();
        let regional = view
            .press_scope(NewsScope::Regional)
            .expect("a tab press asks");
        // The operator changes their mind before the reply lands.
        let national = view
            .press_scope(NewsScope::National)
            .expect("a second tab press asks");

        let stale = page("regional", 128, 6, 1, true, false, 3);
        assert!(
            !view.apply(regional, &Ok(stale.clone())),
            "a reply to a query nobody is waiting for must be dropped"
        );
        assert!(view.stories.is_empty(), "and it must draw no rows");
        assert_eq!(view.scope, NewsScope::National, "the tab is the operator's");

        let current = page("national", 9, 1, 1, false, false, 2);
        assert!(view.apply(national, &Ok(current)));
        assert_eq!(view.stories.len(), 2);
        assert_eq!(view.state, ReadState::Ready);
        assert_eq!(view.asked, None, "the ask is spent");
    }

    /// A tab press resets the page to 1 and asks; pressing the tab already shown asks nothing.
    #[test]
    fn a_tab_press_resets_the_page_and_the_current_tab_asks_nothing() {
        let mut view = View::new();
        let first = view.press_page(1);
        assert!(
            first.is_none(),
            "there is no page 2 before the server says so"
        );
        let first = view.ask();
        view.apply(first, &Ok(page("international", 128, 6, 1, true, false, 3)));
        assert_eq!(view.page, 1);

        let second = view.press_page(1).expect("page 2 exists");
        assert_eq!(second.page, 2);
        view.apply(
            second,
            &Ok(page("international p2", 128, 6, 2, true, true, 3)),
        );
        assert_eq!(view.page, 2, "the reply moved the footer, not the press");

        let tab = view
            .press_scope(NewsScope::Regional)
            .expect("a different tab asks");
        assert_eq!(tab.page, 1, "a new tab starts at page 1");
        assert_eq!(view.page, 1);
        assert_eq!(view.scope, NewsScope::Regional);
        assert!(
            view.press_scope(NewsScope::Regional).is_none(),
            "the tab already shown is not a read"
        );
    }

    /// **The footer's page number moves only when the reply lands**, and a disabled pager sends nothing.
    #[test]
    fn the_footer_page_moves_only_when_the_reply_lands() {
        let mut view = View::new();
        assert!(view.target(-1).is_none(), "no pagination yet, no button");
        let first = view.ask();
        view.apply(first, &Ok(page("international", 128, 6, 1, true, false, 3)));
        assert!(view.target(-1).is_none(), "page 1 has no previous page");
        assert_eq!(view.target(1), Some(2));
        assert_eq!(view.target(0), None, "a step of zero is not a page");
        assert_eq!(view.target(10), None, "a page past the last one is refused");

        let second = view.press_page(1).expect("page 2 exists");
        assert_eq!(
            view.page, 1,
            "the footer keeps the page whose rows are on screen until the reply lands"
        );
        // A failure keeps the rows and the footer where they are, and says what failed.
        view.apply(
            second,
            &Err(crate::data::Error::Forbidden {
                message: "no read on /system/banglinstra".into(),
            }),
        );
        assert_eq!(view.page, 1);
        assert_eq!(view.stories.len(), 3, "the last good page stays on screen");
        assert!(matches!(view.state, ReadState::Failed(ref why) if why.contains("banglinstra")));

        // The server's own `has_next_page` is the last word: a page that says there is no more sends
        // nothing, even if its number is below `total_pages`.
        let mut done = View::new();
        let ask = done.ask();
        done.apply(ask, &Ok(page("last", 128, 6, 6, false, true, 3)));
        assert_eq!(done.target(1), None, "the server said there is no page 7");
        assert_eq!(done.press_page(1), None);
    }

    /// WIB is a fixed +07:00 conversion of the server's UTC, not this machine's clock.
    #[test]
    fn the_time_is_wib_from_utc() {
        let at = Utc.with_ymd_and_hms(2026, 11, 11, 5, 10, 0).unwrap();
        assert_eq!(wib(&at), "11 Nov 2026, 12.10 WIB");
        let late = Utc.with_ymd_and_hms(2026, 11, 11, 20, 5, 0).unwrap();
        assert_eq!(
            wib(&late),
            "12 Nov 2026, 03.05 WIB",
            "the day rolls with the offset, and nothing here reads a clock"
        );
    }

    /// **Absence is not zero** (`INV-01`): no country is a dash, no place is a dash, and a measured
    /// `source_count` of one is "1 source" rather than "1 sources".
    #[test]
    fn absence_draws_a_dash_and_one_source_is_singular() {
        let at = Utc.with_ymd_and_hms(2026, 11, 11, 5, 10, 0).unwrap();
        let mut story = NewsStory {
            id: 1,
            title: "a title".into(),
            source_count: 1,
            country: None,
            source: "en.antaranews.com".into(),
            url: "https://example.test/x".into(),
            first_seen: at,
            last_seen: at,
        };
        assert_eq!(
            when_and_where(&story),
            "11 Nov 2026, 12.10 WIB · —",
            "a story with no country draws a dash, not an empty code and not a place at 0,0"
        );
        assert_eq!(sources(&story), "1 source · en.antaranews.com");

        story.country = Some(crate::data::types::NewsCountry {
            code: "MY".into(),
            name: "Malaysia".into(),
            place: None,
            confidence: 0.55,
        });
        assert_eq!(
            when_and_where(&story),
            "11 Nov 2026, 12.10 WIB · Malaysia · —",
            "a country without a city is a country and no city, not a made-up one"
        );

        story.source_count = 7;
        assert_eq!(sources(&story), "7 sources · en.antaranews.com");
    }

    /// A headline is cut to the newsroom's width with a drawn ellipsis, and a short one is untouched.
    #[test]
    fn a_long_headline_is_cut_with_an_ellipsis() {
        let short = "Coast guard patrols resume";
        assert_eq!(headline(short), short);
        let long = "x".repeat(tokens::NEWS_TITLE_MAX_CHARS + 10);
        let shown = headline(&long);
        assert_eq!(
            shown.chars().count(),
            tokens::NEWS_TITLE_MAX_CHARS + 1,
            "the cap plus the ellipsis"
        );
        assert!(shown.ends_with(ELLIPSIS));
    }

    /// The three tabs are the panel's three scopes, in reading order, and `unknown` is **not** a tab.
    #[test]
    fn the_panel_has_three_tabs_and_no_unknown_one() {
        let scopes: Vec<NewsScope> = SCOPES.iter().map(|(scope, _)| *scope).collect();
        assert_eq!(
            scopes,
            vec![
                NewsScope::International,
                NewsScope::Regional,
                NewsScope::National
            ]
        );
        assert!(
            !scopes.contains(&NewsScope::Unknown),
            "the API's fourth scope is real, and a tab for it would always be empty"
        );
        assert_eq!(SCOPES.len(), 3);
    }
}
