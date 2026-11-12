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
use crate::data::types::{
    NewsArticle, NewsQuery, NewsScope, NewsStory, NewsStoryDetail, NewsWindow, Pagination,
    StoryPage,
};
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

/// A story's provenance, as the detail read left it (#137).
///
/// A **mode**, not a replacement: the list's tab, page and rows stay on the view underneath, so `back`
/// returns to the page the operator left rather than to page 1.
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    /// The read is in flight: "reading the story…" rather than an empty list of articles.
    Reading,
    /// The story and the documents that carried it, oldest first.
    Ready(Box<NewsStoryDetail>),
    /// The read failed, with the server's (or the transport's) words.
    Failed(String),
}

/// One newsroom pane's view: the tab, the page, the rows, the pressed story, and the query whose reply
/// may still land.
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
    /// The row the operator pressed: the story whose detail this pane is showing, or `None` on the list.
    pub selected: Option<i64>,
    /// What the detail read said, when a story is open. `None` on the list.
    pub detail: Option<Detail>,
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
            detail: None,
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

    /// Whether this pane is showing a story rather than the list.
    pub fn detail_open(&self) -> bool {
        self.selected.is_some()
    }

    /// Press a row: that story becomes this pane's selection and its detail is read. `false` when the
    /// row was already open, so a press that changes nothing sends no read.
    pub fn select(&mut self, story: i64) -> bool {
        if self.selected == Some(story) {
            return false;
        }
        self.selected = Some(story);
        self.detail = Some(Detail::Reading);
        true
    }

    /// **Apply a detail only while this pane is still on that story** (#137). A slow story that lands
    /// after `back`, or after another row was pressed, answers a question nobody is asking any more.
    pub fn apply_detail(&mut self, id: i64, result: &Result<Box<NewsStoryDetail>, Error>) -> bool {
        if self.selected != Some(id) {
            return false;
        }
        match result {
            Ok(detail) => {
                // **The list is the truth, the header is the wire's number** (#137): a detail whose
                // `articles` do not match `source_count` is a server inconsistency, and it is a log line
                // rather than a header quietly adjusted to match what happened to arrive.
                if detail.articles.len() as u32 != detail.story.source_count {
                    tracing::warn!(
                        story = id,
                        articles = detail.articles.len(),
                        source_count = detail.story.source_count,
                        "the story's articles disagree with its source_count: the list is what came"
                    );
                }
                self.detail = Some(Detail::Ready(detail.clone()));
            }
            Err(error) => self.detail = Some(Detail::Failed(error.to_string())),
        }
        true
    }

    /// `back`: the list, exactly as it was left — the tab, the page, the rows and the footer. `false`
    /// when no story is open, so Escape falls through to the wall's next rung.
    pub fn back(&mut self) -> bool {
        if self.selected.is_none() {
            return false;
        }
        self.selected = None;
        self.detail = None;
        true
    }

    /// Ask again for the open story. `None` when no story is open.
    pub fn retry_detail(&mut self) -> Option<i64> {
        let id = self.selected?;
        self.detail = Some(Detail::Reading);
        Some(id)
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
    // **The detail is a surface over the list, not a replacement of it** (#137). The list stays mounted,
    // which is what keeps Rune's scroll viewport where the operator left it — unmounting it and drawing
    // it again would return `back` to the top of the page. The surface is opaque, catches the press, and
    // is drawn after the footer so nothing of the list shows through.
    if view.detail_open() {
        body = body.child(detail_surface(pane, view));
    }
    body
}

/// The story's provenance, over the list (#137): `back` and the title, the times and the sources, the
/// place and its confidence, the URL as text — and every outlet that carried it, **oldest first**.
///
/// One scroll viewport, the pane's own: the articles are rows in it, never a second nested viewport.
fn detail_surface(pane: ViewId, view: &View) -> El {
    let mut body = col()
        .key(format!("news-detail-body-{pane}"))
        .w_full()
        .h_full()
        .min_h(tokens::NEWS_SCROLL_MIN_H)
        .scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .pt(tokens::NEWS_TOP)
        .px(tokens::NEWS_PAD)
        .pb(tokens::NEWS_PAD)
        .gap(tokens::GAP)
        .child(back_row(pane, view));
    match view.detail.as_ref() {
        Some(Detail::Ready(detail)) => {
            let story = &detail.story;
            body = body
                .child(prose_line(
                    pane,
                    "provenance",
                    provenance_line(story),
                    tokens::ink_dim(),
                ))
                .child(prose_line(
                    pane,
                    "place",
                    country_line(story),
                    // A weak match reads as weak: the country is shown either way, and below half the
                    // corpus's agreement the line is drawn quietly rather than as settled fact.
                    match &story.country {
                        Some(country)
                            if country.confidence >= crate::data::http::MIN_CONFIDENCE =>
                        {
                            tokens::ink()
                        }
                        _ => tokens::ink_dim(),
                    },
                ))
                // **Provenance, never a control** (#135, #137): on an air-gapped wall this URL opens
                // nothing, so it is drawn as text and is not a button.
                .child(
                    text(story.url.clone())
                        .key(format!("news-url-{pane}"))
                        .font(tokens::LABEL)
                        .color(tokens::ink_dim()),
                );
            for (index, article) in detail.articles.iter().enumerate() {
                body = body.child(article_row(pane, index, article));
            }
        }
        Some(Detail::Reading) | None => {
            body = body.child(note(
                pane,
                "story-reading",
                "reading the story…",
                tokens::ink_dim(),
            ));
        }
        Some(Detail::Failed(error)) => {
            body = body
                .child(note(
                    pane,
                    "story-failed",
                    &format!("the story did not read: {error}"),
                    tokens::ink(),
                ))
                .child(retry_word(pane));
        }
    }
    el().key(format!("news-detail-{pane}"))
        // Over the pane's whole box, including the tab strip and the footer.
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(tokens::panel())
        .clip_rounded(0.0)
        // A press on the detail is not a press on the row underneath it: the list is still mounted, and
        // this is what keeps it out of the hand.
        .on_press(|_: &mut State| {})
        .child(body)
}

/// `back`, and the story's title in the heading style. The word is the exit the mouse has; Escape is the
/// exit the keyboard has, and both go through the same function.
fn back_row(pane: ViewId, view: &View) -> El {
    let title = match view.detail.as_ref() {
        Some(Detail::Ready(detail)) => detail.story.title.clone(),
        _ => String::new(),
    };
    row()
        .key(format!("news-back-row-{pane}"))
        .w_full()
        .gap(tokens::GAP)
        .items_center()
        .child(
            text(BACK)
                .key(format!("news-back-{pane}"))
                .font(tokens::LABEL)
                .color(tokens::ink())
                .h(tokens::NEWS_TAB_H)
                .px(tokens::GAP)
                .radius(tokens::NEWS_TAB_H / 2.0)
                .fill(tokens::name_plate())
                .role("button")
                .label("Back to the list")
                .on_press(move |state: &mut State| press_back(state, pane)),
        )
        .child(
            text(title)
                .key(format!("news-story-title-{pane}"))
                .font(tokens::TITLE)
                .color(tokens::ink())
                .grow(1.0),
        )
}

/// `retry`, after a detail that failed to read.
fn retry_word(pane: ViewId) -> El {
    text(RETRY)
        .key(format!("news-story-retry-{pane}"))
        .font(tokens::LABEL)
        .color(tokens::ink())
        .h(tokens::NEWS_TAB_H)
        .px(tokens::GAP)
        .radius(tokens::NEWS_TAB_H / 2.0)
        .fill(tokens::name_plate())
        .role("button")
        .label("Read the story again")
        .on_press(move |state: &mut State| press_retry_detail(state, pane))
}

/// One line of the detail's prose, at the provenance line's height and vertically centred: the box is
/// the token's, not whatever the glyphs happen to measure.
fn prose_line(pane: ViewId, kind: &'static str, words: String, colour: rune::Color) -> El {
    row()
        .key(format!("news-{kind}-{pane}"))
        .w_full()
        .h(tokens::NEWS_LINE_H)
        .items_center()
        .child(text(words).font(tokens::LABEL).color(colour))
}

/// One outlet that carried the story: its domain and the time it was seen (WIB), the headline, and the
/// facts the wire sent — each one only when it is there (`INV-01`).
fn article_row(pane: ViewId, index: usize, article: &NewsArticle) -> El {
    col()
        .key(format!("news-article-{pane}-{index}"))
        .w_full()
        .h(tokens::NEWS_ARTICLE_H)
        .gap(tokens::CARD_GAP)
        .justify_center()
        .child(
            row()
                .w_full()
                .gap(tokens::GAP)
                .child(
                    text(article.domain.clone())
                        .key(format!("news-domain-{pane}-{index}"))
                        .font(tokens::LABEL)
                        .color(tokens::ink()),
                )
                .child(spacer())
                .child(
                    text(wib(&article.seen_at))
                        .key(format!("news-seen-{pane}-{index}"))
                        .font(tokens::LABEL)
                        .color(tokens::ink_dim()),
                ),
        )
        .child(
            text(headline(&article.title))
                .key(format!("news-article-title-{pane}-{index}"))
                .font(tokens::LABEL)
                .color(tokens::ink()),
        )
        .child(
            text(article_facts(article))
                .key(format!("news-facts-{pane}-{index}"))
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        )
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
    state.focus_pane(pane);
    let query = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .and_then(|view| view.press_scope(scope));
    if let Some(query) = query {
        ask(state, query);
        // **The tab and the page are the pane's place in the feed** (#153): they are the part of a
        // newsroom pane a wall remembers, because they are where the operator was reading.
        state.arranged();
    }
    state.layout_dirty = true;
}

/// A press on `previous`/`next`: the target comes from [`View::target`], so a disabled button cannot
/// send a request.
fn press_page(state: &mut State, pane: ViewId, step: i32) {
    state.focus_pane(pane);
    let query = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .and_then(|view| view.press_page(step));
    if let Some(query) = query {
        ask(state, query);
        state.arranged();
    }
    state.layout_dirty = true;
}

/// A press on `retry`: ask the page whose rows are on screen again.
fn press_retry(state: &mut State, pane: ViewId) {
    state.focus_pane(pane);
    let query = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .map(|view| view.retry());
    if let Some(query) = query {
        ask(state, query);
    }
    state.layout_dirty = true;
}

/// A press on a row: the story opens **in this pane**, and its detail is read off the UI thread. The
/// list stays underneath, so `back` returns to the page the operator left.
fn select(state: &mut State, pane: ViewId, story: i64) {
    state.focus_pane(pane);
    let opened = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .is_some_and(|view| view.select(story));
    if opened {
        ask_story(state, pane, story);
    }
    state.layout_dirty = true;
}

/// Send the detail read, or say why there is none: a wall with no reads thread must not show "reading the
/// story…" for the life of the process.
fn ask_story(state: &mut State, pane: ViewId, story: i64) {
    match &state.backend {
        Some(backend) => backend.request(crate::data::Command::Story(story)),
        None => {
            if let Some(view) = state.pane_mut(pane).and_then(|pane| pane.news.as_mut()) {
                view.detail = Some(Detail::Failed(
                    "this wall has no reads thread: no credential is configured".to_string(),
                ));
            }
        }
    }
}

/// `back`: the list, exactly as it was left. The same function Escape reaches.
fn press_back(state: &mut State, pane: ViewId) {
    state.focus_pane(pane);
    if let Some(view) = state.pane_mut(pane).and_then(|pane| pane.news.as_mut()) {
        view.back();
    }
    state.layout_dirty = true;
}

/// `retry` after a detail that failed: one more read of the same story.
fn press_retry_detail(state: &mut State, pane: ViewId) {
    state.focus_pane(pane);
    let asked = state
        .pane_mut(pane)
        .and_then(|pane| pane.news.as_mut())
        .and_then(|view| view.retry_detail());
    if let Some(story) = asked {
        ask_story(state, pane, story);
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

/// What the detail's exit says (#137). A word, because a wall has no title bar to point at.
pub const BACK: &str = "back";
/// What a failed detail offers, and the list's own footer button says the same word.
pub const RETRY: &str = "retry";

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

/// The detail's provenance line: when the story was first learned and when it was last seen, both in
/// WIB, then `3 sources · rappler.com` — the count and the lead, from the wire.
pub fn provenance_line(story: &NewsStory) -> String {
    format!(
        "{} → {} · {}",
        wib(&story.first_seen),
        wib(&story.last_seen),
        sources(story)
    )
}

/// The Negara/Lokasi line: the country and the city, with a dash for either absence, and the confidence
/// as a **fact** — `0.72 confidence` — because the store's number is what the corpus said, not a filter
/// this client re-applies. Absent country is a dash and nothing else.
pub fn country_line(story: &NewsStory) -> String {
    match &story.country {
        None => "—".to_string(),
        Some(country) => format!(
            "{} · {} · {:.2} confidence",
            country.name,
            country.place.as_deref().unwrap_or("—"),
            country.confidence
        ),
    }
}

/// One article's facts, in the order they are read: an unclassified source is a **word** and comes first
/// (an operator has a question to answer), then the people, the tone with its sign, the CAMEO code and
/// the class. Every one is drawn only when the wire sent it; a measured `tone` of `0.0` is `+0.0` and
/// still draws (`INV-01`).
pub fn article_facts(article: &NewsArticle) -> String {
    let mut parts: Vec<String> = Vec::new();
    if article.flagged {
        parts.push("unclassified source".to_string());
    }
    if let Some(persons) = &article.persons
        && !persons.is_empty()
    {
        parts.push(persons.join(", "));
    }
    if let Some(tone) = article.tone {
        parts.push(format!("tone {tone:+.1}"));
    }
    if let Some(code) = &article.event_code {
        parts.push(format!("event {code}"));
    }
    if let Some(class) = article.quad_class {
        parts.push(format!("class {class}"));
    }
    if parts.is_empty() {
        return "—".to_string();
    }
    parts.join(" · ")
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

    fn article(domain: &str, seen: u32) -> NewsArticle {
        NewsArticle {
            id: i64::from(seen),
            title: format!("{domain} headline"),
            domain: domain.to_string(),
            url: format!("https://{domain}/x"),
            flagged: false,
            persons: None,
            tone: None,
            event_code: None,
            quad_class: None,
            seen_at: Utc.with_ymd_and_hms(2026, 11, 11, 9, seen, 0).unwrap(),
        }
    }

    fn detail(story: NewsStory, articles: Vec<NewsArticle>) -> NewsStoryDetail {
        NewsStoryDetail { story, articles }
    }

    /// **`back` returns to the page the operator left**, not to page 1 (#137): the detail is a mode over
    /// the list, so the tab, the page, the rows and the footer are exactly what they were.
    #[test]
    fn back_returns_to_the_page_the_operator_left() {
        let mut view = View::new();
        let first = view.ask();
        view.apply(first, &Ok(page("one", 128, 6, 1, true, false, 25)));
        let second = view.press_page(1).expect("page 2 exists");
        let page_two = page("two", 128, 6, 2, true, true, 25);
        let second_title = page_two.rows[0].title.clone();
        view.apply(second, &Ok(page_two));
        assert_eq!(view.page, 2);

        let id = view.stories[0].id;
        assert!(view.select(id), "a press opens the story");
        assert_eq!(view.selected, Some(id));
        assert_eq!(view.detail, Some(Detail::Reading));
        assert!(
            !view.select(id),
            "the story already open is not a second read"
        );
        let story = view.stories[0].clone();
        assert!(view.apply_detail(
            id,
            &Ok(Box::new(detail(story, vec![article("rappler.com", 19)])))
        ));

        assert!(view.back(), "back closes the story");
        assert_eq!(view.selected, None);
        assert_eq!(view.detail, None);
        assert_eq!(
            view.page, 2,
            "the list comes back on the page it was left on"
        );
        assert_eq!(view.stories[0].title, second_title);
        assert_eq!(
            view.pagination.map(|pagination| pagination.current_page),
            Some(2),
            "and the footer still says so"
        );
        assert!(!view.back(), "nothing open, nothing to close");
    }

    /// **A detail that lands after the operator left the story is discarded** (#137): the reply is
    /// compared with the selection, so `back` and a second press both make the slow story harmless.
    #[test]
    fn a_detail_for_a_story_the_operator_left_is_discarded() {
        let mut view = View::new();
        let asked = view.ask();
        view.apply(asked, &Ok(page("one", 128, 6, 1, true, false, 25)));
        let first = view.stories[0].clone();
        let second = view.stories[1].clone();

        view.select(first.id);
        // The operator presses another row before the first story lands.
        view.select(second.id);
        let late = detail(first.clone(), vec![article("rappler.com", 19)]);
        assert!(
            !view.apply_detail(first.id, &Ok(Box::new(late))),
            "a story the pane has left is not applied"
        );
        assert_eq!(
            view.detail,
            Some(Detail::Reading),
            "the pane is still waiting for the row it is on"
        );

        // And after `back`, nothing lands at all.
        view.back();
        let after_back = detail(second.clone(), vec![article("rappler.com", 19)]);
        assert!(!view.apply_detail(second.id, &Ok(Box::new(after_back))));
        assert_eq!(view.detail, None);
        assert_eq!(view.selected, None);
    }

    /// The detail's words, where absence is not zero (`INV-01`) and a measured zero still draws at one
    /// decimal: `tone +0.0`, persons joined, the CAMEO code and class only when the wire sent them.
    #[test]
    fn the_detail_words_name_the_facts_and_the_absences() {
        let mut flagged = article("malaysiasun.com", 41);
        flagged.flagged = true;
        assert_eq!(article_facts(&flagged), "unclassified source");

        let mut measured = article("rappler.com", 19);
        measured.persons = Some(vec!["Rina Kartika".into(), "Bayu Setiawan".into()]);
        measured.tone = Some(0.0);
        measured.event_code = Some("042".into());
        measured.quad_class = Some(1);
        assert_eq!(
            article_facts(&measured),
            "Rina Kartika, Bayu Setiawan · tone +0.0 · event 042 · class 1",
            "a measured zero is drawn with its sign, and the fields are in reading order"
        );

        let mut negative = article("en.antaranews.com", 30);
        negative.tone = Some(-3.42);
        assert_eq!(article_facts(&negative), "tone -3.4", "one decimal, signed");

        assert_eq!(
            article_facts(&article("thejakartapost.com", 22)),
            "—",
            "an article with no facts at all draws a dash rather than an empty line"
        );

        // The story's place line: a dash for an absent country, and the confidence as a fact.
        let mut story = measured_story();
        story.country = None;
        assert_eq!(country_line(&story), "—");
        story.country = Some(crate::data::types::NewsCountry {
            code: "MY".into(),
            name: "Malaysia".into(),
            place: None,
            confidence: 0.55,
        });
        assert_eq!(country_line(&story), "Malaysia · — · 0.55 confidence");
        story.country = Some(crate::data::types::NewsCountry {
            code: "ID".into(),
            name: "Indonesia".into(),
            place: Some("Jakarta".into()),
            confidence: 0.72,
        });
        assert_eq!(
            country_line(&story),
            "Indonesia · Jakarta · 0.72 confidence"
        );
    }

    fn measured_story() -> NewsStory {
        let at = Utc.with_ymd_and_hms(2026, 11, 11, 9, 19, 0).unwrap();
        NewsStory {
            id: 1103,
            title: "Fisheries agency detains a cargo ship".into(),
            source_count: 3,
            country: None,
            source: "rappler.com".into(),
            url: "https://rappler.com/2026/11/story-1103".into(),
            first_seen: at,
            last_seen: at,
        }
    }

    /// The provenance line is both clocks and the count, in WIB: what a reader needs to know who had the
    /// story first.
    #[test]
    fn the_provenance_line_is_wib_and_the_source_count() {
        let story = measured_story();
        assert_eq!(
            provenance_line(&story),
            "11 Nov 2026, 16.19 WIB → 11 Nov 2026, 16.19 WIB · 3 sources · rappler.com"
        );
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
