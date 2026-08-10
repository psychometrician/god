//! god — a grammar of data.
//!
//! One sentence, written once, meaning the same thing wherever it is read.
//!
//! ```text
//! sales
//!   then keep where [region] is "West"
//!   then add [margin] as [revenue] - [cost]
//!   then summarize [margin] as total([margin]), [orders] as row_count() by [product]
//!   then sort [margin] descending
//!   then take 10
//! ```
//!
//! **Data manipulation has many flavors and one core.** Every tool in common use
//! can filter rows, derive a column, group and total, order and truncate; what
//! differs between them is spelling, and the spellings do not agree. This crate
//! is one small vocabulary for that core, with the property that matters more
//! than the words themselves: **the parts combine without exceptions.** A
//! vocabulary is easy to learn because its rules for joining are regular, not
//! because it is short.
//!
//! The path a sentence takes:
//!
//! ```text
//!   text ──▶ parse ──▶ plan ──▶ check ──▶ backend
//!                                 │
//!                        every refusal, before
//!                        anything is executed
//! ```
//!
//! **There is one parser and one checker**, so there is nothing for two
//! implementations to disagree about. A host language — R, Python, a SQL cell —
//! carries the text in and the table back, and decides nothing.
//!
//! A backend turns the checked plan into text. Some of that text is for an
//! engine to run; some is the same pipeline written in a language the reader
//! already knows, which is how a small grammar stays a door rather than a wall.

pub mod backend;
pub mod check;
pub mod diagnostic;
pub mod parse;
pub mod plan;
pub mod seats;
pub mod vocabulary;

pub use check::{Schema, Type};
pub use diagnostic::Diagnostic;
pub use plan::Plan;

/// A pipeline that has been read, checked, and written out for a backend.
pub struct Compiled {
    /// What the backend produced. A query, or a pipeline in another language.
    pub text: String,
    /// The columns the pipeline ends with, in order.
    pub schema: Schema,
    /// Anything the grammar decided that the caller did not say. Never fatal,
    /// and never silent: an unreported choice is the same defect as a dropped
    /// clause.
    pub assumptions: Vec<Diagnostic>,
}

/// Read a pipeline, check it against the table it will run on, and write it out.
///
/// **Checking is not optional and there is no way past it.** A backend is only
/// ever handed a plan that has already been found to mean something, which is
/// what makes a refusal fatal rather than advisory: nothing has run yet, so
/// stopping costs nothing and produces no half-answer.
pub fn compile(source: &str, schema: &Schema, backend_name: &str) -> Result<Compiled, Diagnostic> {
    compile_tables(source, schema, &check::Tables::empty(), backend_name)
}

/// The same, for a pipeline that names more than one table.
///
/// Only `join` does, and it is the first verb that can. The other tables are
/// passed separately from the head table because the head is not optional and
/// these are: a pipeline without a join needs none of this and should not have
/// to say so.
pub fn compile_tables(
    source: &str,
    schema: &Schema,
    others: &check::Tables,
    backend_name: &str,
) -> Result<Compiled, Diagnostic> {
    let plan = parse::parse(source)?;
    let checked = check::check_tables(&plan, schema, others)?;

    let Some(backend) = backend::find(backend_name) else {
        let suggestion = diagnostic::nearest(backend_name, backend::names())
            .map(|s| format!(" Did you mean `{s}`?"))
            .unwrap_or_default();
        return Err(Diagnostic {
            kind: diagnostic::Kind::Illegal,
            message: format!(
                "there is no backend called `{backend_name}`.{suggestion} There is: {}",
                backend::names().join(", ")
            ),
            span: None,
        });
    };

    // **Asked after the checker and before anything is written.** A sentence the
    // grammar accepts should be one every target can express, and where one
    // cannot, the answer is a refusal rather than a query that says something
    // close. Reaching this point means the sentence is legal; what it means here
    // is that this engine cannot say it.
    if let Some(refusal) = backend.refuses(&checked.plan) {
        return Err(refusal);
    }

    Ok(Compiled {
        text: backend.render(&checked.plan, &checked.entering),
        schema: checked.schema,
        assumptions: checked.assumptions,
    })
}
