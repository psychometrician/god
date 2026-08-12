//! A pipeline drawn, rather than executed or translated.
//!
//! **This is not a query plan and it is not trying to be one.** A query planner
//! draws what an optimizer decided, which is worth seeing when an optimizer is
//! yours; here the pushdowns belong to the engine underneath, so the plan before
//! and the plan after would be the same plan. Two views of it already exist —
//! the sentence written back out, and the query, which is one clause per step.
//!
//! What is missing from all of those is the **table between the steps**, and
//! that is what this draws.
//!
//! ```text
//!   plan + the columns each step is handed  ──▶  scene  ──▶  text
//!                                                   │
//!                                          cells, never pixels
//! ```
//!
//! **The reading happens once and the placing happens twice.** What a step
//! makes, what it takes away, which table arrives and what became of the rows
//! are facts about the sentence, worked out in `ladder::read`. Where those facts
//! sit is a layout, and there are two: a ladder of characters for a terminal,
//! and a diagram of bars and chips for a page. Two readings of one sentence is
//! how the drawings would start disagreeing about it; two layouts is just two
//! layouts.

pub mod diagram;
pub mod ladder;
pub mod scene;
pub mod text;

pub use scene::{Ink, Scene};

use crate::check::{Schema, Tables};
use crate::plan::Plan;

/// The pipeline as a ladder, in plain text.
///
/// Never fails. A sentence the checker refuses is drawn as far as it checked,
/// with the refusal under the words that caused it, because how far you got is
/// the thing worth knowing and it is exactly what an error message alone leaves
/// out.
pub fn ladder(plan: &Plan, source: &str, schema: &Schema, others: &Tables) -> String {
    text::render(&ladder::build(plan, source, schema, others))
}

/// The same pipeline as a diagram.
///
/// **Shape rather than characters.** A table is a bar holding one chip per
/// column, so its width is its width: a `summarize` narrows the page and a
/// `join` widens it, and a reader sees that before reading a word. The ladder
/// cannot say it, because a grid of characters can only look like a grid of
/// characters.
pub fn picture(plan: &Plan, source: &str, schema: &Schema, others: &Tables) -> String {
    diagram::render(&ladder::read(plan, source, schema, others), others)
}

/// The two ways to look at the same drawing.
///
/// Named here so nothing else keeps a copy — a launcher offering a third would
/// be offering something that does not exist.
pub fn ways() -> &'static [&'static str] {
    &["text", "svg"]
}
