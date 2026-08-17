//! Turning a checked plan into something else.
//!
//! **Every backend produces text, and that is the whole interface.** What
//! differs is who reads it. An *executing* backend writes a query for an engine
//! to run, and the answers come back. A *printing* backend writes the same
//! pipeline in a language someone already knows, and a person reads it.
//!
//! The printing backends are not documentation and they are not a nicety. A
//! small grammar covers most of what people do and never all of it, so anyone
//! who adopts one needs a way out the day they hit the edge. Being able to ask
//! *"what would this be in dplyr?"* means learning this grammar makes the tools
//! you already use easier rather than becoming one more thing to keep in your
//! head. The escape hatch is the point rather than an admission.
//!
//! They cost almost nothing, too, which is the argument for having several: once
//! the plan exists, a backend is a walk over it.

use crate::check::Schema;
use crate::diagnostic::Diagnostic;
use crate::plan::{Expr, Plan};

pub mod dplyr;
pub mod god;
pub mod pandas;
pub mod polars;
pub mod pyspark;
pub mod sql;

pub trait Backend {
    /// The word a caller writes to ask for this one.
    fn name(&self) -> &'static str;

    /// What this backend cannot write, and why.
    ///
    /// **Almost every backend refuses nothing, which is why this has an answer
    /// by default.** A sentence the grammar accepts is a sentence every target
    /// should be able to express, and where one cannot, §3.1 has already
    /// settled what to do: refuse, and name what to write instead. A quiet
    /// difference between engines is the one outcome the whole design is
    /// against, because it is the one nobody notices.
    ///
    /// It runs after the checker and before anything is rendered, so a refusal
    /// here reads exactly like a refusal from the gate.
    fn refuses(&self, _plan: &Plan) -> Option<Diagnostic> {
        None
    }

    /// `entering` holds the columns each step is handed, which a backend needs
    /// when the grammar says one thing and the target spells it two ways. Most
    /// backends ignore it.
    fn render(&self, plan: &Plan, entering: &[Schema]) -> String;
}

/// Every backend the grammar has.
pub fn all() -> Vec<Box<dyn Backend>> {
    vec![
        Box::new(sql::Sql),
        Box::new(sql::SparkSql),
        Box::new(dplyr::Dplyr),
        Box::new(pandas::Pandas),
        Box::new(polars::Polars),
        Box::new(pyspark::PySpark),
        Box::new(god::God),
    ]
}

pub fn find(name: &str) -> Option<Box<dyn Backend>> {
    all().into_iter().find(|b| b.name() == name)
}

pub fn names() -> Vec<&'static str> {
    all().iter().map(|b| b.name()).collect()
}

/// How far `previous` or `following` was told to look, as a target writes it.
///
/// **Every one of the five spells the offset as a plain trailing number** —
/// `lag(x, 12)`, `shift(12)`, `F.lag(x, 12)` — so what differs between them is
/// the punctuation around it and not the thing itself. It lives here rather
/// than five times over because a default that drifts between backends is the
/// quiet disagreement §3.1 exists to refuse.
///
/// Empty when nobody asked, so the common sentence keeps rendering as the short
/// call it already did. The checker has already refused anything but a whole
/// number of at least one, so this never has to decide what a bad one means.
pub fn step(args: &[Expr]) -> String {
    match args.get(1) {
        Some(Expr::Whole { value, .. }) => format!(", {value}"),
        _ => String::new(),
    }
}

/// The same, for a target that writes the offset on its own: `.shift(12)`.
///
/// `sign` is what a backward step looks like there — polars and pandas both
/// walk the other way on a negative — so `following` passes `-`.
pub fn step_alone(args: &[Expr], sign: &str) -> String {
    match args.get(1) {
        Some(Expr::Whole { value, .. }) => format!("{sign}{value}"),
        _ => format!("{sign}1"),
    }
}
