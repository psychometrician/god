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
//! Layout happens once, in cells of a fixed grid, and an emitter turns cells
//! into output. There is exactly one emitter today and it writes text; a second
//! one that draws a picture is arithmetic over the same scene rather than a
//! second layout, which is the only arrangement where the two cannot drift.

pub mod ladder;
pub mod scene;
pub mod svg;
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

/// The same ladder, drawn.
///
/// **The same scene, so the two cannot disagree.** Whatever the text says about
/// where a column sits, the picture puts it there, because neither of them
/// decides: the layout was settled before either was asked.
pub fn picture(plan: &Plan, source: &str, schema: &Schema, others: &Tables) -> String {
    svg::render(&ladder::build(plan, source, schema, others))
}

/// The two ways to look at the same drawing.
///
/// Named here so nothing else keeps a copy — a launcher offering a third would
/// be offering something that does not exist.
pub fn ways() -> &'static [&'static str] {
    &["text", "svg"]
}
