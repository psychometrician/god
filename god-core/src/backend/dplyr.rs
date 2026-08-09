//! A plan as dplyr, for reading rather than for running.
//!
//! Nothing executes this. It exists so that someone can ask what a sentence
//! would be in a language they already know, and get an answer they recognize
//! immediately:
//!
//! ```text
//! sales then keep where [region] is "West" then take 10
//!
//! sales |>
//!   filter(region == "West") |>
//!   head(10)
//! ```
//!
//! **This is what keeps a small grammar from becoming one more thing to learn.**
//!
//! One shape does not line up one-to-one: `keep where matching(...)` is a
//! condition in the grammar and a verb in dplyr, so it prints as `semi_join`
//! rather than as a `filter`. The sentence means the same thing; only the
//! spelling moves.
//! Any closed vocabulary covers most of what people do and never all of it, so
//! the question is not whether someone reaches the edge but what happens when
//! they do. Being handed the equivalent in the tool they already use turns the
//! edge into a doorway.
//!
//! A column is written bare here, the way dplyr writes it. Where a name could
//! not be written bare in R it is wrapped in backticks, which is R's own answer
//! and not the grammar's.

use super::Backend;
use crate::check::Schema;
use crate::plan::*;

pub struct Dplyr;

impl Backend for Dplyr {
    fn name(&self) -> &'static str {
        "dplyr"
    }

    // dplyr needs no help telling a new column from a rewritten one: `mutate`
    // covers both, exactly as `add` does.
    fn render(&self, plan: &Plan, _entering: &[Schema]) -> String {
        let mut lines = vec![plan.source.clone()];

        for step in &plan.steps {
            let call = match step {
                // A filtering join is a whole verb in dplyr rather than a
                // condition inside `filter`, which is the reason the grammar
                // only lets `matching` stand as the whole question.
                Step::Keep { condition, .. } => match filtering_join(condition) {
                    Some((other, by, negated)) => {
                        let keys: Vec<String> = by.iter().map(|k| k.text.clone()).collect();
                        format!(
                            "{}({}, by = join_by({}))",
                            if negated { "anti_join" } else { "semi_join" },
                            other.text,
                            keys.join(", ")
                        )
                    }
                    None => format!("filter({})", expr(condition)),
                },

                Step::Pick { names, all_but, .. } => {
                    let listed: Vec<String> = names.iter().map(|n| name(&n.text)).collect();
                    if *all_but {
                        format!("select(!c({}))", listed.join(", "))
                    } else {
                        format!("select({})", listed.join(", "))
                    }
                }

                Step::Add { values, by, .. } => {
                    let mut args: Vec<String> = values
                        .iter()
                        .map(|v| format!("{} = {}", name(&v.name.text), expr(&v.value)))
                        .collect();
                    if !by.is_empty() {
                        args.push(format!(".by = {}", columns(by)));
                    }
                    format!("mutate({})", args.join(", "))
                }

                Step::Summarize { values, by, .. } => {
                    let mut args: Vec<String> = values
                        .iter()
                        .map(|v| format!("{} = {}", name(&v.name.text), expr(&v.value)))
                        .collect();
                    if !by.is_empty() {
                        args.push(format!(".by = {}", columns(by)));
                    }
                    format!("summarise({})", args.join(", "))
                }

                Step::Sort { keys, .. } => {
                    let ordered: Vec<String> = keys
                        .iter()
                        .map(|k| {
                            if k.descending {
                                format!("desc({})", name(&k.column.text))
                            } else {
                                name(&k.column.text)
                            }
                        })
                        .collect();
                    format!("arrange({})", ordered.join(", "))
                }

                Step::AddRows { other, .. } => format!("bind_rows({})", other.text),

                Step::DropDuplicates { .. } => "distinct()".to_string(),

                Step::Rename { values, .. } => {
                    let pairs: Vec<String> = values
                        .iter()
                        .map(|v| match &v.value {
                            Expr::Column(from) => format!("{} = {}", v.name.text, from.text),
                            other => format!("{} = {}", v.name.text, expr(other)),
                        })
                        .collect();
                    format!("rename({})", pairs.join(", "))
                }

                Step::DropMissing { names, .. } => {
                    if names.is_empty() {
                        "drop_na()".to_string()
                    } else {
                        let listed: Vec<String> =
                            names.iter().map(|n| n.text.clone()).collect();
                        format!("drop_na({})", listed.join(", "))
                    }
                }

                Step::FillMissing { values, .. } => {
                    let pairs: Vec<String> = values
                        .iter()
                        .map(|v| {
                            format!(
                                "{} = coalesce({}, {})",
                                v.name.text,
                                v.name.text,
                                expr(&v.value)
                            )
                        })
                        .collect();
                    format!("mutate({})", pairs.join(", "))
                }

                // **The two reshaping verbs are where this backend earns its
                // keep as a reading aid.** One clause here becomes a small
                // family of coupled arguments over there, and seeing the two
                // side by side is the clearest statement of what the grammar
                // bought. Where a pattern has more than one separator, tidyr's
                // own answer is a regex, so that is what gets written.
                Step::Lengthen { names, all_but, name: pattern, value, resolved, .. } => {
                    let listed: Vec<String> = names.iter().map(|n| name(&n.text)).collect();
                    let cols = if *all_but {
                        format!("!c({})", listed.join(", "))
                    } else {
                        format!("c({})", listed.join(", "))
                    };
                    let mut args = vec![format!("cols = {cols}")];

                    let pieces: Vec<String> = pattern
                        .parts
                        .iter()
                        .map(|p| match p {
                            PatternPart::Named(n) => text(n),
                            PatternPart::Value => text(".value"),
                        })
                        .collect();
                    if pieces.len() == 1 {
                        args.push(format!("names_to = {}", pieces[0]));
                    } else {
                        args.push(format!("names_to = c({})", pieces.join(", ")));
                        let between = &pattern.literals[1..pattern.literals.len() - 1];
                        if between.iter().all(|s| s == &between[0]) {
                            args.push(format!("names_sep = {}", text(&between[0])));
                        } else {
                            args.push(format!(
                                "names_pattern = {}",
                                text(&regex_for(pattern))
                            ));
                        }
                    }
                    // `.value` names the value columns from the data, so there
                    // is nothing left for `values_to` to say.
                    if !pattern.has_value() {
                        let held = value
                            .as_ref()
                            .map(|v| v.text.clone())
                            .or_else(|| {
                                resolved.as_ref().and_then(|r| r.value_columns.first().cloned())
                            })
                            .unwrap_or_else(|| "value".into());
                        args.push(format!("values_to = {}", text(&held)));
                    }
                    format!("pivot_longer({})", args.join(", "))
                }

                Step::Widen { name: pattern, value, by, missing, .. } => {
                    let mut args = Vec::new();
                    if !by.is_empty() {
                        args.push(format!("id_cols = {}", columns(by)));
                    }
                    let pieces: Vec<String> = pattern
                        .named_parts()
                        .into_iter()
                        .map(name)
                        .collect();
                    args.push(if pieces.len() == 1 {
                        format!("names_from = {}", pieces[0])
                    } else {
                        format!("names_from = c({})", pieces.join(", "))
                    });
                    if pieces.len() > 1 {
                        let between = &pattern.literals[1..pattern.literals.len() - 1];
                        if between.iter().all(|s| s == &between[0]) {
                            args.push(format!("names_sep = {}", text(&between[0])));
                        } else {
                            args.push(format!("names_glue = {}", text(&glue_for(pattern))));
                        }
                    }
                    // An aggregate in `value` is what answers "two rows want one
                    // cell", and over here that is a separate argument holding a
                    // separate function.
                    match aggregate_of(value) {
                        Some((fname, inner)) => {
                            args.push(format!("values_from = {}", expr(&inner)));
                            args.push(format!("values_fn = {fname}"));
                        }
                        None => args.push(format!("values_from = {}", expr(value))),
                    }
                    if let Some(filler) = missing {
                        args.push(format!("values_fill = {}", expr(filler)));
                    }
                    format!("pivot_wider({})", args.join(", "))
                }

                Step::Join { other, by, unmatched, .. } => {
                    let keys: Vec<String> =
                        by.iter().map(|k| k.text.clone()).collect();
                    let verb = match unmatched {
                        Unmatched::This => "left_join",
                        Unmatched::None => "inner_join",
                        Unmatched::Both => "full_join",
                    };
                    format!(
                        "{verb}({}, by = join_by({}))",
                        other.text,
                        keys.join(", ")
                    )
                }
                Step::Take { count, by, .. } => {
                    if by.is_empty() {
                        format!("head({count})")
                    } else {
                        let groups: Vec<String> =
                            by.iter().map(|n| n.text.clone()).collect();
                        format!("slice_head(n = {count}, by = c({}))", groups.join(", "))
                    }
                }
            };
            lines.push(format!("  {call}"));
        }

        lines.join(" |>\n")
    }
}

/// A pattern as tidyr's `names_pattern`, which is a regex.
///
/// **Only reached where the separators differ**, since `names_sep` covers the
/// rest. It is worth seeing: the moment a name has two different separators in
/// it, tidyr's answer stops being readable, and this is the backend that shows
/// the reader what they are not having to write (§14.1).
fn regex_for(pattern: &Pattern) -> String {
    let escape = |s: &str| {
        s.chars()
            .map(|c| {
                if "\\^$.|?*+()[]{}".contains(c) {
                    format!("\\{c}")
                } else {
                    c.to_string()
                }
            })
            .collect::<String>()
    };
    let mut out = format!("^{}", escape(&pattern.literals[0]));
    for i in 0..pattern.parts.len() {
        out.push_str("(.+)");
        out.push_str(&escape(&pattern.literals[i + 1]));
    }
    out.push('$');
    out
}

/// A pattern as tidyr's `names_glue`, which spells pieces the same way.
fn glue_for(pattern: &Pattern) -> String {
    let written = pattern.text();
    written.trim_matches('"').to_string()
}

/// The aggregate wrapping a single column, where there is one.
///
/// `value average([answer])` is one clause here and two arguments there, so the
/// two halves have to come apart again to be written out.
fn aggregate_of(value: &Expr) -> Option<(&'static str, Expr)> {
    let Expr::Call { name: fname, args, .. } = value else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let r = match fname.as_str() {
        "total" => "sum",
        "average" => "mean",
        "median" => "median",
        "smallest" => "min",
        "largest" => "max",
        "first" => "dplyr::first",
        "last" => "dplyr::last",
        "unique_count" => "dplyr::n_distinct",
        _ => return None,
    };
    Some((r, args[0].clone()))
}

/// A column, bare where R allows it and in backticks where it does not.
fn name(text: &str) -> String {
    let bare = !text.is_empty()
        && text.chars().next().is_some_and(|c| c.is_alphabetic())
        && text.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.');
    if bare {
        text.to_string()
    } else {
        format!("`{}`", text.replace('`', "\\`"))
    }
}

fn columns(names: &[Name]) -> String {
    if names.len() == 1 {
        name(&names[0].text)
    } else {
        format!(
            "c({})",
            names.iter().map(|n| name(&n.text)).collect::<Vec<_>>().join(", ")
        )
    }
}

fn text(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn expr(e: &Expr) -> String {
    match e {
        Expr::Column(n) => name(&n.text),
        Expr::Text { value, .. } => text(value),
        Expr::Whole { value, .. } => value.to_string(),
        Expr::Decimal { value, .. } => {
            let s = format!("{value}");
            if s.contains('.') || s.contains('e') {
                s
            } else {
                format!("{s}.0")
            }
        }
        Expr::Truth { value, .. } => if *value { "TRUE" } else { "FALSE" }.to_string(),
        Expr::Missing { .. } => "NA".to_string(),

        Expr::Arithmetic { op, left, right, .. } => {
            format!("({} {} {})", expr(left), op, expr(right))
        }
        Expr::Compare { op, left, right, .. } => {
            let symbol = match op {
                Compare::Is => "==",
                Compare::IsNot => "!=",
                Compare::Less => "<",
                Compare::LessOrEqual => "<=",
                Compare::Greater => ">",
                Compare::GreaterOrEqual => ">=",
            };
            format!("({} {symbol} {})", expr(left), expr(right))
        }
        Expr::Logic { op, left, right, .. } => {
            let symbol = match op {
                Logic::And => "&",
                Logic::Or => "|",
            };
            format!("({} {symbol} {})", expr(left), expr(right))
        }
        Expr::Not { inner, .. } => format!("!{}", expr(inner)),
        Expr::In { left, set, negated, .. } => {
            let values: Vec<String> = set.iter().map(expr).collect();
            let test = format!("({} %in% c({}))", expr(left), values.join(", "));
            if *negated {
                format!("!{test}")
            } else {
                test
            }
        }
        Expr::IsMissing { inner, negated, .. } => {
            let test = format!("is.na({})", expr(inner));
            if *negated {
                format!("!{test}")
            } else {
                test
            }
        }
        // Unreachable in a checked plan: `matching` may only stand as a whole
        // `keep` condition, and the step above renders that case as the verb
        // dplyr actually has.
        Expr::Matching { other, .. } => format!("semi_join({})", other.text),
        // dplyr calls competition ranking `min_rank`, which names the
        // implementation rather than the idea. `desc()` is its own word for a
        // reversed ordering, and is the same one `arrange` takes.
        Expr::Window { kind, key, .. } => match (kind, key) {
            (Window::Rank, Some(k)) if k.descending => {
                format!("min_rank(desc({}))", name(&k.column.text))
            }
            (Window::Rank, Some(k)) => format!("min_rank({})", name(&k.column.text)),
            (Window::Rank, None) => "min_rank()".to_string(),
            (Window::RowNumber, _) => "row_number()".to_string(),
        },
        // base R, so the printed pipeline needs nothing installed beyond dplyr
        // itself. `fixed = TRUE` because the value is text a person typed, not
        // a regular expression.
        Expr::TextTest { op, left, value, .. } => match op {
            TextOp::Starts => format!("startsWith({}, {})", expr(left), expr(value)),
            TextOp::Ends => format!("endsWith({}, {})", expr(left), expr(value)),
            TextOp::Contains => {
                format!("grepl({}, {}, fixed = TRUE)", expr(value), expr(left))
            }
        },
        Expr::ColumnValue { .. } => "value".to_string(),
        Expr::ColumnKind { .. } => "kind".to_string(),
        // dplyr spells the arms with a formula, which is the shape god could
        // not borrow: `~` has no Python equivalent, and a form only one host can
        // write is not a form. This is what it looks like over there.
        Expr::When { arms, otherwise, .. } => {
            let mut parts: Vec<String> = arms
                .iter()
                .map(|(test, value)| format!("{} ~ {}", expr(test), expr(value)))
                .collect();
            if let Some(fallback) = otherwise {
                parts.push(format!(".default = {}", expr(fallback)));
            }
            format!("case_when({})", parts.join(", "))
        }
        Expr::ColumnName { .. } => "name".to_string(),
        Expr::Call { name: fname, args, .. } => call(fname, args),
    }
}

/// The table and key of a `keep` that is really a filtering join, and whether it
/// is the anti one.
fn filtering_join(condition: &Expr) -> Option<(&Name, &[Name], bool)> {
    match condition {
        Expr::Matching { other, by, .. } => Some((other, by, false)),
        Expr::Not { inner, .. } => match inner.as_ref() {
            Expr::Matching { other, by, .. } => Some((other, by, true)),
            _ => None,
        },
        _ => None,
    }
}

/// How dplyr spells each of the grammar's functions.
///
/// The aggregations take `na.rm = TRUE`, which is a decision rather than a
/// convenience: the grammar's `total` is the total of the values that are there,
/// and R's `sum` of a vector holding one missing value is missing. Writing it out
/// here keeps the printed code doing what the sentence said.
fn call(fname: &str, args: &[Expr]) -> String {
    let arg = |i: usize| args.get(i).map(expr).unwrap_or_default();
    match fname {
        "total" => format!("sum({}, na.rm = TRUE)", arg(0)),
        "average" => format!("mean({}, na.rm = TRUE)", arg(0)),
        "median" => format!("median({}, na.rm = TRUE)", arg(0)),
        "smallest" => format!("min({}, na.rm = TRUE)", arg(0)),
        "largest" => format!("max({}, na.rm = TRUE)", arg(0)),
        "first" => format!("first({})", arg(0)),
        "last" => format!("last({})", arg(0)),
        "unique_count" => format!("n_distinct({})", arg(0)),
        "row_count" => "n()".to_string(),
        // dplyr borrowed SQL's word for this one, so the translation is the
        // term of art in both directions.
        "first_present" => format!(
            "coalesce({})",
            args.iter().map(expr).collect::<Vec<_>>().join(", ")
        ),
        // lubridate is named explicitly rather than assumed to be attached,
        // because `year` and `month` are not in base R and a reader copying this
        // line needs to know where they come from.
        "year" => format!("lubridate::year({})", arg(0)),
        "month" => format!("lubridate::month({})", arg(0)),
        "day" => format!("lubridate::day({})", arg(0)),
        "hour" => format!("lubridate::hour({})", arg(0)),
        // Monday is 1 here too, which is what `week_start = 1` says.
        "weekday" => format!("lubridate::wday({}, week_start = 1)", arg(0)),
        // `cumsum` down a sorted column is the running total; `lag` and `lead`
        // keep dplyr's own names, which is what a reader over there recognizes
        // even though the grammar refuses those two words for being jargon.
        "running_total" => format!("cumsum({})", arg(0)),
        "previous" => format!("lag({})", arg(0)),
        "following" => format!("lead({})", arg(0)),
        "to_number" => format!("as.numeric({})", arg(0)),
        "to_whole" => format!("as.integer({})", arg(0)),
        "to_text" => format!("as.character({})", arg(0)),
        "to_date" => format!("as.Date({})", arg(0)),
        "trim" => format!("trimws({})", arg(0)),
        "characters" => format!("nchar({})", arg(0)),
        // `fixed = TRUE` because the grammar's word looks for text rather than a
        // pattern, and `gsub` would read it as a regular expression otherwise.
        "replace_text" => format!("gsub({}, {}, {}, fixed = TRUE)", arg(1), arg(2), arg(0)),
        // stringr rather than base, because base R's answer is a list you then
        // have to index, and this backend exists to be recognized rather than to
        // be minimal.
        "split_text" => {
            format!("stringr::str_split_i({}, stringr::fixed({}), {})", arg(0), arg(1), arg(2))
        }
        "between" => format!("between({}, {}, {})", arg(0), arg(1), arg(2)),
        "lower" => format!("tolower({})", arg(0)),
        "upper" => format!("toupper({})", arg(0)),
        other => unreachable!("`{other}` reached the dplyr backend without a spelling"),
    }
}
