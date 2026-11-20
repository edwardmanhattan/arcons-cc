//! **The time field, without a window** — the parts of #276 that are decisions rather than pixels.
//!
//! ADR-0009, decision 5: the calendar and date-input semantics come from Vercel Labs' UI SDK as *ideas*
//! (`plain-date.ts`, `calendar-model.ts`, `date-input-model.ts`), ported to Rust. Nothing here knows what a
//! `TextInput` is, so every rule below is testable without a frame.

// `civil`, `calendar`, `segment`, `paste` and `zoned` land with #276's second commit.
