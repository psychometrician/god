//! A plan as polars, for reading rather than for running.
//!
//! Nothing executes this. It exists so that someone can ask what a sentence
//! would be in a language they already know, and get an answer they recognize
//! immediately:
//!
//! ```text
//! sales then keep where [region] is "West" then take 10
//!
//! (sales
//!     .filter(pl.col("region") == "West")
//!     .head(10))
//! ```
//!
//! **polars is the closest of the printing targets to the grammar itself**, and
//! the rendering is short because of it. A verb is a method, a column is
//! `pl.col`, and the two reshaping verbs are one call each. Where the other
//! targets need a paragraph, this one needs a line, and that is worth seeing
//! next to them.
//!
//! Two places the fit is not exact, and both are recorded where they happen:
//! `add_rows` is a function over two frames rather than a method on one, and a
//! name pattern with more than one piece has to be taken apart after the
//! unpivot rather than during it.
//!
//! The chain is wrapped in parentheses so the printed text is something a reader
//! can paste. A leading `.` on a continuation line is not a statement in Python
//! the way `|>` is an operator in R, so the parentheses are doing real work
//! rather than decorating.

use super::Backend;
use crate::check::Schema;
use crate::plan::*;

pub struct Polars;

impl Backend for Polars {
    fn name(&self) -> &'static str {
        "polars"
    }

    fn render(&self, plan: &Plan, entering: &[Schema]) -> String {
        let mut calls: Vec<String> = Vec::new();

        for (i, step) in plan.steps.iter().enumerate() {
            match step {
                // A filtering join is a `how=` on the join here rather than a
                // condition inside the filter, which is the reason the grammar
                // only lets `matching` stand as the whole question.
                Step::Keep { condition, .. } => match filtering_join(condition) {
                    Some((other, by, negated)) => calls.push(format!(
                        "join({}, on={}, how=\"{}\")",
                        other.text,
                        list(by),
                        if negated { "anti" } else { "semi" }
                    )),
                    None => calls.push(format!("filter({})", expr(condition))),
                },

                Step::Pick { names, all_but, .. } => {
                    let listed = list(names);
                    if *all_but {
                        calls.push(format!("drop({listed})"));
                    } else {
                        calls.push(format!("select({listed})"));
                    }
                }

                Step::Add { values, by, .. } => {
                    let args: Vec<String> = values
                        .iter()
                        .map(|v| {
                            let value = expr(&v.value);
                            // `over` is how a value is worked out within a group
                            // and handed back to every row, which is exactly what
                            // `by` means on `add`.
                            let scoped = if by.is_empty() {
                                value
                            } else {
                                format!("{value}.over({})", list(by))
                            };
                            format!("{scoped}.alias({})", text(&v.name.text))
                        })
                        .collect();
                    calls.push(format!("with_columns({})", args.join(", ")));
                    // Computing a window regroups the rows and nothing puts them
                    // back, so a sort written earlier has to be said again.
                    if values.iter().any(|v| v.value.windows()) {
                        if let Some(keys) = last_sort(plan, i) {
                            calls.push(sort_by(keys));
                        }
                    }
                }

                Step::Summarize { values, by, .. } => {
                    let args: Vec<String> = values
                        .iter()
                        .map(|v| format!("{}.alias({})", expr(&v.value), text(&v.name.text)))
                        .collect();
                    if by.is_empty() {
                        // No groups means one group, and `select` over an
                        // aggregate is how polars says that. `group_by` with an
                        // empty list is not a sentence it has.
                        calls.push(format!("select({})", args.join(", ")));
                    } else {
                        calls.push(format!("group_by({}).agg({})", list(by), args.join(", ")));
                        // Grouping promises nothing about the order the groups
                        // come back in, so they are ordered by the columns that
                        // define them.
                        calls.push(ordered(by));
                    }
                }

                Step::Sort { keys, .. } => {
                    let columns: Vec<String> =
                        keys.iter().map(|k| text(&k.column.text)).collect();
                    let mut args = vec![format!("[{}]", columns.join(", "))];
                    if keys.iter().any(|k| k.descending) {
                        let flags: Vec<String> = keys
                            .iter()
                            .map(|k| if k.descending { "True" } else { "False" }.to_string())
                            .collect();
                        args.push(format!("descending=[{}]", flags.join(", ")));
                    }
                    calls.push(format!("sort({})", args.join(", ")));
                }

                Step::Take { count, by, .. } => {
                    if by.is_empty() {
                        calls.push(format!("head({count})"));
                    } else {
                        calls.push(format!("group_by({}).head({count})", list(by)));
                        // Taking the first rows of each group regroups them, so
                        // the order the sort established has to survive it.
                        if let Some(keys) = last_sort(plan, i) {
                            calls.push(sort_by(keys));
                        }
                    }
                }

                Step::Join { other, by, unmatched, .. } => {
                    let how = match unmatched {
                        Unmatched::This => "left",
                        Unmatched::None => "inner",
                        Unmatched::Both => "full",
                    };
                    calls.push(format!(
                        "join({}, on={}, how=\"{how}\")",
                        other.text,
                        list(by)
                    ));
                }

                // **The one verb that is a function over two frames rather than a
                // method on one.** `pl.concat([a, b])` is what a polars reader
                // writes with both frames in hand; `vstack` is the same thing as
                // a method, and it is what keeps this printable as one chain.
                Step::AddRows { other, .. } => calls.push(format!("vstack({})", other.text)),

                // The same mechanism pandas needs, in polars' spelling, and the
                // same reason: no library here has a word for it.
                //
                // `coalesce=True` is the entry that decides whether this is
                // right. A full join leaves polars holding two of each key —
                // the left's and the right's, one of them null on every row
                // that matched only one side — and without coalescing, the
                // grid's values would sit in a second column called
                // `region_right` while `region` stayed missing.
                Step::AddCombinations { names, by, .. } => {
                    let held: Vec<String> = by.iter().map(|n| n.text.clone()).collect();
                    let mut grid = String::new();
                    for (k, n) in names.iter().enumerate() {
                        let mut wanted = held.clone();
                        wanted.push(n.text.clone());
                        let distinct = format!(
                            "d.select({}).drop_nulls().unique()",
                            strings(&wanted)
                        );
                        if k == 0 {
                            grid = distinct;
                        } else if held.is_empty() {
                            grid = format!("{grid}.join({distinct}, how=\"cross\")");
                        } else {
                            grid = format!(
                                "{grid}.join({distinct}, on={}, how=\"inner\")",
                                strings(&held)
                            );
                        }
                    }
                    let keys: Vec<String> =
                        held.iter().cloned().chain(names.iter().map(|n| n.text.clone())).collect();
                    calls.push(format!(
                        "pipe(lambda d: d.join({grid}, on={}, how=\"full\", coalesce=True))",
                        strings(&keys)
                    ));
                }

                // Dropping repeats says nothing about the order the rest should
                // be in, and an answer that reorders itself between runs is not
                // predictable. Its groups are the distinct rows, so ordering by
                // every column is the same rule `summarize` follows.
                Step::DropDuplicates { .. } => {
                    calls.push("unique()".to_string());
                    let all: Vec<String> =
                        entering[i].columns.iter().map(|(c, _)| c.clone()).collect();
                    calls.push(format!("sort({})", strings(&all)));
                }

                Step::Rename { values, .. } => {
                    let pairs: Vec<String> = values
                        .iter()
                        .map(|v| match &v.value {
                            Expr::Column(from) => {
                                format!("{}: {}", text(&from.text), text(&v.name.text))
                            }
                            other => format!("{}: {}", expr(other), text(&v.name.text)),
                        })
                        .collect();
                    calls.push(format!("rename({{{}}})", pairs.join(", ")));
                }

                Step::DropMissing { names, .. } => {
                    if names.is_empty() {
                        calls.push("drop_nulls()".to_string());
                    } else {
                        calls.push(format!("drop_nulls({})", list(names)));
                    }
                }

                Step::FillMissing { values, .. } => {
                    let filled: Vec<String> = values
                        .iter()
                        .map(|v| {
                            format!(
                                "pl.col({}).fill_null({})",
                                text(&v.name.text),
                                expr(&v.value)
                            )
                        })
                        .collect();
                    calls.push(format!("with_columns({})", filled.join(", ")));
                }

                Step::Lengthen { resolved, .. } => {
                    let Some(shape) = resolved else { continue };
                    calls.extend(lengthen(shape));
                    calls.push(format!("sort({})", strings(&lengthen_order(shape))));
                }

                Step::Widen { name: pattern, value, by, missing, .. } => {
                    let pieces: Vec<String> =
                        pattern.named_parts().into_iter().map(|p| text(&p)).collect();
                    let on = if pieces.len() == 1 {
                        pieces[0].clone()
                    } else {
                        format!("[{}]", pieces.join(", "))
                    };
                    let mut args = vec![format!("on={on}")];
                    if !by.is_empty() {
                        args.push(format!("index={}", list(by)));
                    }
                    // An aggregate in `value` is what answers "two rows want one
                    // cell", and over here that is a separate argument naming the
                    // function rather than an expression wrapping the column.
                    match aggregate_of(value) {
                        Some((word, inner)) => {
                            args.push(format!("values={}", column_text(&inner)));
                            args.push(format!("aggregate_function=\"{word}\""));
                        }
                        None => args.push(format!("values={}", column_text(value))),
                    }
                    calls.push(format!("pivot({})", args.join(", ")));
                    if let Some(filler) = missing {
                        calls.push(format!("fill_null({})", expr(filler)));
                    }
                    if !by.is_empty() {
                        calls.push(ordered(by));
                    }
                }
            }
        }

        if calls.is_empty() {
            return plan.source.clone();
        }
        format!(
            "({}\n    .{})",
            plan.source,
            calls.join("\n    .")
        )
    }
}

/// `lengthen` as one `unpivot`, plus whatever taking the name apart needs.
///
/// **One piece is one call and the rest is arithmetic on text.** polars unpivots
/// into exactly one name column, so a pattern with two pieces has to be split
/// afterward and a `{value}` pattern has to be pivoted back. That is not a
/// failing of polars; it is the same work tidyr hides behind `names_sep` and
/// `names_pattern`, made visible. Seeing the difference is the reason a reader
/// asked.
fn lengthen(shape: &Lengthened) -> Vec<String> {
    let mut calls = Vec::new();

    // Every column that gets stacked, in the order the table had them, with no
    // repeats: one block per output row group, and a `{value}` pattern reads
    // several sources into one block.
    let mut stacked: Vec<String> = Vec::new();
    for row in &shape.rows {
        for source in &row.sources {
            if !stacked.contains(source) {
                stacked.push(source.clone());
            }
        }
    }

    let simple = shape.name_columns.len() == 1 && shape.value_columns.len() == 1;
    // A name column to split, when the pattern has more than one piece. It is
    // dropped again below, so its name only has to survive two calls.
    let holding = if simple { shape.name_columns[0].clone() } else { "name".to_string() };
    let held = if shape.value_columns.len() == 1 {
        shape.value_columns[0].clone()
    } else {
        "value".to_string()
    };

    let mut args = Vec::new();
    if !shape.keep.is_empty() {
        args.push(format!("index={}", strings(&shape.keep)));
    }
    args.push(format!("on={}", strings(&stacked)));
    args.push(format!("variable_name={}", text(&holding)));
    args.push(format!("value_name={}", text(&held)));
    calls.push(format!("unpivot({})", args.join(", ")));

    if simple {
        return calls;
    }

    // The pieces of the name, in order. A `{value}` piece says which value
    // column the rest of the row belongs to, so it is split out under a name of
    // its own and then read back as column headings.
    let mut pieces: Vec<String> = shape.name_columns.clone();
    if shape.value_columns.len() > 1 {
        pieces.push("__value".to_string());
    }
    let separator = separator_of(&stacked, &pieces);
    let splits: Vec<String> = pieces
        .iter()
        .enumerate()
        .map(|(i, piece)| {
            format!(
                "pl.col({}).str.split({}).list.get({i}).alias({})",
                text(&holding),
                text(&separator),
                text(piece)
            )
        })
        .collect();
    calls.push(format!("with_columns({})", splits.join(", ")));
    calls.push(format!("drop({})", text(&holding)));

    if shape.value_columns.len() > 1 {
        let mut index = shape.keep.clone();
        index.extend(shape.name_columns.iter().cloned());
        calls.push(format!(
            "pivot(on=\"__value\", index={}, values={})",
            strings(&index),
            text(&held)
        ));
    }
    calls
}

/// The order a `lengthen` has to restate.
///
/// Stacking columns into rows is one more step that decides the order for
/// itself, so the answer is ordered by what it produced: the columns that were
/// kept, then the names, then the values.
fn lengthen_order(shape: &Lengthened) -> Vec<String> {
    shape
        .keep
        .iter()
        .chain(shape.name_columns.iter())
        .chain(shape.value_columns.iter())
        .cloned()
        .collect()
}

/// `sort` over a list of columns, all ascending.
fn ordered(names: &[Name]) -> String {
    format!("sort({})", list(names))
}

/// `sort` over keys that may carry a direction.
fn sort_by(keys: &[SortKey]) -> String {
    let columns: Vec<String> = keys.iter().map(|k| text(&k.column.text)).collect();
    let mut args = vec![format!("[{}]", columns.join(", "))];
    if keys.iter().any(|k| k.descending) {
        let flags: Vec<String> = keys
            .iter()
            .map(|k| if k.descending { "True" } else { "False" }.to_string())
            .collect();
        args.push(format!("descending=[{}]", flags.join(", ")));
    }
    format!("sort({})", args.join(", "))
}

/// The most recent `sort` before a step, whose order a later step has to restate.
fn last_sort(plan: &Plan, before: usize) -> Option<&[SortKey]> {
    plan.steps[..before].iter().rev().find_map(|step| match step {
        Step::Sort { keys, .. } => Some(keys.as_slice()),
        _ => None,
    })
}

/// The text between the pieces of a stacked column's name.
///
/// The checker has already matched every stacked column against the pattern, so
/// the separator is whatever sits between the pieces in a name that matched.
/// Reading it back off the data rather than off the pattern keeps this honest
/// when the pattern's literals are empty.
fn separator_of(stacked: &[String], pieces: &[String]) -> String {
    for candidate in ["_", ".", "-", " "] {
        if stacked
            .iter()
            .all(|s| s.matches(candidate).count() + 1 == pieces.len())
        {
            return candidate.to_string();
        }
    }
    "_".to_string()
}

/// The aggregate wrapping a single column, as polars' own word for it.
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
    let word = match fname.as_str() {
        "total" => "sum",
        "average" => "mean",
        "median" => "median",
        "smallest" => "min",
        "largest" => "max",
        "first" => "first",
        "last" => "last",
        "unique_count" => "len",
        _ => return None,
    };
    Some((word, args[0].clone()))
}

/// The bare name of a column, for an argument that wants a name and not an
/// expression. `pivot` takes its `values` that way.
fn column_text(e: &Expr) -> String {
    match e {
        Expr::Column(n) => text(&n.text),
        other => expr(other),
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

fn list(names: &[Name]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| text(&n.text)).collect();
    format!("[{}]", quoted.join(", "))
}

fn strings(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| text(n)).collect();
    format!("[{}]", quoted.join(", "))
}

fn text(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A value standing where polars would otherwise read it as a column name.
///
/// **`then("big")` is the column called `big`, not the word.** polars resolves a
/// bare string to a column wherever an expression is expected, so a rendering
/// that looks exactly right returns the wrong thing, or fails naming a column
/// nobody wrote. It is the same shape as a wrongly quoted identifier on Spark:
/// the text reads correctly and means something else, and only running it says
/// so. A number cannot be a column name, so only text needs the wrapper.
fn literal(e: &Expr) -> String {
    match e {
        Expr::Text { value, .. } => format!("pl.lit({})", text(value)),
        other => expr(other),
    }
}

fn expr(e: &Expr) -> String {
    match e {
        Expr::Column(n) => format!("pl.col({})", text(&n.text)),
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
        Expr::Truth { value, .. } => if *value { "True" } else { "False" }.to_string(),
        Expr::Missing { .. } => "None".to_string(),

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
        // polars overloads the bitwise operators, so the parentheses are not
        // style: `&` binds tighter than `==` in Python and the expression means
        // something else without them.
        Expr::Logic { op, left, right, .. } => {
            let symbol = match op {
                Logic::And => "&",
                Logic::Or => "|",
            };
            format!("({} {symbol} {})", expr(left), expr(right))
        }
        Expr::Not { inner, .. } => format!("~{}", expr(inner)),
        Expr::In { left, set, negated, .. } => {
            let values: Vec<String> = set.iter().map(expr).collect();
            let test = format!("{}.is_in([{}])", expr(left), values.join(", "));
            if *negated {
                format!("~{test}")
            } else {
                test
            }
        }
        Expr::IsMissing { inner, negated, .. } => {
            if *negated {
                format!("{}.is_not_null()", expr(inner))
            } else {
                format!("{}.is_null()", expr(inner))
            }
        }
        // Unreachable in a checked plan: `matching` may only stand as a whole
        // `keep` condition, and the step above renders that case as the join
        // polars actually has.
        Expr::Matching { other, .. } => format!("# matching({})", other.text),
        // `"min"` is competition ranking, which is what a person means by rank:
        // ties share a place and the next one skips. polars defaults to the
        // average of the tied places, which is not it.
        Expr::Window { kind, key, .. } => match (kind, key) {
            (Window::Rank, Some(k)) if k.descending => {
                format!("pl.col({}).rank(\"min\", descending=True)", text(&k.column.text))
            }
            (Window::Rank, Some(k)) => {
                format!("pl.col({}).rank(\"min\")", text(&k.column.text))
            }
            (Window::Rank, None) => "pl.first().rank(\"min\")".to_string(),
            // Counting from 1, the way every other place in this grammar does.
            (Window::RowNumber, _) => "pl.int_range(1, pl.len() + 1)".to_string(),
        },
        // `literal=True` because the grammar's word looks for text a person
        // typed, and `contains` reads a regular expression otherwise.
        Expr::TextTest { op, left, value, .. } => match op {
            TextOp::Starts => format!("{}.str.starts_with({})", expr(left), expr(value)),
            TextOp::Ends => format!("{}.str.ends_with({})", expr(left), expr(value)),
            TextOp::Contains => {
                format!("{}.str.contains({}, literal=True)", expr(left), expr(value))
            }
        },
        Expr::ColumnValue { .. } => "value".to_string(),
        Expr::ColumnKind { .. } => "kind".to_string(),
        // polars spells the arms as a chain, which is the shape closest to the
        // grammar's own: each test carries the value it gives, in order, and the
        // first match wins.
        Expr::When { arms, otherwise, .. } => {
            // Only the first arm is reached through the module. The rest are
            // methods on the chain, and writing `pl.when` again produces
            // `.pl.when`, which is not a thing.
            let mut out = String::new();
            for (test, value) in arms {
                out.push_str(&format!(
                    "{}when({}).then({})",
                    if out.is_empty() { "pl." } else { "." },
                    expr(test),
                    literal(value)
                ));
            }
            match otherwise {
                Some(fallback) => format!("{out}.otherwise({})", literal(fallback)),
                None => out,
            }
        }
        Expr::ColumnName { .. } => "name".to_string(),
        Expr::Call { name: fname, args, .. } => call(fname, args),
    }
}

/// How polars spells each of the grammar's functions.
///
/// **Nearly every one is a method on the expression rather than a call around
/// it**, which is the one structural difference from the dplyr rendering and the
/// reason this reads as a chain throughout.
fn call(fname: &str, args: &[Expr]) -> String {
    let arg = |i: usize| args.get(i).map(expr).unwrap_or_default();
    match fname {
        // polars skips the absent value in an aggregate, which is what the
        // grammar's `total` means, so none of these needs an argument saying so.
        "total" => format!("{}.sum()", arg(0)),
        "average" => format!("{}.mean()", arg(0)),
        "median" => format!("{}.median()", arg(0)),
        "smallest" => format!("{}.min()", arg(0)),
        "largest" => format!("{}.max()", arg(0)),
        "first" => format!("{}.first()", arg(0)),
        "last" => format!("{}.last()", arg(0)),
        "unique_count" => format!("{}.n_unique()", arg(0)),
        "row_count" => "pl.len()".to_string(),
        "first_present" => format!(
            "pl.coalesce([{}])",
            args.iter().map(expr).collect::<Vec<_>>().join(", ")
        ),
        // `concat_str` leaves nulls alone by default, which is the rule the
        // grammar settled on, so nothing has to be said to get it.
        //
        // **`literal` rather than `expr`, and this was caught by running it.**
        // A separator is the commonest argument here and it is nearly always a
        // bare piece of text, which polars reads inside this list as a *column
        // name*: the printed line looked perfect and failed saying it could not
        // find a column called `" "`.
        "join_text" => format!(
            "pl.concat_str([{}])",
            args.iter().map(literal).collect::<Vec<_>>().join(", ")
        ),
        "year" => format!("{}.dt.year()", arg(0)),
        "month" => format!("{}.dt.month()", arg(0)),
        "day" => format!("{}.dt.day()", arg(0)),
        "hour" => format!("{}.dt.hour()", arg(0)),
        // Monday is 1 here already, which is the numbering the grammar names.
        "weekday" => format!("{}.dt.weekday()", arg(0)),
        "running_total" => format!("{}.cum_sum()", arg(0)),
        // polars keeps the sign convention the grammar refused the words for:
        // one row back is a positive shift, one row forward is a negative one.
        "previous" => format!("{}.shift(1)", arg(0)),
        "following" => format!("{}.shift(-1)", arg(0)),
        "to_number" => format!("{}.cast(pl.Float64)", arg(0)),
        "to_whole" => format!("{}.cast(pl.Int64)", arg(0)),
        "to_text" => format!("{}.cast(pl.String)", arg(0)),
        // Not a cast: polars deprecated the string one, and `pl.Date` was the
        // wrong target anyway. DuckDB reads `to_date` as a timestamp, so
        // `hour` has an answer there, and a Date here threw the time away in
        // silence. Found by running the printed code, which is the only way
        // this class is ever found.
        "to_date" => format!("{}.str.to_datetime(time_unit=\"us\")", arg(0)),
        "trim" => format!("{}.str.strip_chars()", arg(0)),
        "characters" => format!("{}.str.len_chars()", arg(0)),
        // `literal=True` for the same reason `contains` needs it: the value is
        // text rather than a pattern.
        "replace_text" => {
            format!("{}.str.replace_all({}, {}, literal=True)", arg(0), arg(1), arg(2))
        }
        // The grammar counts the pieces from 1 and polars indexes from 0, so the
        // difference is written here rather than left to the reader.
        "split_text" => {
            let piece = match args.get(2) {
                Some(Expr::Whole { value, .. }) => (value - 1).to_string(),
                _ => format!("{} - 1", arg(2)),
            };
            format!("{}.str.split({}).list.get({piece})", arg(0), arg(1))
        }
        "between" => format!("{}.is_between({}, {})", arg(0), arg(1), arg(2)),
        "lower" => format!("{}.str.to_lowercase()", arg(0)),
        "upper" => format!("{}.str.to_uppercase()", arg(0)),
        other => unreachable!("`{other}` reached the polars backend without a spelling"),
    }
}
