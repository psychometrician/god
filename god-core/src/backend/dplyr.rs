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

        for (i, step) in plan.steps.iter().enumerate() {
            let call = match step {
                // A filtering join is a whole verb in dplyr rather than a
                // condition inside `filter`, which is the reason the grammar
                // only lets `matching` stand as the whole question.
                Step::Keep { condition, .. } => match filtering_join(condition) {
                    Some((other, by, negated)) => {
                        format!(
                            "{}({}, by = join_by({}))",
                            if negated { "anti_join" } else { "semi_join" },
                            other.text,
                            join_by(by)
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

                Step::Sort { keys, missing_first, .. } => {
                    // **dplyr already puts a missing value last, in both
                    // directions, and there is no argument to say otherwise** —
                    // `arrange` has no `na_position`. So the default needs
                    // nothing written and the other way is spelled with a key
                    // in front: `!is.na(x)` is FALSE where the value is absent,
                    // and FALSE sorts before TRUE. Measured against dplyr
                    // rather than read: `arrange(!is.na(x), x)` returns the
                    // absent rows first, ascending and descending alike.
                    let mut ordered: Vec<String> = Vec::new();
                    for k in keys {
                        if *missing_first {
                            ordered.push(format!("!is.na({})", name(&k.column.text)));
                        }
                        ordered.push(if k.descending {
                            format!("desc({})", name(&k.column.text))
                        } else {
                            name(&k.column.text)
                        });
                    }
                    format!("arrange({})", ordered.join(", "))
                }

                Step::AddRows { other, .. } => format!("bind_rows({})", other.text),

                // **The one place tidyr is shorter than the grammar**, and it
                // is worth showing for that reason alone: `complete` is one
                // call and this verb is one word, so the reading aid here is
                // the name rather than the shape.
                //
                // `by` is where they part. tidyr has no argument for it and
                // reads the grouping off the frame, so a grouped completion is
                // three calls with a state change in the middle — set the
                // grouping, complete, put it back. That `ungroup` is not
                // decoration: a frame left grouped changes what every later
                // verb does, which is the class of bug the grammar's `by =`
                // exists to make impossible.
                Step::AddCombinations { names, by, .. } => {
                    let crossed: Vec<String> =
                        names.iter().map(|n| name(&n.text)).collect();
                    let complete = format!("complete({})", crossed.join(", "));
                    if by.is_empty() {
                        complete
                    } else {
                        let groups: Vec<String> =
                            by.iter().map(|n| name(&n.text)).collect();
                        format!(
                            "group_by({}) |>\n  {complete} |>\n  ungroup()",
                            groups.join(", ")
                        )
                    }
                }

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
                    let verb = match unmatched {
                        Unmatched::This => "left_join",
                        Unmatched::None => "inner_join",
                        Unmatched::Both => "full_join",
                    };
                    format!("{verb}({}, by = join_by({}))", other.text, join_by(by))
                }
                // dplyr names both ends, so this is the one target where the
                // grammar's pair maps onto a pair rather than onto a mechanism.
                Step::Take { count, by, last, ties, .. } if *ties => {
                    // **dplyr is the one target with a word for this**, which
                    // is why the rendering changes shape rather than gaining a
                    // clause: `slice_min`/`slice_max` take `with_ties`, and
                    // `slice_head` does not have it at all. Which of the two is
                    // right follows from the sort's own direction, since "the
                    // first three with ties" is "the three smallest" when the
                    // sort ascends and "the three largest" when it descends.
                    let sorted = last_sort(plan, i)
                        .expect("ties are only reached after a sort");
                    let first = &sorted[0];
                    let descending = first.descending != *last;
                    let verb = if descending { "slice_max" } else { "slice_min" };
                    let mut args = vec![
                        format!("order_by = {}", name(&first.column.text)),
                        format!("n = {count}"),
                        "with_ties = TRUE".to_string(),
                    ];
                    if !by.is_empty() {
                        args.push(format!("by = {}", columns(by)));
                    }
                    format!("{verb}({})", args.join(", "))
                }

                Step::Take { count, by, last, .. } => {
                    let end = if *last { "tail" } else { "head" };
                    if by.is_empty() {
                        format!("{end}({count})")
                    } else {
                        let groups: Vec<String> =
                            by.iter().map(|n| n.text.clone()).collect();
                        format!("slice_{end}(n = {count}, by = c({}))", groups.join(", "))
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
        "standard_deviation" => "sd",
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
        // **A quantified condition never reaches a backend**, because the
        // checker expands it into ordinary conditions before anything renders
        // (§13.11's move, for a question). It is written out in the grammar's
        // own words rather than panicking, so that the drawing of a sentence
        // that did *not* check still has something to show.
        Expr::Quantified { every, .. } => {
            format!("# {} of the matched columns", if *every { "every" } else { "any" })
        }

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

        // dplyr 1.2.0's own lookup — it deprecated `case_match` in this
        // word's favour the same release, and a printing backend does not
        // hand a reader a deprecation warning. `from` and `to` are parallel
        // vectors, which is dplyr's idiom to spell even though the grammar
        // refused the shape for its own sentence; `default` is the
        // `otherwise`, always written because the sentence always carries
        // one.
        Expr::Lookup { subject, pairs, otherwise, .. } => {
            let froms: Vec<String> = pairs.iter().map(|(from, _)| expr(from)).collect();
            let tos: Vec<String> = pairs.iter().map(|(_, to)| expr(to)).collect();
            format!(
                "recode_values({}, from = c({}), to = c({}), default = {})",
                expr(subject),
                froms.join(", "),
                tos.join(", "),
                expr(otherwise)
            )
        }

        // **slider, which is the tidyverse's own rolling.** Base R has no
        // readable spelling for a moving window — `stats::filter` reads as
        // nothing and handles one aggregate — and this backend already writes
        // `vctrs::` and `stringr::` by name for the same reason: recognized
        // beats minimal. `.complete = TRUE` is the full-window rule the
        // grammar names — `NA` until the window holds n rows — and the plain
        // R function inside (no `na.rm`) is what makes a missing value in a
        // full window answer missing, the way every engine here answers it.
        Expr::Rolling { agg, args, count, .. } => {
            let rfun = match agg.as_str() {
                "total" => "sum",
                "average" => "mean",
                "median" => "median",
                "smallest" => "min",
                "largest" => "max",
                "standard_deviation" => "sd",
                other => unreachable!("`{other}` reached the dplyr backend inside `rolling`"),
            };
            let n = match count.as_ref() {
                Expr::Whole { value, .. } => *value,
                _ => unreachable!("the checker admits only a written whole number"),
            };
            format!(
                "slider::slide_dbl({}, {rfun}, .before = {}, .complete = TRUE)",
                expr(&args[0]),
                n - 1
            )
        }
    }
}

/// The inside of dplyr's `join_by()`, for a join or for a filtering one.
///
/// **This is the one target where the grammar's `is` maps onto a word rather
/// than onto a mechanism.** `join_by` already spells both shapes — a bare name
/// where both tables agree, `customer_id == id` where they do not — and it
/// writes the sides in the order god writes them, so the pair goes across
/// unchanged.
/// The keys of the most recent `sort` before this step, which is what "first"
/// and "tied" both mean here.
fn last_sort(plan: &Plan, before: usize) -> Option<&[SortKey]> {
    plan.steps[..before].iter().rev().find_map(|step| match step {
        Step::Sort { keys, .. } => Some(keys.as_slice()),
        _ => None,
    })
}

fn join_by(keys: &[JoinKey]) -> String {
    keys.iter()
        .map(|k| {
            if k.is_same() {
                k.this.text.clone()
            } else {
                format!("{} == {}", k.this.text, k.other.text)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The table and key of a `keep` that is really a filtering join, and whether it
/// is the anti one.
fn filtering_join(condition: &Expr) -> Option<(&Name, &[JoinKey], bool)> {
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
        // R's own `sd` is the sample deviation, which is what the grammar's
        // word means — measured against all five engines rather than assumed.
        "standard_deviation" => format!("sd({}, na.rm = TRUE)", arg(0)),
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
        // **`str_c` rather than `paste0`, and the difference is the whole
        // ruling.** `paste0("a", NA)` is the four characters `aNA`, because
        // base R stringifies the missing value; `str_c` returns NA. The grammar
        // says missing anywhere makes the answer missing, so the spelling that
        // says so is the one printed.
        "join_text" => format!(
            "stringr::str_c({})",
            args.iter().map(expr).collect::<Vec<_>>().join(", ")
        ),
        // **`paste` here where `join_text` refused it, and the subsetting is
        // what makes that safe.** Base R stringifies a missing value, so a bare
        // `paste(x, collapse = ", ")` writes the characters `NA` into the middle
        // of the answer — measured, not assumed. This aggregate skips absent
        // values the way every other aggregate does, and `x[!is.na(x)]` is how
        // R says that. `str_c(collapse = )` would propagate instead, which is
        // the other word's rule and not this one's.
        "join_rows" => format!(
            "paste({0}[!is.na({0})], collapse = {1})",
            arg(0),
            arg(1)
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
        // R's `%%` is already floored, which is the convention the grammar
        // names, so this passes straight through.
        "remainder" => format!("({} %% {})", arg(0), arg(1)),
        // **`vctrs::vec_fill_missing` rather than an idiom.** The base-R
        // spellings for this are all tricks — indexing by a `cummax` of
        // positions, or a `cumsum` of non-missing — and every one of them is
        // unreadable at a glance, which is the thing a printing backend exists
        // to avoid. vctrs is a tidyverse package a dplyr reader already has,
        // and this backend already writes `lubridate::` for the same reason.
        "latest" => format!("vctrs::vec_fill_missing({}, direction = \"down\")", arg(0)),
        "previous" => format!("lag({}{})", arg(0), super::step(args)),
        "following" => format!("lead({}{})", arg(0), super::step(args)),
        "to_number" => format!("as.numeric({})", arg(0)),
        // `as.integer` after the rounding, so the answer prints as a whole
        // number rather than as `7` with a decimal point. The rounding is what
        // decided the value; the cast only settles how it is stored.
        "round_below" => format!("as.integer(floor({}))", arg(0)),
        "round_above" => format!("as.integer(ceiling({}))", arg(0)),
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
