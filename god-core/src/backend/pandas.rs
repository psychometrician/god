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
            super::entering(entering.get(i));
            match step {
                Step::Keep { condition, .. } => match filtering_join(condition) {
                    Some((other, by, negated)) => {
                        // A filtering join has no verb here, so it is spelled as
                        // what it does: keep the rows whose key is, or is not,
                        // one of the keys over there. The two sides are named
                        // separately, which costs nothing here — `isin` was
                        // always reading one column against another and never
                        // needed them to share a word.
                        let mine = by.first().map(|k| k.this.text.clone()).unwrap_or_default();
                        let theirs =
                            by.first().map(|k| k.other.text.clone()).unwrap_or_default();
                        // `dropna()` on their side, because `isin` finds a
                        // missing value among missing values, and a missing key
                        // matches nothing. A row here whose key is missing is
                        // then matched by nothing, so the anti form keeps it,
                        // which is what `NOT EXISTS` does.
                        calls.push(format!(
                            "loc[lambda d: {}d[{}].isin({}[{}].dropna())]",
                            if negated { "~" } else { "" },
                            text(&mine),
                            other.text,
                            text(&theirs)
                        ));
                    }
                    None => calls.push(format!(
                        "loc[lambda d: {}]",
                        test(condition, Over::default())
                    )),
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
                                &format!("({}, {})", text(&column), how),
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
                            "groupby({}, as_index=False, dropna=False).agg({})",
                            list(by),
                            named.join(", ")
                        ));
                        // Grouping already sorts by the group columns here, and
                        // saying so keeps the answer the same as every other
                        // target's rather than resting on a default.
                        calls.push(sorted(by));
                    }
                }

                Step::Sort { keys, missing_first, .. } => calls.push(sort_by(keys, *missing_first)),

                Step::Take { count, by, last, ties, .. } if *ties => {
                    // **No `with_ties` anywhere in pandas**, so this renders the
                    // mechanism the way SQL does: rank, keep, drop. `method`
                    // matters — "min" is the ranking that gives tied rows the
                    // same place and skips the next, which is what makes
                    // `rank <= n` keep everything level with the cut.
                    let sorted = last_sort(plan, i)
                        .expect("ties are only reached after a sort");
                    let first = &sorted[0];
                    let ascend = if first.descending != *last { "False" } else { "True" };
                    let grouped = if by.is_empty() {
                        format!("d[{}]", text(&first.column.text))
                    } else {
                        format!(
                            "d.{}[{}]",
                            grouped(by),
                            text(&first.column.text)
                        )
                    };
                    calls.push(format!(
                        "loc[lambda d: {grouped}.rank(method=\"min\", ascending={ascend}) <= {count}]"
                    ));
                    calls.push(sort_by(sorted, last_missing_first(plan, i)));
                }

                Step::Take { count, by, last, .. } => {
                    let end = if *last { "tail" } else { "head" };
                    if by.is_empty() {
                        calls.push(format!("{end}({count})"));
                    } else {
                        calls.push(format!(
                            "groupby({}, as_index=False, dropna=False).{end}({count})",
                            list(by)
                        ));
                        if let Some(keys) = last_sort(plan, i) {
                            calls.push(sort_by(keys, last_missing_first(plan, i)));
                        }
                    }
                }

                Step::Join { other, by, unmatched, .. } => {
                    let how = match unmatched {
                        Unmatched::This => "left",
                        Unmatched::None => "inner",
                        Unmatched::Both => "outer",
                    };
                    // **`merge` joins a missing key to a missing key**, and a
                    // missing value equals nothing on every engine the sentence
                    // runs on. So the other table comes in without the rows
                    // whose key is missing, and a full join hands those back
                    // afterwards, unmatched, the way the query does.
                    let theirs: Vec<Name> = by.iter().map(|k| k.other.clone()).collect();
                    let partner = format!("{}.dropna(subset={})", other.text, list(&theirs));
                    // **`on=` and `left_on=`/`right_on=` cannot be mixed**, so
                    // one differing key puts every key into the long form. That
                    // is pandas' rule rather than a choice made here.
                    let merged = if by.iter().all(JoinKey::is_same) {
                        let keys: Vec<Name> = by.iter().map(|k| k.this.clone()).collect();
                        format!("merge({partner}, on={}, how=\"{how}\")", list(&keys))
                    } else {
                        let mine: Vec<Name> = by.iter().map(|k| k.this.clone()).collect();
                        format!(
                            "merge({partner}, left_on={}, right_on={}, how=\"{how}\")",
                            list(&mine),
                            list(&theirs)
                        )
                    };
                    if *unmatched == Unmatched::Both {
                        let set_aside = if theirs.len() == 1 {
                            format!("{0}[{0}[{1}].isna()]", other.text, text(&theirs[0].text))
                        } else {
                            format!("{0}[{0}[{1}].isna().any(axis=1)]", other.text, list(&theirs))
                        };
                        calls.push(format!(
                            "pipe(lambda d: pd.concat([d.{merged}, {set_aside}], ignore_index=True))"
                        ));
                    } else {
                        calls.push(merged);
                    }
                    if !by.iter().all(JoinKey::is_same) {
                        // **pandas keeps both key columns here and coalesces
                        // neither**, which is the difference from `on=` and the
                        // place this rendering would go quietly wrong. An outer
                        // join's other-side-only rows carry the key under the
                        // other table's name alone, so it is filled across
                        // before the column is dropped — the same answer the SQL
                        // backend gets from `COALESCE`.
                        if *unmatched == Unmatched::Both {
                            let filled: Vec<String> = by
                                .iter()
                                .filter(|k| !k.is_same())
                                .map(|k| {
                                    format!(
                                        "{}=lambda d: d[{}].fillna(d[{}])",
                                        k.this.text,
                                        text(&k.this.text),
                                        text(&k.other.text)
                                    )
                                })
                                .collect();
                            calls.push(format!("assign({})", filled.join(", ")));
                        }
                        let dropped: Vec<Name> = by
                            .iter()
                            .filter(|k| !k.is_same())
                            .map(|k| k.other.clone())
                            .collect();
                        calls.push(format!("drop(columns={})", list(&dropped)));
                    }
                }

                // **`pd.concat` is a function over two frames rather than a
                // method on one**, so this is the one verb that has to step out
                // of the chain and back into it. `.pipe` is how pandas does that.
                Step::AddRows { other, .. } => calls.push(format!(
                    "pipe(lambda d: pd.concat([d, {}], ignore_index=True))",
                    other.text
                )),

                // **pandas has no word for this, so the rendering is the
                // mechanism**: build the grid of combinations, then merge it
                // back with `how="outer"` so the rows that were not there
                // arrive with everything else missing.
                //
                // `MultiIndex.from_product` is the shorter spelling and is
                // wrong here in a way that only shows up on real data: it needs
                // the key columns to be unique, and reindexing onto it drops or
                // raises when they are not. A grid built by merging holds
                // whatever the table holds.
                Step::AddCombinations { names, by, .. } => {
                    let held: Vec<String> = by.iter().map(|n| n.text.clone()).collect();
                    let mut grid = String::new();
                    for (k, n) in names.iter().enumerate() {
                        let mut wanted = held.clone();
                        wanted.push(n.text.clone());
                        let distinct = format!(
                            "d[{}].dropna().drop_duplicates()",
                            strings(&wanted)
                        );
                        if k == 0 {
                            grid = distinct;
                        } else if held.is_empty() {
                            grid = format!("{grid}.merge({distinct}, how=\"cross\")");
                        } else {
                            grid = format!(
                                "{grid}.merge({distinct}, on={}, how=\"inner\")",
                                strings(&held)
                            );
                        }
                    }
                    let keys: Vec<String> =
                        held.iter().cloned().chain(names.iter().map(|n| n.text.clone())).collect();
                    calls.push(format!(
                        "pipe(lambda d: d.merge({grid}, on={}, how=\"outer\"))",
                        strings(&keys)
                    ));
                }

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
                    // **Grouped and unstacked rather than `pivot_table`, because
                    // a missing key is a group.** `pivot_table` drops a row whose
                    // `by` is missing, and its `dropna=False` does worse with two
                    // keys: it builds every combination of them, rows the table
                    // never had. A `groupby` that keeps the missing key, then the
                    // names turned into columns, is the same table the query
                    // makes.
                    let mut keys: Vec<String> = by.iter().map(|n| n.text.clone()).collect();
                    keys.extend(pieces.iter().map(|p| p.to_string()));
                    // A total of no values is missing, as in `summarize`.
                    let reduce = if how == "sum" {
                        "sum(min_count=1)".to_string()
                    } else {
                        format!("agg({})", text(how))
                    };
                    calls.push(format!(
                        "groupby({}, dropna=False)[{held}].{reduce}",
                        strings(&keys)
                    ));
                    // A count of nothing is 0, which is what the query answers
                    // for a combination the table never had.
                    if how == "nunique" {
                        calls.push(format!("unstack({columns}, fill_value=0)"));
                    } else {
                        calls.push(format!("unstack({columns})"));
                    }
                    if let Some(filler) = missing {
                        calls.push(format!("fillna({})", value_of(filler)));
                    }
                    // The grouping is in the index and the column axis has a
                    // name, and neither is a column a later step could name.
                    // Both are put back.
                    calls.push("reset_index()".to_string());
                    calls.push("rename_axis(columns=None)".to_string());
                }
            }
        }

        super::entering(None);
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
        // `groupby` and `unstack` for the reason `widen` gives: `pivot_table`
        // drops a row whose kept column is missing.
        index.push("__value".to_string());
        calls.push(format!(
            "groupby({}, dropna=False)[{}].first()",
            strings(&index),
            text(&held)
        ));
        calls.push("unstack(\"__value\")".to_string());
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
fn aggregation(value: &Expr, by: &[Name]) -> (String, String) {
    let fallback = by.first().map(|n| n.text.clone()).unwrap_or_default();
    let Expr::Call { name: fname, args, .. } = value else {
        return (fallback, text("first"));
    };
    let column = match args.first() {
        Some(Expr::Column(n)) => n.text.clone(),
        _ => fallback,
    };
    // **The one aggregate here that is a callable rather than a method name.**
    // pandas has no `str.join` among the named aggregations, so this is the
    // lambda a pandas user would write, and `dropna` is what makes it skip the
    // absent values the way the grammar's word says.
    //
    // **And it answers missing when there is nothing to join**, as the query
    // does: `str.join` of nothing is an empty text, which is a value.
    if fname == "join_rows" {
        let separator = args.get(1).map(value_of).unwrap_or_else(|| text(""));
        return (
            column,
            format!("lambda s: {separator}.join(s.dropna()) if s.notna().any() else None"),
        );
    }
    // **Three named aggregations are callables, and each says what a word
    // says that pandas' own name does not.** `"sum"` totals nothing to 0, and
    // a total of no values is missing, so `min_count=1` asks for one.
    // `"first"` and `"last"` skip a missing value, and the grammar's words
    // read the row, whatever it holds, the way every other target does.
    let call = match fname.as_str() {
        "total" => Some("lambda s: s.sum(min_count=1)"),
        "first" => Some("lambda s: s.iloc[0]"),
        "last" => Some("lambda s: s.iloc[-1]"),
        _ => None,
    };
    if let Some(call) = call {
        return (column, call.to_string());
    }
    let how = match fname.as_str() {
        "total" => "sum",
        "average" => "mean",
        "median" => "median",
        "smallest" => "min",
        "largest" => "max",
        "standard_deviation" => "std",
        "first" => "first",
        "last" => "last",
        "unique_count" => "nunique",
        "row_count" => "size",
        _ => "first",
    };
    (column, text(how))
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
        "standard_deviation" => "std",
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

/// A grouping that keeps the rows whose key is missing.
///
/// **pandas drops them by default and nobody else does.** DuckDB, Spark, dplyr
/// and polars all answer a missing key as a group of its own, so `groupby`
/// alone printed an answer one group short, with nothing said.
fn grouped(by: &[Name]) -> String {
    format!("groupby({}, dropna=False)", list(by))
}

fn sorted(names: &[Name]) -> String {
    format!("sort_values({})", list(names))
}

fn sort_by(keys: &[SortKey], missing_first: bool) -> String {
    let columns: Vec<String> = keys.iter().map(|k| text(&k.column.text)).collect();
    let mut args = vec![format!("[{}]", columns.join(", "))];
    if keys.iter().any(|k| k.descending) {
        let flags: Vec<String> = keys
            .iter()
            .map(|k| if k.descending { "False" } else { "True" }.to_string())
            .collect();
        args.push(format!("ascending=[{}]", flags.join(", ")));
    }
    // pandas already puts a missing value last, in both directions, so the
    // default needs nothing written. `na_position` takes one value for the
    // whole call rather than one per column — which is why the grammar's clause
    // is a trailing one.
    if missing_first {
        args.push("na_position=\"first\"".to_string());
    }
    format!("sort_values({})", args.join(", "))
}

/// Where that same `sort` put its missing values, which a restatement carries.
fn last_missing_first(plan: &Plan, before: usize) -> bool {
    plan.steps[..before]
        .iter()
        .rev()
        .find_map(|step| match step {
            Step::Sort { missing_first, .. } => Some(*missing_first),
            _ => None,
        })
        .unwrap_or(false)
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
        format!("d.{}[{}]", grouped(over.partition), text(name))
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
        // **A comparison kept as a value answers missing where a part of it is
        // missing**, which is what the query does and what numpy's booleans
        // cannot hold: `>` answers False over a hole and `!=` answers True. So
        // each one is made a nullable boolean and masked where it does not
        // know. pandas' nullable `&`, `|` and `~` then reason about the unknown
        // the way SQL does, so a compound needs nothing more. A condition that
        // only keeps rows is written by `test` below and needs none of this.
        Expr::Compare { op, left, right, .. } => {
            unknowable(compared(*op, &go(left), &go(right)), &[left, right], over)
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
            let test = unknowable(
                format!("{}.isin([{}])", go(left), values.join(", ")),
                &[left],
                over,
            );
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
        // **A quantified condition never reaches a backend**, because the
        // checker expands it into ordinary conditions before anything renders
        // (§13.11's move, for a question). It is written out in the grammar's
        // own words rather than panicking, so that the drawing of a sentence
        // that did *not* check still has something to show.
        Expr::Quantified { every, .. } => {
            format!("# {} of the matched columns", if *every { "every" } else { "any" })
        }

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
        Expr::TextTest { op, left, value: v, .. } => {
            unknowable(text_test(*op, &go(left), &flat(v)), &[left, v], over)
        }
        Expr::ColumnValue { .. } => "value".to_string(),
        Expr::ColumnKind { .. } => "kind".to_string(),
        // **pandas has nothing of its own that reads better than this.** Nested
        // `where` calls invert the order the arms were written in, so `np.select`
        // is what pandas users reach for, and it keeps first-match-wins.
        Expr::When { arms, otherwise, .. } => {
            // An arm takes the rows its question is true for, and an unknown
            // answer falls through to the next one, as `CASE WHEN` does.
            let tests: Vec<String> = arms.iter().map(|(t, _)| test(t, over)).collect();
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
        Expr::Call { name: fname, args, .. } if fname == "between" => {
            let parts: Vec<&Expr> = args.iter().collect();
            if parts[1..].iter().all(|b| !can_be_missing(b)) {
                unknowable(call(fname, args, over), &parts[..1], over)
            } else {
                // A bound that can be missing leaves the answer known whenever
                // the other comparison is false, so this one is written as the
                // two comparisons it is, and the nullable `&` decides.
                format!(
                    "({} & {})",
                    unknowable(compared(Compare::GreaterOrEqual, &go(&args[0]), &go(&args[1])), &[&args[0], &args[1]], over),
                    unknowable(compared(Compare::LessOrEqual, &go(&args[0]), &go(&args[2])), &[&args[0], &args[2]], over)
                )
            }
        }
        Expr::Call { name: fname, args, .. } => call(fname, args, over),

        // `np.select`, the same spelling the conditional gets. pandas' own
        // lookup is `.map`, and it sends every unpaired value missing — which
        // would silently erase the `otherwise` the sentence wrote — so the
        // faithful rendering is the one that carries the fallback.
        Expr::Lookup { subject, pairs, otherwise, .. } => {
            let tests: Vec<String> = pairs
                .iter()
                .map(|(from, _)| format!("({} == {})", go(subject), flat(from)))
                .collect();
            let values: Vec<String> = pairs.iter().map(|(_, to)| flat(to)).collect();
            format!(
                "np.select([{}], [{}], default={})",
                tests.join(", "),
                values.join(", "),
                flat(otherwise)
            )
        }

        // **`.rolling(n)` counts present values toward its floor**, and its
        // floor defaults to the window size, so pandas' own default is the
        // grammar's rule already: missing until the window holds n values, a
        // hole in a full window included. The grouped form goes through
        // `transform` because `.rolling` on a grouped column hands back a
        // two-level index that `assign` cannot line up.
        Expr::Rolling { agg, args, count, .. } => {
            let method = match agg.as_str() {
                "total" => "sum",
                "average" => "mean",
                "median" => "median",
                "smallest" => "min",
                "largest" => "max",
                "standard_deviation" => "std",
                other => unreachable!("`{other}` reached the pandas backend inside `rolling`"),
            };
            let n = match count.as_ref() {
                Expr::Whole { value, .. } => *value,
                _ => unreachable!("the checker admits only a written whole number"),
            };
            match args.first() {
                Some(Expr::Column(c)) if !over.partition.is_empty() => format!(
                    "d.{}[{}].transform(lambda s: s.rolling({n}).{method}())",
                    grouped(over.partition),
                    text(&c.text)
                ),
                Some(Expr::Column(c)) => {
                    format!("d[{}].rolling({n}).{method}()", text(&c.text))
                }
                _ => unreachable!("the checker admits only a plain column"),
            }
        }
    }
}

/// A comparison as numpy answers it: False over a hole, except `!=`.
fn compared(op: Compare, left: &str, right: &str) -> String {
    let symbol = match op {
        Compare::Is => "==",
        Compare::IsNot => "!=",
        Compare::Less => "<",
        Compare::LessOrEqual => "<=",
        Compare::Greater => ">",
        Compare::GreaterOrEqual => ">=",
    };
    format!("({left} {symbol} {right})")
}

/// A text test, False over a hole. `na=False` says so rather than leaving it
/// to the column's type: an object column answers NaN there, which `loc`
/// refuses.
fn text_test(op: TextOp, left: &str, value: &str) -> String {
    match op {
        TextOp::Starts => format!("{left}.str.startswith({value}, na=False)"),
        TextOp::Ends => format!("{left}.str.endswith({value}, na=False)"),
        TextOp::Contains => format!("{left}.str.contains({value}, regex=False, na=False)"),
    }
}

/// Whether a value can be missing, which a written one cannot.
fn can_be_missing(e: &Expr) -> bool {
    !matches!(
        e,
        Expr::Text { .. } | Expr::Whole { .. } | Expr::Decimal { .. } | Expr::Truth { .. }
    )
}

/// A test kept as a value: a nullable boolean, missing wherever one of its
/// parts is. Nothing changes where every part is written out.
fn unknowable(test: String, parts: &[&Expr], over: Over) -> String {
    let holes: Vec<String> = parts
        .iter()
        .filter(|p| can_be_missing(p))
        .map(|p| format!("{}.isna()", inner(p, over)))
        .collect();
    if holes.is_empty() {
        test
    } else {
        format!("{test}.astype(\"boolean\").mask({})", holes.join(" | "))
    }
}

/// `!=`, made to answer False over a hole the way every other comparison does.
fn differs(left: &Expr, right: &Expr, over: Over) -> String {
    let mut parts = vec![compared(Compare::IsNot, &inner(left, over), &inner(right, over))];
    for side in [left, right] {
        if can_be_missing(side) {
            parts.push(format!("{}.notna()", inner(side, over)));
        }
    }
    format!("({})", parts.join(" & "))
}

/// A condition, as the rows it keeps.
///
/// **A row is kept where the condition is true, and an unknown answer is not
/// true.** numpy's booleans have no unknown, so pandas answers False for a
/// comparison over a hole, which keeps nothing, and that much agrees with the
/// query. It goes wrong in exactly two places: `!=` answers True over a hole,
/// and `~` turns every False, the unknown ones included, into True. So `not`
/// is pushed inward to the comparisons, where it becomes their opposite
/// (`<` for `>=`, `|` for `&`), and the two that would keep a hole say they
/// do not, with `.notna()`. What is printed is the filter a careful pandas user
/// writes by hand.
fn test(e: &Expr, over: Over) -> String {
    match e {
        Expr::Compare { op: Compare::IsNot, left, right, .. } => differs(left, right, over),
        Expr::Compare { op, left, right, .. } => {
            compared(*op, &inner(left, over), &inner(right, over))
        }
        Expr::Logic { op, left, right, .. } => {
            let symbol = match op {
                Logic::And => "&",
                Logic::Or => "|",
            };
            format!("({} {symbol} {})", test(left, over), test(right, over))
        }
        Expr::Not { inner: i, .. } => refuted(i, over),
        Expr::In { left, set, negated, .. } => {
            let values: Vec<String> = set.iter().map(|v| inner(v, Over::default())).collect();
            let found = format!("{}.isin([{}])", inner(left, over), values.join(", "));
            if *negated {
                format!("(~{found} & {}.notna())", inner(left, over))
            } else {
                found
            }
        }
        Expr::TextTest { op, left, value, .. } => {
            text_test(*op, &inner(left, over), &inner(value, Over::default()))
        }
        Expr::Call { name, args, .. } if name == "between" => call(name, args, over),
        other => inner(other, over),
    }
}

/// A condition, as the rows where it is false: `test` of its opposite.
fn refuted(e: &Expr, over: Over) -> String {
    match e {
        Expr::Compare { op, left, right, .. } => {
            let opposite = match op {
                Compare::Is => return differs(left, right, over),
                Compare::IsNot => Compare::Is,
                Compare::Less => Compare::GreaterOrEqual,
                Compare::LessOrEqual => Compare::Greater,
                Compare::Greater => Compare::LessOrEqual,
                Compare::GreaterOrEqual => Compare::Less,
            };
            compared(opposite, &inner(left, over), &inner(right, over))
        }
        Expr::Logic { op, left, right, .. } => {
            let symbol = match op {
                Logic::And => "|",
                Logic::Or => "&",
            };
            format!("({} {symbol} {})", refuted(left, over), refuted(right, over))
        }
        Expr::Not { inner: i, .. } => test(i, over),
        Expr::In { left, set, negated, .. } => {
            let values: Vec<String> = set.iter().map(|v| inner(v, Over::default())).collect();
            let found = format!("{}.isin([{}])", inner(left, over), values.join(", "));
            if *negated {
                found
            } else {
                format!("(~{found} & {}.notna())", inner(left, over))
            }
        }
        Expr::IsMissing { inner: i, negated, .. } => {
            let method = if *negated { "isna" } else { "notna" };
            format!("{}.{method}()", inner(i, over))
        }
        Expr::TextTest { op, left, value, .. } => format!(
            "(~{} & {}.notna())",
            text_test(*op, &inner(left, over), &inner(value, Over::default())),
            inner(left, over)
        ),
        Expr::Call { name, args, .. } if name == "between" => format!(
            "({} | {})",
            compared(Compare::Less, &inner(&args[0], over), &inner(&args[1], over)),
            compared(Compare::Greater, &inner(&args[0], over), &inner(&args[2], over))
        ),
        Expr::Truth { value, .. } => if *value { "False" } else { "True" }.to_string(),
        other => format!("~{}", test(other, over)),
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
            "standard_deviation" => "std",
            "first" => "first",
            "last" => "last",
            "unique_count" => "nunique",
            "row_count" => "size",
            _ => unreachable!("every aggregate has a transform word"),
        };
        let groups = grouped(over.partition);
        let column = match (fname, args.first()) {
            ("row_count", _) => text(&over.partition[0].text),
            (_, Some(Expr::Column(n))) => text(&n.text),
            _ => unreachable!("refused before rendering"),
        };
        // The same three corrections the named aggregations make.
        let how = match fname {
            "total" => format!("{}, min_count=1", text(word)),
            "first" => "lambda s: s.iloc[0]".to_string(),
            "last" => "lambda s: s.iloc[-1]".to_string(),
            _ => text(word),
        };
        return format!("d.{groups}[{column}].transform({how})");
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
        // grammar's `total` means. `min_count=1` is the other half: a total of
        // no values is missing, and pandas' own answer is 0.
        "total" => format!("{}.sum(min_count=1)", arg(0)),
        "average" => format!("{}.mean()", arg(0)),
        "median" => format!("{}.median()", arg(0)),
        "smallest" => format!("{}.min()", arg(0)),
        "largest" => format!("{}.max()", arg(0)),
        // `.std()` is the sample deviation — `ddof=1` is pandas' own default,
        // which is the definition the grammar's word names.
        "standard_deviation" => format!("{}.std()", arg(0)),
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
        // The same idea off a group: the column's absent values dropped, the
        // rest run together.
        "join_rows" => format!(
            "({}.join({1}.dropna()) if {1}.notna().any() else None)",
            inner(&args[1], plain),
            arg(0)
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
        // Python's `%` is floored, which is the convention the grammar names.
        "remainder" => format!("({} % {})", arg(0), arg(1)),
        "latest" => windowed("ffill()"),
        "previous" => windowed(&format!("shift({})", super::step_alone(args, ""))),
        "following" => windowed(&format!("shift({})", super::step_alone(args, "-"))),
        "to_number" => format!("pd.to_numeric({})", arg(0)),
        // **`np.trunc` first, and it fixes two things at once.** A bare
        // `.astype("Int64")` *raises* on a value with a fractional part in
        // pandas 3 — "cannot safely cast" — so this rendering did not merely
        // disagree with the others, it would not run. And truncation toward
        // zero is the convention the grammar names, which DuckDB needed
        // `trunc()` to reach as well. Missing values survive as missing.
        // `np.floor` before the cast for the same reason R needs it, and the
        // cast is not optional: a bare `.astype("Int64")` raises on a value
        // with a fractional part in pandas 3, so the rounding has to happen
        // first whatever else is true.
        "round_below" => format!("np.floor({}).astype(\"Int64\")", arg(0)),
        "round_above" => format!("np.ceil({}).astype(\"Int64\")", arg(0)),
        // **A whole number is written whole, however it is stored**, which is
        // the query's rule: a column with a hole in it is a float column in
        // pandas, and `astype` writes its 100 as `100.0`. Past 1e15 a float
        // stops holding whole numbers exactly, so it is written as it is.
        "to_text" if args.first().is_some_and(super::holds_number) => format!(
            "{}.map(lambda v: str(int(v)) if float(v).is_integer() and abs(v) < 1e15 else str(v), na_action=\"ignore\")",
            arg(0)
        ),
        "to_text" => format!("{}.astype(\"string\")", arg(0)),
        // **`format="mixed"` and `.dt.normalize()` are both corrections, and both
        // were found by running this against the engine rather than reading it.**
        //
        // `normalize` is the one that matters: a bare `pd.to_datetime` keeps the
        // time, and every other target — DuckDB, Spark, dplyr, PySpark — throws
        // it away, because `to_date` converts into a *date* and a date has no
        // hour in it. Keeping it made one sentence answer 14 on the printed
        // pandas and 0 on the engine that ran it.
        //
        // `format="mixed"` is the second: a column holding both
        // `2026-01-02 14:30:00` and `2026-03-15` is one DuckDB parses without
        // complaint and a bare `pd.to_datetime` *raises* on, so the printed code
        // did not merely disagree, it stopped.
        "to_date" => format!("pd.to_datetime({}, format=\"mixed\").dt.normalize()", arg(0)),
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
