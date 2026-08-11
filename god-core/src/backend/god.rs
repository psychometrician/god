//! A plan written back out as the grammar itself.
//!
//! **This is the backend that checks the other end of the pipe.** Reading text
//! into a plan and writing a plan back into text are inverses, so a pipeline put
//! through both has to come out as the same pipeline. That is a property a test
//! can hold the parser to on any sentence at all, rather than on the handful
//! someone thought to write down — and it fails loudly when the parser quietly
//! drops a clause, which is the failure that is otherwise invisible.
//!
//! It earns its place twice over: a plan that prints as text is a plan that can
//! be saved in a config file, stored in a column, or handed between machines.

use super::Backend;
use crate::check::Schema;
use crate::plan::*;

pub struct God;

impl Backend for God {
    fn name(&self) -> &'static str {
        "god"
    }

    fn render(&self, plan: &Plan, _entering: &[Schema]) -> String {
        let mut lines = vec![plan.source.clone()];
        for step in &plan.steps {
            lines.push(format!("  then {}", step_text(step)));
        }
        lines.join("\n")
    }
}

/// One step, in the words it is written with.
///
/// **The ladder drawing labels its bands with this, rather than with a spelling
/// of its own.** Two renderings of the same step is two things to keep in step,
/// and the one that drifts is the one nobody runs the round trip against.
pub(crate) fn step_text(step: &Step) -> String {
    match step {
        Step::Keep { condition, .. } => format!("keep where {}", expr(condition)),

        Step::Pick { names, all_but, .. } => format!(
            "pick {}{}",
            if *all_but { "all_but " } else { "" },
            columns(names)
        ),

        Step::Add { values, by, .. } => {
            format!("add {}{}", assignments(values), grouping(by))
        }

        Step::Summarize { values, by, .. } => {
            format!("summarize {}{}", assignments(values), grouping(by))
        }

        Step::Sort { keys, .. } => {
            let written: Vec<String> = keys
                .iter()
                .map(|k| {
                    format!(
                        "[{}]{}",
                        k.column.text,
                        if k.descending { " descending" } else { "" }
                    )
                })
                .collect();
            format!("sort {}", written.join(", "))
        }

        Step::Take { count, by, .. } => {
            format!("take {count}{}", grouping(by))
        }

        Step::AddRows { other, .. } => format!("add_rows {}", other.text),

        Step::DropDuplicates { .. } => "drop_duplicates".to_string(),

        Step::Rename { values, .. } => format!("rename {}", assignments(values)),

        Step::DropMissing { names, .. } => {
            if names.is_empty() {
                "drop_missing".to_string()
            } else {
                format!("drop_missing {}", columns(names))
            }
        }

        Step::FillMissing { values, .. } => {
            format!("fill_missing {}", assignments(values))
        }

        Step::Lengthen { names, all_but, condition, name, value, .. } => {
            let chosen = match condition {
                Some(c) => format!("where {}", expr(c)),
                None => format!(
                    "{}{}",
                    if *all_but { "all_but " } else { "" },
                    columns(names)
                ),
            };
            // The naming clause is printed only where it says something
            // the default does not, so a sentence that took the defaults
            // comes back out as the sentence that was written.
            let mut said = Vec::new();
            if name.quoted || name.named_parts() != ["name"] {
                said.push(format!("name {}", name.text()));
            }
            if let Some(v) = value {
                said.push(format!("value [{}]", v.text));
            }
            if said.is_empty() {
                format!("lengthen {chosen}")
            } else {
                format!("lengthen {chosen} as {}", said.join(", "))
            }
        }

        Step::Widen { name, value, by, missing, giving, .. } => {
            // `by` is always printed, even where the caller left it out,
            // because the checker has settled it by then and what was
            // assumed on someone's behalf is worth their seeing. Same
            // reason `join` prints the key it worked out.
            let mut out = format!("widen name {}, value {}", name.text(), expr(value));
            if !by.is_empty() {
                out.push_str(&format!(" by {}", columns(by)));
            }
            if let Some(filler) = missing {
                out.push_str(&format!(" missing {}", expr(filler)));
            }
            if !giving.is_empty() {
                out.push_str(&format!(" giving {}", columns(giving)));
            }
            out
        }

        Step::Join { other, by, unmatched, .. } => {
            // The key is always printed, even where the caller left it
            // out, because by the time a plan reaches a backend the
            // checker has settled it. Printing the settled sentence is
            // how someone sees what was assumed on their behalf.
            let matched = if by.is_empty() {
                String::new()
            } else {
                format!(" by {}", columns(by))
            };
            let survivors = if *unmatched == Unmatched::This {
                String::new()
            } else {
                format!(" unmatched \"{}\"", unmatched.word())
            };
            format!("join {}{matched}{survivors}", other.text)
        }
    }
}

fn columns(names: &[Name]) -> String {
    format!(
        "[{}]",
        names.iter().map(|n| n.text.clone()).collect::<Vec<_>>().join(", ")
    )
}

fn assignments(values: &[Named]) -> String {
    values
        .iter()
        .map(|v| format!("[{}] as {}", v.name.text, expr(&v.value)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn grouping(by: &[Name]) -> String {
    if by.is_empty() {
        String::new()
    } else {
        format!(" by {}", columns(by))
    }
}

fn expr(e: &Expr) -> String {
    match e {
        Expr::Column(n) => format!("[{}]", n.text),
        Expr::Text { value, .. } => format!("\"{value}\""),
        Expr::Whole { value, .. } => value.to_string(),
        Expr::Decimal { value, .. } => {
            let s = format!("{value}");
            if s.contains('.') {
                s
            } else {
                format!("{s}.0")
            }
        }
        Expr::Truth { value, .. } => if *value { "yes" } else { "no" }.to_string(),
        Expr::Missing { .. } => "missing".to_string(),

        Expr::Arithmetic { op, left, right, .. } => {
            format!("({} {} {})", expr(left), op, expr(right))
        }
        Expr::Compare { op, left, right, .. } => {
            let word = match op {
                Compare::Is => "is",
                Compare::IsNot => "is not",
                Compare::Less => "<",
                Compare::LessOrEqual => "<=",
                Compare::Greater => ">",
                Compare::GreaterOrEqual => ">=",
            };
            format!("({} {word} {})", expr(left), expr(right))
        }
        Expr::Logic { op, left, right, .. } => {
            let word = match op {
                Logic::And => "and",
                Logic::Or => "or",
            };
            format!("({} {word} {})", expr(left), expr(right))
        }
        Expr::Not { inner, .. } => format!("(not {})", expr(inner)),
        Expr::In { left, set, negated, .. } => format!(
            "({} {}in {{{}}})",
            expr(left),
            if *negated { "not " } else { "" },
            set.iter().map(expr).collect::<Vec<_>>().join(", ")
        ),
        Expr::IsMissing { inner, negated, .. } => format!(
            "({} is {}missing)",
            expr(inner),
            if *negated { "not " } else { "" }
        ),
        Expr::TextTest { op, left, value, .. } => {
            format!("({} {} {})", expr(left), op.word(), expr(value))
        }
        Expr::When { arms, otherwise, .. } => {
            let mut parts: Vec<String> = arms
                .iter()
                .flat_map(|(test, value)| [expr(test), expr(value)])
                .collect();
            if let Some(fallback) = otherwise {
                parts.push(format!("otherwise {}", expr(fallback)));
            }
            format!("when({})", parts.join(", "))
        }
        Expr::ColumnName { .. } => "name".to_string(),
        Expr::ColumnValue { .. } => "value".to_string(),
        Expr::ColumnKind { .. } => "kind".to_string(),
        Expr::Window { kind, key, .. } => match key {
            Some(k) => format!(
                "{}([{}]{})",
                kind.word(),
                k.column.text,
                if k.descending { " descending" } else { "" }
            ),
            None => format!("{}()", kind.word()),
        },
        Expr::Call { name, args, .. } => {
            format!("{name}({})", args.iter().map(expr).collect::<Vec<_>>().join(", "))
        }
        // The key is always printed, even where the caller left it out, because
        // by the time a plan reaches a backend the checker has settled it.
        // Printing the settled sentence is how someone sees what was assumed on
        // their behalf, which is the same reason `join` prints its own.
        Expr::Matching { other, by, .. } => {
            if by.is_empty() {
                format!("matching({})", other.text)
            } else {
                format!("matching({}, by {})", other.text, columns(by))
            }
        }
    }
}
