//! A plan as pandas, for reading rather than for running.
//!
//! Nothing executes this. It exists so that someone can ask what a sentence
//! would be in a language they already know, and get an answer they recognize
//! immediately:
//!
//! ```text
//! sales then keep where [region] is "West" then take 10
//!
//! (sales
//!     .loc[lambda d: (d["region"] == "West")]
//!     .head(10))
//! ```
//!
//! **This is the longest of the printing backends and that is the honest
//! answer**, not a failure to find a shorter one. pandas has no single way to
//! chain, so the idiom that does chain is `.loc[lambda d: ...]`, `.assign` and
//! `.pipe`, and every column reference inside one of those has to name the frame
//! again. Where the grammar says `[revenue]` once, pandas says `d["revenue"]`
//! every time. Seeing that side by side is the reason someone asked.
//!
//! `np` is numpy, which pandas already requires, and it is reached for in one
//! place: a conditional with more than two arms is `np.select`, which is what
//! pandas users write because pandas has nothing of its own that reads better.
//!
//! **Two shapes genuinely do not chain and are written with `.pipe`.** Stacking
//! rows is `pd.concat`, a function over two frames rather than a method on one,
//! and asking whether a row has a partner needs the other frame in hand.

use super::Backend;
use crate::check::Schema;
use crate::plan::*;

pub struct Pandas;

/// The grouping a value is worked out within, when a step carries one.
#[derive(Default, Clone, Copy)]
struct Over<'a> {
    partition: &'a [Name],
}

impl Backend for Pandas {
    fn name(&self) -> &'static str {
        "pandas"
    }

    /// A grouped aggregate of an expression, `add ... total([a] * [b]) by`.
    ///
    /// pandas reaches a group's answer through one named column, so an
    /// aggregate built from arithmetic under a `by` has no one-line spelling
    /// here. Saying so beats rendering `transform` on something that is not a
    /// column, and the repair is one step: make the expression a column of its
    /// own, then aggregate that column by the group.
    fn refuses(&self, plan: &Plan) -> Option<crate::diagnostic::Diagnostic> {
        for step in &plan.steps {
            if let Step::Add { values, by, span, .. } = step {
                if by.is_empty() {
                    continue;
                }
                for v in values {
                    let mut bad = false;
                    v.value.walk(&mut |e| {
                        if let Expr::Call { name, args, .. } = e {
                            if crate::vocabulary::is_aggregate(name)
                                && name != "row_count"
                                && !matches!(args.first(), Some(Expr::Column(_)))
                            {
                                bad = true;
                            }
                        }
                    });
                    if bad {
                        return Some(crate::diagnostic::Diagnostic::illegal(
                            "pandas reaches a group's answer through one named column, \
                             so it cannot spell a grouped aggregate of an expression. \
                             Make the expression a column in its own `add`, then \
                             aggregate that column `by` the group",
                            *span,
                        ));
                    }
                }
            }
        }
        None
    }

    fn render(&self, plan: &Plan, entering: &[Schema]) -> String {
        let mut calls: Vec<String> = Vec::new();

        for (i, step) in plan.steps.iter().enumerate() {
            match step {
                Step::Keep { condition, .. } => match filtering_join(condition) {
                    Some((other, by, negated)) => {
                        // A filtering join has no verb here, so it is spelled as
                        // what it does: keep the rows whose key is, or is not,
                        // one of the keys over there.
                        let key = by.first().map(|k| k.text.clone()).unwrap_or_default();
                        calls.push(format!(
                            "loc[lambda d: {}d[{}].isin({}[{}])]",
                            if negated { "~" } else { "" },
                            text(&key),
                            other.text,
                            text(&key)
                        ));
                    }
                    None => calls.push(format!("loc[lambda d: {}]", expr(condition))),
                },

                Step::Pick { names, all_but, .. } => {
                    let listed = list(names);
                    if *all_but {
                        calls.push(format!("drop(columns={listed})"));
                    } else {
                        calls.push(format!("loc[:, {listed}]"));
                    }
                }

                Step::Add { values, by, .. } => {
                    let over = Over { partition: by };
                    let args: Vec<String> = values
                        .iter()
                        .map(|v| assignment(&v.name.text, &expr_over(&v.value, over)))
                        .collect();
                    calls.push(format!("assign({})", args.join(", ")));
                }

                Step::Summarize { values, by, .. } => {
                    let named: Vec<String> = values
                        .iter()
                        .map(|v| {
                            let (column, how) = aggregation(&v.value, by);
                            assignment(
                                &v.name.text,
                                &format!("({}, {})", text(&column), text(how)),
                            )
                        })
                        .collect();
                    if by.is_empty() {
                        // **No grouping is the awkward one here, not the easy
                        // one.** Named aggregation needs a column to hang each
                        // answer on, and `row_count` names none, so a summarize
                        // with no `by` is built as the one row it is. It also
                        // takes any expression, which named aggregation cannot.
                        calls.push(format!("pipe(lambda d: pd.DataFrame({{{}}}))", pairs(values)));
                    } else {
                        calls.push(format!(
                            "groupby({}, as_index=False).agg({})",
                            list(by),
                            named.join(", ")
                        ));
                        // Grouping already sorts by the group columns here, and
                        // saying so keeps the answer the same as every other
                        // target's rather than resting on a default.
                        calls.push(sorted(by));
                    }
                }

                Step::Sort { keys, .. } => calls.push(sort_by(keys)),

                Step::Take { count, by, .. } => {
                    if by.is_empty() {
                        calls.push(format!("head({count})"));
                    } else {
                        calls.push(format!("groupby({}, as_index=False).head({count})", list(by)));
                        if let Some(keys) = last_sort(plan, i) {
                            calls.push(sort_by(keys));
                        }
                    }
                }

                Step::Join { other, by, unmatched, .. } => {
                    let how = match unmatched {
                        Unmatched::This => "left",
                        Unmatched::None => "inner",
                        Unmatched::Both => "outer",
                    };
                    calls.push(format!(
                        "merge({}, on={}, how=\"{how}\")",
                        other.text,
                        list(by)
                    ));
                }

                // **`pd.concat` is a function over two frames rather than a
                // method on one**, so this is the one verb that has to step out
                // of the chain and back into it. `.pipe` is how pandas does that.
                Step::AddRows { other, .. } => calls.push(format!(
                    "pipe(lambda d: pd.concat([d, {}], ignore_index=True))",
                    other.text
                )),

                Step::DropDuplicates { .. } => {
                    calls.push("drop_duplicates()".to_string());
                    let all: Vec<String> =
                        entering[i].columns.iter().map(|(c, _)| c.clone()).collect();
                    calls.push(format!("sort_values({})", strings(&all)));
                }

                Step::Rename { values, .. } => {
                    let renamed: Vec<String> = values
                        .iter()
                        .map(|v| match &v.value {
                            Expr::Column(from) => {
                                format!("{}: {}", text(&from.text), text(&v.name.text))
                            }
                            other => format!("{}: {}", expr(other), text(&v.name.text)),
                        })
                        .collect();
                    calls.push(format!("rename(columns={{{}}})", renamed.join(", ")));
                }

                Step::DropMissing { names, .. } => {
                    if names.is_empty() {
                        calls.push("dropna()".to_string());
                    } else {
                        calls.push(format!("dropna(subset={})", list(names)));
                    }
                }

                Step::FillMissing { values, .. } => {
                    let filled: Vec<String> = values
                        .iter()
                        .map(|v| {
                            assignment(
                                &v.name.text,
                                &format!(
                                    "lambda d: d[{}].fillna({})",
                                    text(&v.name.text),
                                    value(&v.value)
                                ),
                            )
                        })
                        .collect();
                    calls.push(format!("assign({})", filled.join(", ")));
                }

                Step::Lengthen { resolved, .. } => {
                    let Some(shape) = resolved else { continue };
                    calls.extend(lengthen(shape));
                    calls.push(format!("sort_values({})", strings(&lengthen_order(shape))));
                }

                Step::Widen { name: pattern, value, by, missing, .. } => {
                    let pieces = pattern.named_parts();
                    let columns = if pieces.len() == 1 {
                        text(pieces[0])
                    } else {
                        let owned: Vec<String> =
                            pieces.iter().map(|p| p.to_string()).collect();
                        strings(&owned)
                    };
                    let (held, how) = match aggregate_of(value) {
                        Some((word, inner)) => (column_text(&inner), word),
                        None => (column_text(value), "first"),
                    };
                    let mut args = vec![format!("index={}", list(by))];
                    args.push(format!("columns={columns}"));
                    args.push(format!("values={held}"));
                    args.push(format!("aggfunc={}", text(how)));
                    if let Some(filler) = missing {
                        args.push(format!("fill_value={}", value_of(filler)));
                    }
                    calls.push(format!("pivot_table({})", args.join(", ")));
                    // `pivot_table` puts the grouping in the index and names the
                    // column axis, and neither is a column a later step could
                    // name. Both are put back.
                    calls.push("reset_index()".to_string());
                    calls.push("rename_axis(columns=None)".to_string());
                }
            }
        }

        if calls.is_empty() {
            return plan.source.clone();
        }
        format!("({}\n    .{})", plan.source, calls.join("\n    ."))
    }
}

/// `lengthen` as `melt`, plus whatever taking the name apart needs.
fn lengthen(shape: &Lengthened) -> Vec<String> {
    let mut calls = Vec::new();

    let mut stacked: Vec<String> = Vec::new();
    for row in &shape.rows {
        for source in &row.sources {
            if !stacked.contains(source) {
                stacked.push(source.clone());
            }
        }
    }

    let simple = shape.name_columns.len() == 1 && shape.value_columns.len() == 1;
    let holding = if simple { shape.name_columns[0].clone() } else { "name".to_string() };
    let held = if shape.value_columns.len() == 1 {
        shape.value_columns[0].clone()
    } else {
        "value".to_string()
    };

    calls.push(format!(
        "melt(id_vars={}, value_vars={}, var_name={}, value_name={})",
        strings(&shape.keep),
        strings(&stacked),
        text(&holding),
        text(&held)
    ));

    if simple {
        return calls;
    }

    let mut pieces: Vec<String> = shape.name_columns.clone();
    if shape.value_columns.len() > 1 {
        pieces.push("__value".to_string());
    }
    let separator = separator_of(&stacked, &pieces);
    for (i, piece) in pieces.iter().enumerate() {
        calls.push(format!(
            "assign({})",
            assignment(
                piece,
                &format!(
                    "lambda d: d[{}].str.split({}).str[{i}]",
                    text(&holding),
                    text(&separator)
                )
            )
        ));
    }
    calls.push(format!("drop(columns=[{}])", text(&holding)));

    if shape.value_columns.len() > 1 {
        let mut index = shape.keep.clone();
        index.extend(shape.name_columns.iter().cloned());
        calls.push(format!(
            "pivot_table(index={}, columns=\"__value\", values={}, aggfunc=\"first\")",
            strings(&index),
            text(&held)
        ));
        calls.push("reset_index()".to_string());
        calls.push("rename_axis(columns=None)".to_string());
    }
    calls
}

fn lengthen_order(shape: &Lengthened) -> Vec<String> {
    shape
        .keep
        .iter()
        .chain(shape.name_columns.iter())
        .chain(shape.value_columns.iter())
        .cloned()
        .collect()
}

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

/// The column and the function behind a `summarize` value, as named aggregation
/// wants them.
///
/// **pandas asks for the two halves separately**, where the grammar writes one
/// expression, so they come apart here. `row_count` names no column at all, so
/// it is counted over one of the grouping columns, which is always present and
/// never missing.
fn aggregation(value: &Expr, by: &[Name]) -> (String, &'static str) {
    let fallback = by.first().map(|n| n.text.clone()).unwrap_or_default();
    let Expr::Call { name: fname, args, .. } = value else {
        return (fallback, "first");
    };
    let column = match args.first() {
        Some(Expr::Column(n)) => n.text.clone(),
        _ => fallback,
    };
    let how = match fname.as_str() {
        "total" => "sum",
        "average" => "mean",
        "median" => "median",
        "smallest" => "min",
        "largest" => "max",
        "first" => "first",
        "last" => "last",
        "unique_count" => "nunique",
        "row_count" => "size",
        _ => "first",
    };
    (column, how)
}

/// A `summarize` with no grouping, as the single row it produces.
///
/// Each answer is a scalar, so it is put in a list of one, which is how a
/// one-row frame is built.
fn pairs(values: &[Named]) -> String {
    let written: Vec<String> = values
        .iter()
        .map(|v| format!("{}: [{}]", text(&v.name.text), inner(&v.value, Over::default())))
        .collect();
    written.join(", ")
}

/// A keyword argument, spelled the way the name allows.
///
/// `assign(margin=...)` is what a pandas reader writes, and it is only available
/// where the name is a Python identifier. A column called `total revenue` needs
/// the dictionary form, so the name decides the spelling rather than a rule
/// applied everywhere.
fn assignment(name: &str, value: &str) -> String {
    if identifier(name) {
        format!("{name}={value}")
    } else {
        format!("**{{{}: {value}}}", text(name))
    }
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
}

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
        "unique_count" => "nunique",
        _ => return None,
    };
    Some((word, args[0].clone()))
}

fn column_text(e: &Expr) -> String {
    match e {
        Expr::Column(n) => text(&n.text),
        other => expr(other),
    }
}

/// A literal as a plain Python value, for an argument that is not an expression.
fn value_of(e: &Expr) -> String {
    match e {
        Expr::Text { value, .. } => text(value),
        Expr::Whole { value, .. } => value.to_string(),
        Expr::Decimal { value, .. } => decimal(*value),
        Expr::Truth { value, .. } => if *value { "True" } else { "False" }.to_string(),
        Expr::Missing { .. } => "None".to_string(),
        other => expr(other),
    }
}

fn value(e: &Expr) -> String {
    value_of(e)
}

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

fn sorted(names: &[Name]) -> String {
    format!("sort_values({})", list(names))
}

fn sort_by(keys: &[SortKey]) -> String {
    let columns: Vec<String> = keys.iter().map(|k| text(&k.column.text)).collect();
    let mut args = vec![format!("[{}]", columns.join(", "))];
    if keys.iter().any(|k| k.descending) {
        let flags: Vec<String> = keys
            .iter()
            .map(|k| if k.descending { "False" } else { "True" }.to_string())
            .collect();
        args.push(format!("ascending=[{}]", flags.join(", ")));
    }
    format!("sort_values({})", args.join(", "))
}

fn last_sort(plan: &Plan, before: usize) -> Option<&[SortKey]> {
    plan.steps[..before].iter().rev().find_map(|step| match step {
        Step::Sort { keys, .. } => Some(keys.as_slice()),
        _ => None,
    })
}

fn list(names: &[Name]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| text(&n.text)).collect();
    format!("[{}]", quoted.join(", "))
}

fn strings(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| text(n)).collect();
    format!("[{}]", quoted.join(", "))
}

fn text(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

fn decimal(v: f64) -> String {
    let s = format!("{v}");
    if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    }
}

/// A value, wrapped in the lambda `assign` and `loc` both want.
fn expr_over(e: &Expr, over: Over) -> String {
    format!("lambda d: {}", inner(e, over))
}

fn expr(e: &Expr) -> String {
    inner(e, Over::default())
}

/// A column, reached through the frame the lambda was handed.
fn column(name: &str, over: Over) -> String {
    if over.partition.is_empty() {
        format!("d[{}]", text(name))
    } else {
        format!(
            "d.groupby({})[{}]",
            list(over.partition),
            text(name)
        )
    }
}

fn inner(e: &Expr, over: Over) -> String {
    let go = |x: &Expr| inner(x, over);
    // Most of an expression is worked out row by row and never asks about a
    // group, so the grouping only reaches the places that do: the windows.
    let plain = Over::default();
    let flat = |x: &Expr| inner(x, plain);
    match e {
        Expr::Column(n) => format!("d[{}]", text(&n.text)),
        Expr::Text { value, .. } => text(value),
        Expr::Whole { value, .. } => value.to_string(),
        Expr::Decimal { value, .. } => decimal(*value),
        Expr::Truth { value, .. } => if *value { "True" } else { "False" }.to_string(),
        Expr::Missing { .. } => "None".to_string(),

        Expr::Arithmetic { op, left, right, .. } => {
            format!("({} {} {})", go(left), op, go(right))
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
            format!("({} {symbol} {})", go(left), go(right))
        }
        // pandas overloads the bitwise operators, so the parentheses are not
        // style: `&` binds tighter than `==` in Python and the expression means
        // something else without them.
        Expr::Logic { op, left, right, .. } => {
            let symbol = match op {
                Logic::And => "&",
                Logic::Or => "|",
            };
            format!("({} {symbol} {})", go(left), go(right))
        }
        Expr::Not { inner: i, .. } => format!("~{}", go(i)),
        Expr::In { left, set, negated, .. } => {
            let values: Vec<String> = set.iter().map(|v| flat(v)).collect();
            let test = format!("{}.isin([{}])", go(left), values.join(", "));
            if *negated {
                format!("~{test}")
            } else {
                test
            }
        }
        Expr::IsMissing { inner: i, negated, .. } => {
            if *negated {
                format!("{}.notna()", go(i))
            } else {
                format!("{}.isna()", go(i))
            }
        }
        // Unreachable in a checked plan: `matching` may only stand as a whole
        // `keep` condition, and the step above renders that case.
        Expr::Matching { other, .. } => format!("# matching({})", other.text),
        Expr::Window { kind, key, .. } => match (kind, key) {
            // `"min"` is competition ranking, which is what a person means by
            // rank. pandas defaults to averaging the tied places, which is not it.
            (Window::Rank, Some(k)) => {
                let ascending = if k.descending { ", ascending=False" } else { "" };
                format!(
                    "{}.rank(method=\"min\"{ascending})",
                    column(&k.column.text, over)
                )
            }
            (Window::Rank, None) => "d.rank(method=\"min\")".to_string(),
            // Counting from 1, the way every other place in this grammar does.
            (Window::RowNumber, _) => "np.arange(1, len(d) + 1)".to_string(),
        },
        Expr::TextTest { op, left, value: v, .. } => match op {
            TextOp::Starts => format!("{}.str.startswith({})", go(left), flat(v)),
            TextOp::Ends => format!("{}.str.endswith({})", go(left), flat(v)),
            TextOp::Contains => {
                format!("{}.str.contains({}, regex=False)", go(left), flat(v))
            }
        },
        Expr::ColumnValue { .. } => "value".to_string(),
        Expr::ColumnKind { .. } => "kind".to_string(),
        // **pandas has nothing of its own that reads better than this.** Nested
        // `where` calls invert the order the arms were written in, so `np.select`
        // is what pandas users reach for, and it keeps first-match-wins.
        Expr::When { arms, otherwise, .. } => {
            let tests: Vec<String> = arms.iter().map(|(t, _)| go(t)).collect();
            let values: Vec<String> = arms.iter().map(|(_, v)| flat(v)).collect();
            let default = otherwise
                .as_ref()
                .map(|f| flat(f))
                .unwrap_or_else(|| "None".to_string());
            format!(
                "np.select([{}], [{}], default={default})",
                tests.join(", "),
                values.join(", ")
            )
        }
        Expr::ColumnName { .. } => "name".to_string(),
        Expr::Call { name: fname, args, .. } => call(fname, args, over),
    }
}

/// How pandas spells each of the grammar's functions.
fn call(fname: &str, args: &[Expr], over: Over) -> String {
    // A grouped aggregate in `add` reaches its column through `groupby` and
    // hands the answer back with `transform`. The plain method beside other
    // columns would total the whole table and say nothing about it, which is
    // the silent wrong answer this whole project refuses to ship. `row_count`
    // names no column, so it borrows the first grouping key and counts with
    // `size`; an aggregate of an *expression* under a `by` has no one-line
    // pandas spelling and `refuses` has already turned it away.
    if !over.partition.is_empty() && crate::vocabulary::is_aggregate(fname) {
        let word = match fname {
            "total" => "sum",
            "average" => "mean",
            "median" => "median",
            "smallest" => "min",
            "largest" => "max",
            "first" => "first",
            "last" => "last",
            "unique_count" => "nunique",
            "row_count" => "size",
            _ => unreachable!("every aggregate has a transform word"),
        };
        let groups = list(over.partition);
        let column = match (fname, args.first()) {
            ("row_count", _) => text(&over.partition[0].text),
            (_, Some(Expr::Column(n))) => text(&n.text),
            _ => unreachable!("refused before rendering"),
        };
        return format!("d.groupby({groups})[{column}].transform({})", text(word));
    }
    let plain = Over::default();
    let arg = |i: usize| args.get(i).map(|a| inner(a, plain)).unwrap_or_default();
    // A window is worked out within the grouping, where there is one, and the
    // grouped form has to reach the column through `groupby` rather than take a
    // method on it.
    let windowed = |method: &str| match args.first() {
        Some(Expr::Column(n)) => format!("{}.{method}", column(&n.text, over)),
        _ => format!("{}.{method}", arg(0)),
    };
    match fname {
        // pandas skips the absent value in an aggregate, which is what the
        // grammar's `total` means, so none of these needs an argument saying so.
        "total" => format!("{}.sum()", arg(0)),
        "average" => format!("{}.mean()", arg(0)),
        "median" => format!("{}.median()", arg(0)),
        "smallest" => format!("{}.min()", arg(0)),
        "largest" => format!("{}.max()", arg(0)),
        "first" => format!("{}.iloc[0]", arg(0)),
        "last" => format!("{}.iloc[-1]", arg(0)),
        "unique_count" => format!("{}.nunique()", arg(0)),
        "row_count" => "len(d)".to_string(),
        "first_present" => {
            let rest: Vec<String> = args[1..].iter().map(|a| inner(a, plain)).collect();
            format!("{}.fillna({})", arg(0), rest.join(").fillna("))
        }
        // **Plain `+`, which is what a pandas reader would write**, and it
        // propagates the missing value the way the grammar says: adding to a
        // NaN gives a NaN. `str.cat` is the other spelling and its default is
        // the opposite, dropping the row's missing part and joining the rest.
        "join_text" => format!(
            "({})",
            args.iter().map(|a| inner(a, plain)).collect::<Vec<_>>().join(" + ")
        ),
        "year" => format!("{}.dt.year", arg(0)),
        "month" => format!("{}.dt.month", arg(0)),
        "day" => format!("{}.dt.day", arg(0)),
        "hour" => format!("{}.dt.hour", arg(0)),
        // **pandas counts Monday as 0 and the grammar counts it as 1**, so this
        // is shifted rather than passed through. `dayofweek` is already the ISO
        // order; only the origin differs.
        "weekday" => format!("({}.dt.dayofweek + 1)", arg(0)),
        "running_total" => windowed("cumsum()"),
        "previous" => windowed("shift(1)"),
        "following" => windowed("shift(-1)"),
        "to_number" => format!("pd.to_numeric({})", arg(0)),
        "to_whole" => format!("{}.astype(\"Int64\")", arg(0)),
        "to_text" => format!("{}.astype(\"string\")", arg(0)),
        "to_date" => format!("pd.to_datetime({})", arg(0)),
        "trim" => format!("{}.str.strip()", arg(0)),
        "characters" => format!("{}.str.len()", arg(0)),
        // `regex=False` because the grammar's word looks for text a person
        // typed rather than a pattern.
        "replace_text" => {
            format!("{}.str.replace({}, {}, regex=False)", arg(0), arg(1), arg(2))
        }
        // The grammar counts the pieces from 1 and pandas indexes from 0, so the
        // difference is written here rather than left to the reader.
        "split_text" => {
            let piece = match args.get(2) {
                Some(Expr::Whole { value, .. }) => (value - 1).to_string(),
                _ => format!("{} - 1", arg(2)),
            };
            format!("{}.str.split({}).str[{piece}]", arg(0), arg(1))
        }
        "between" => format!("{}.between({}, {})", arg(0), arg(1), arg(2)),
        "lower" => format!("{}.str.lower()", arg(0)),
        "upper" => format!("{}.str.upper()", arg(0)),
        other => unreachable!("`{other}` reached the pandas backend without a spelling"),
    }
}
