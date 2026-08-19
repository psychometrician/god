//! A plan as PySpark, for reading rather than for running.
//!
//! Nothing executes this. It exists so that someone can ask what a sentence
//! would be in a language they already know, and get an answer they recognize
//! immediately:
//!
//! ```text
//! sales then keep where [region] is "West" then take 10
//!
//! (sales
//!     .filter(F.col("region") == "West")
//!     .limit(10))
//! ```
//!
//! **This is the backend a notebook reader is most likely to want**, because the
//! cluster they are on already has the frame in hand and a query string is one
//! more thing to hand somewhere. It is the same escape hatch every printing
//! backend is, pointed at the place a large table actually lives.
//!
//! `F` is `pyspark.sql.functions` and `Window` is `pyspark.sql.Window`, which is
//! what nearly every notebook already calls them. The imports are not written
//! out, for the same reason the dplyr rendering does not write `library(dplyr)`:
//! the reader is being shown a sentence, not a script.
//!
//! **Three shapes do not line up one-to-one.** A filtering join is a `how=` here
//! rather than a condition. The first rows of each group need a numbered window
//! and a filter, because Spark has no grouped head. And a window has to carry
//! both the step's grouping and the expression's own ordering into one `Window`,
//! where the grammar states them in two places.

use super::Backend;
use crate::check::Schema;
use crate::plan::*;

/// The column a grouped `take` counts with, and drops again.
const RANK: &str = "__place";

pub struct PySpark;

/// What a window is worked out over: the grouping it partitions by, and the
/// ordering it falls back to when the expression does not carry its own.
#[derive(Default, Clone)]
struct Over<'a> {
    partition: &'a [Name],
    order: Option<&'a [SortKey]>,
    /// Whether this expression stands in an `add`, where an aggregate spans
    /// its group and every row keeps the answer. Spark spells that
    /// `.over(Window.partitionBy(...))`, and with no `by` the window is the
    /// whole frame, `Window.partitionBy()` with nothing inside. Without the
    /// flag an aggregate in `withColumn` is an analysis error, and one with a
    /// `by` was silently ignoring the grouping.
    windowed: bool,
}

impl Backend for PySpark {
    fn name(&self) -> &'static str {
        "pyspark"
    }

    fn render(&self, plan: &Plan, entering: &[Schema]) -> String {
        let mut calls: Vec<String> = Vec::new();

        for (i, step) in plan.steps.iter().enumerate() {
            match step {
                Step::Keep { condition, .. } => match filtering_join(condition) {
                    Some((other, by, negated)) => calls.push(format!(
                        "join({}{}, on={}, how=\"{}\")",
                        other.text,
                        renames(by),
                        list(&by.iter().map(|k| k.this.clone()).collect::<Vec<_>>()),
                        if negated { "anti" } else { "semi" }
                    )),
                    None => calls.push(format!("filter({})", expr(condition))),
                },

                Step::Pick { names, all_but, .. } => {
                    let listed: Vec<String> = names.iter().map(|n| text(&n.text)).collect();
                    let verb = if *all_but { "drop" } else { "select" };
                    calls.push(format!("{verb}({})", listed.join(", ")));
                }

                // One `withColumn` per value, which is what a Spark reader
                // writes and what a chain of them already looks like.
                Step::Add { values, by, .. } => {
                    let over = Over { partition: by, order: last_sort(plan, i), windowed: true };
                    for v in values {
                        calls.push(format!(
                            "withColumn({}, {})",
                            text(&v.name.text),
                            expr_over(&v.value, over.clone())
                        ));
                    }
                    if values.iter().any(|v| v.value.windows()) {
                        if let Some(keys) = last_sort(plan, i) {
                            calls.push(sort_by(keys, last_missing_first(plan, i)));
                        }
                    }
                }

                Step::Summarize { values, by, .. } => {
                    let args: Vec<String> = values
                        .iter()
                        .map(|v| format!("{}.alias({})", expr(&v.value), text(&v.name.text)))
                        .collect();
                    if by.is_empty() {
                        calls.push(format!("agg({})", args.join(", ")));
                    } else {
                        let groups: Vec<String> = by.iter().map(|n| text(&n.text)).collect();
                        calls.push(format!(
                            "groupBy({}).agg({})",
                            groups.join(", "),
                            args.join(", ")
                        ));
                        // Grouping promises nothing about the order the groups
                        // come back in, so they are ordered by what defines them.
                        calls.push(ordered(by));
                    }
                }

                Step::Sort { keys, missing_first, .. } => {
                    calls.push(sort_by(keys, *missing_first))
                }

                Step::Take { count, by, last, ties, .. } if *ties => {
                    // Spark already needs a numbered window for a grouped
                    // `take`, so this is that machinery with `rank` in place of
                    // `row_number` — which is the one-word difference between
                    // breaking ties arbitrarily and keeping them.
                    let sorted = last_sort(plan, i)
                        .expect("ties are only reached after a sort");
                    let counted = if *last { flipped(sorted) } else { sorted.to_vec() };
                    calls.push(format!(
                        "withColumn({}, F.rank().over({}))",
                        text(RANK),
                        window(
                            &Over { partition: by, order: Some(&counted), windowed: false },
                            &[]
                        )
                    ));
                    calls.push(format!("filter(F.col({}) <= {count})", text(RANK)));
                    calls.push(format!("drop({})", text(RANK)));
                    calls.push(sort_by(sorted, last_missing_first(plan, i)));
                }

                Step::Take { count, by, last, .. } => {
                    if by.is_empty() {
                        if *last {
                            // **Spark's `tail` returns rows to the driver rather
                            // than a frame**, so it cannot stand in a chain. The
                            // sort is walked backwards, the first rows are taken
                            // from that end, and the caller's order is restored.
                            let keys = last_sort(plan, i)
                                .expect("take_last is only reached after a sort");
                            calls.push(sort_by(&flipped(keys), !last_missing_first(plan, i)));
                            calls.push(format!("limit({count})"));
                            calls.push(sort_by(keys, last_missing_first(plan, i)));
                        } else {
                        calls.push(format!("limit({count})"));
                        }
                    } else {
                        // **Spark has no grouped head**, so the rows are numbered
                        // within each group and the numbering is filtered. The
                        // sort before it says what "first" means, and the order
                        // has to be restated after the filter because a filter
                        // promises nothing about it.
                        let keys = last_sort(plan, i).unwrap_or_default();
                        // Numbering from the far end is the same window counting
                        // the other way.
                        let counted = if *last { flipped(keys) } else { keys.to_vec() };
                        calls.push(format!(
                            "withColumn({}, F.row_number().over({}))",
                            text(RANK),
                            window(&Over { partition: by, order: Some(&counted), windowed: false }, &[])
                        ));
                        calls.push(format!("filter(F.col({}) <= {count})", text(RANK)));
                        calls.push(format!("drop({})", text(RANK)));
                        if !keys.is_empty() {
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
                    // **A differing pair is renamed rather than joined on a
                    // condition**, and that is the whole reason this backend
                    // reads the way it does. `on=` as a *condition* is what
                    // PySpark reaches for, and it gives up the two things the
                    // list form provides for free: Spark stops coalescing the
                    // key on a full join, and both columns come back, one of
                    // them ambiguous to refer to afterwards. Renaming the other
                    // table's column first keeps the list form, so every join
                    // kind behaves as the same-named case already does.
                    calls.push(format!(
                        "join({}{}, on={}, how=\"{how}\")",
                        other.text,
                        renames(by),
                        list(&by.iter().map(|k| k.this.clone()).collect::<Vec<_>>())
                    ));
                }

                // By name rather than by position, which is the one that means
                // what the sentence said. The checker has already refused a pair
                // of tables whose columns differ.
                Step::AddRows { other, .. } => {
                    calls.push(format!("unionByName({})", other.text))
                }

                // The same mechanism the other two need. Spark's `full` join
                // coalesces the keys for itself when `on` is a list of names
                // rather than a condition, so this needs no `coalesce=`: the
                // list is the spelling that says the two sides mean one column.
                Step::AddCombinations { names, by, .. } => {
                    let held: Vec<String> = by.iter().map(|n| n.text.clone()).collect();
                    let mut grid = String::new();
                    for (k, n) in names.iter().enumerate() {
                        let mut wanted = held.clone();
                        wanted.push(n.text.clone());
                        let distinct = format!(
                            "d.select({}).dropna().distinct()",
                            strings(&wanted)
                                .trim_start_matches('[')
                                .trim_end_matches(']')
                        );
                        if k == 0 {
                            grid = distinct;
                        } else if held.is_empty() {
                            grid = format!("{grid}.crossJoin({distinct})");
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
                        "transform(lambda d: d.join({grid}, on={}, how=\"full\"))",
                        strings(&keys)
                    ));
                }

                Step::DropDuplicates { .. } => {
                    calls.push("dropDuplicates()".to_string());
                    let all: Vec<String> =
                        entering[i].columns.iter().map(|(c, _)| c.clone()).collect();
                    calls.push(format!("orderBy({})", strings(&all)));
                }

                Step::Rename { values, .. } => {
                    for v in values {
                        match &v.value {
                            Expr::Column(from) => calls.push(format!(
                                "withColumnRenamed({}, {})",
                                text(&from.text),
                                text(&v.name.text)
                            )),
                            other => calls.push(format!(
                                "withColumn({}, {})",
                                text(&v.name.text),
                                expr(other)
                            )),
                        }
                    }
                }

                Step::DropMissing { names, .. } => {
                    if names.is_empty() {
                        calls.push("dropna()".to_string());
                    } else {
                        calls.push(format!("dropna(subset={})", list(names)));
                    }
                }

                Step::FillMissing { values, .. } => {
                    for v in values {
                        calls.push(format!(
                            "withColumn({}, F.coalesce(F.col({}), {}))",
                            text(&v.name.text),
                            text(&v.name.text),
                            as_column(&v.value)
                        ));
                    }
                }

                Step::Lengthen { resolved, .. } => {
                    let Some(shape) = resolved else { continue };
                    calls.extend(lengthen(shape));
                    calls.push(format!("orderBy({})", strings(&lengthen_order(shape))));
                }

                Step::Widen { name: pattern, value, by, missing, giving, .. } => {
                    let groups: Vec<String> = by.iter().map(|n| text(&n.text)).collect();
                    let pieces = pattern.named_parts();
                    // Spark pivots on one column. Where the grammar names
                    // several, they are joined into one first, which is what the
                    // query does too.
                    let on = if pieces.len() == 1 {
                        text(&pieces[0])
                    } else {
                        let joined: Vec<String> =
                            pieces.iter().map(|p| format!("F.col({})", text(p))).collect();
                        format!("F.concat_ws(\"_\", {})", joined.join(", "))
                    };
                    let made: Vec<String> = giving.iter().map(|n| text(&n.text)).collect();
                    let aggregate = match aggregate_of(value) {
                        Some((word, inner)) => format!("F.{word}({})", column_text(&inner)),
                        None => format!("F.first({})", column_text(value)),
                    };
                    let mut pivot = format!("pivot({on}");
                    if !made.is_empty() {
                        pivot.push_str(&format!(", [{}]", made.join(", ")));
                    }
                    pivot.push(')');
                    calls.push(format!(
                        "groupBy({}).{pivot}.agg({aggregate})",
                        groups.join(", ")
                    ));
                    if let Some(filler) = missing {
                        calls.push(format!("na.fill({})", expr(filler)));
                    }
                    if !by.is_empty() {
                        calls.push(ordered(by));
                    }
                }
            }
        }

        let head = head_of(&plan.source);
        if calls.is_empty() {
            return head;
        }
        format!("({head}\n    .{})", calls.join("\n    ."))
    }
}

/// The frame a pipeline starts from.
///
/// **A name in parts is a table in a catalog, and Spark is the one target that
/// has one.** `main.sales.orders` is not a variable a notebook holds; it is
/// something the session looks up, so it is written the way the session is
/// asked. A plain name is a plain name, which is the frame already in hand.
fn head_of(source: &str) -> String {
    if source.contains('.') {
        format!("spark.table({})", text(source))
    } else {
        source.to_string()
    }
}

/// `lengthen` as Spark's own `unpivot`, plus whatever taking the name apart needs.
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
        "unpivot(ids={}, values={}, variableColumnName={}, valueColumnName={})",
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
        // Spark counts the pieces from 1, which is the same way the grammar
        // does, so this index needs no adjustment.
        calls.push(format!(
            "withColumn({}, F.split_part(F.col({}), {}, {}))",
            text(piece),
            text(&holding),
            text(&separator),
            i + 1
        ));
    }
    calls.push(format!("drop({})", text(&holding)));

    if shape.value_columns.len() > 1 {
        let mut index = shape.keep.clone();
        index.extend(shape.name_columns.iter().cloned());
        let groups: Vec<String> = index.iter().map(|n| text(n)).collect();
        let made: Vec<String> = shape.value_columns.iter().map(|v| text(v)).collect();
        calls.push(format!(
            "groupBy({}).pivot(\"__value\", [{}]).agg(F.first({}))",
            groups.join(", "),
            made.join(", "),
            text(&held)
        ));
    }
    calls
}

/// The order a `lengthen` has to restate.
fn lengthen_order(shape: &Lengthened) -> Vec<String> {
    shape
        .keep
        .iter()
        .chain(shape.name_columns.iter())
        .chain(shape.value_columns.iter())
        .cloned()
        .collect()
}

/// The text between the pieces of a stacked column's name.
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

/// A `Window`, built from the grouping and whichever ordering applies.
fn window(over: &Over, key: &[SortKey]) -> String {
    let mut out = String::from("Window");
    if !over.partition.is_empty() {
        let groups: Vec<String> = over.partition.iter().map(|n| text(&n.text)).collect();
        out.push_str(&format!(".partitionBy({})", groups.join(", ")));
    }
    // The expression's own key wins where it has one: `rank` says what it ranks
    // by. Where it does not, the order is the one the rows already have, which
    // the checker has guaranteed by refusing the sentence without a `sort`.
    let ordering = if key.is_empty() { over.order.unwrap_or_default() } else { key };
    if !ordering.is_empty() {
        let written: Vec<String> = ordering.iter().map(sort_key).collect();
        out.push_str(&format!(".orderBy({})", written.join(", ")));
    }
    out
}

fn sort_key(k: &SortKey) -> String {
    if k.descending {
        format!("F.col({}).desc()", text(&k.column.text))
    } else {
        format!("F.col({})", text(&k.column.text))
    }
}

fn ordered(names: &[Name]) -> String {
    let written: Vec<String> = names.iter().map(|n| text(&n.text)).collect();
    format!("orderBy({})", written.join(", "))
}

/// The same keys, read from the other end.
///
/// `take_last` is `take` over a reversed order, so every target that has no word
/// for the far end spells it by flipping the sort. Written once here rather than
/// inline at each use.
fn flipped(keys: &[SortKey]) -> Vec<SortKey> {
    keys.iter()
        .map(|k| SortKey { column: k.column.clone(), descending: !k.descending })
        .collect()
}

fn sort_by(keys: &[SortKey], missing_first: bool) -> String {
    // **PySpark's own default is the one that disagrees**: ascending puts a
    // missing value first and descending puts it last, so the same column read
    // both ways moves the absent rows from one end to the other. Both ends are
    // therefore written out, and the four `asc_nulls_*`/`desc_nulls_*` methods
    // are what say it.
    let written: Vec<String> = keys
        .iter()
        .map(|k| {
            format!(
                "F.col({}).{}_nulls_{}()",
                text(&k.column.text),
                if k.descending { "desc" } else { "asc" },
                if missing_first { "first" } else { "last" }
            )
        })
        .collect();
    format!("orderBy({})", written.join(", "))
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

fn aggregate_of(value: &Expr) -> Option<(&'static str, Expr)> {
    let Expr::Call { name: fname, args, .. } = value else {
        return None;
    };
    if args.len() != 1 {
        return None;
    }
    let word = match fname.as_str() {
        "total" => "sum",
        "average" => "avg",
        "median" => "median",
        "smallest" => "min",
        "largest" => "max",
        "standard_deviation" => "stddev",
        "first" => "first",
        "last" => "last",
        "unique_count" => "countDistinct",
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

fn list(names: &[Name]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| text(&n.text)).collect();
    format!("[{}]", quoted.join(", "))
}

/// What the other table has to be renamed to before the join, so that every key
/// is one word on both sides.
///
/// Empty where the two tables already agree, which is every join written before
/// 2026-08-16 and most of them since.
fn renames(keys: &[JoinKey]) -> String {
    keys.iter()
        .filter(|k| !k.is_same())
        .map(|k| {
            format!(
                ".withColumnRenamed({}, {})",
                text(&k.other.text),
                text(&k.this.text)
            )
        })
        .collect::<Vec<_>>()
        .join("")
}

fn strings(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| text(n)).collect();
    format!("[{}]", quoted.join(", "))
}

fn text(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A value in a position that wants a `Column` rather than a plain Python value.
///
/// **Most of Spark's functions take a `Column` and raise on anything else**, so
/// `F.coalesce(F.col("cost"), 0)` does not quietly do the wrong thing: it stops
/// and says the argument should have been a column. That is the friendlier half
/// of this class of mistake. The unfriendly half is the one polars has, where a
/// bare string is read as a column name and the answer is wrong rather than
/// absent, so both are written out rather than trusted to a rule of thumb about
/// which functions are forgiving.
///
/// The column methods are the forgiving ones: `between`, `isin`, `startswith`
/// and `when` all take plain values, and wrapping those would only add noise.
fn as_column(e: &Expr) -> String {
    match e {
        Expr::Column(_) => expr(e),
        Expr::Text { .. }
        | Expr::Whole { .. }
        | Expr::Decimal { .. }
        | Expr::Truth { .. }
        | Expr::Missing { .. } => format!("F.lit({})", expr(e)),
        other => expr(other),
    }
}

fn expr(e: &Expr) -> String {
    expr_over(e, Over::default())
}

fn expr_over(e: &Expr, over: Over) -> String {
    let go = |inner: &Expr| expr_over(inner, over.clone());
    match e {
        Expr::Column(n) => format!("F.col({})", text(&n.text)),
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
        // Spark overloads the bitwise operators, so the parentheses are not
        // style: `&` binds tighter than `==` in Python and the expression means
        // something else without them.
        Expr::Logic { op, left, right, .. } => {
            let symbol = match op {
                Logic::And => "&",
                Logic::Or => "|",
            };
            format!("({} {symbol} {})", go(left), go(right))
        }
        Expr::Not { inner, .. } => format!("~{}", go(inner)),
        Expr::In { left, set, negated, .. } => {
            let values: Vec<String> = set.iter().map(|v| go(v)).collect();
            let test = format!("{}.isin({})", go(left), values.join(", "));
            if *negated {
                format!("~{test}")
            } else {
                test
            }
        }
        Expr::IsMissing { inner, negated, .. } => {
            if *negated {
                format!("{}.isNotNull()", go(inner))
            } else {
                format!("{}.isNull()", go(inner))
            }
        }
        // Unreachable in a checked plan: `matching` may only stand as a whole
        // `keep` condition, and the step above renders that case as the join
        // Spark actually has.
        Expr::Matching { other, .. } => format!("# matching({})", other.text),
        // **A quantified condition never reaches a backend**, because the
        // checker expands it into ordinary conditions before anything renders
        // (§13.11's move, for a question). It is written out in the grammar's
        // own words rather than panicking, so that the drawing of a sentence
        // that did *not* check still has something to show.
        Expr::Quantified { every, .. } => {
            format!("# {} of the matched columns", if *every { "every" } else { "any" })
        }

        // Spark's `rank` is competition ranking already: ties share a place and
        // the next one skips. `dense_rank` is the other one, and is not what a
        // person means by rank.
        Expr::Window { kind, key, .. } => {
            let word = match kind {
                Window::Rank => "F.rank()",
                Window::RowNumber => "F.row_number()",
            };
            let own: Vec<SortKey> = key.iter().cloned().collect();
            format!("{word}.over({})", window(&over, &own))
        }
        Expr::TextTest { op, left, value, .. } => match op {
            TextOp::Starts => format!("{}.startswith({})", go(left), go(value)),
            TextOp::Ends => format!("{}.endswith({})", go(left), go(value)),
            TextOp::Contains => format!("{}.contains({})", go(left), go(value)),
        },
        Expr::ColumnValue { .. } => "value".to_string(),
        Expr::ColumnKind { .. } => "kind".to_string(),
        Expr::When { arms, otherwise, .. } => {
            let mut out = String::new();
            for (test, value) in arms {
                out.push_str(&format!(
                    "{}when({}, {})",
                    if out.is_empty() { "F." } else { "." },
                    go(test),
                    go(value)
                ));
            }
            match otherwise {
                Some(fallback) => format!("{out}.otherwise({})", go(fallback)),
                None => out,
            }
        }
        Expr::ColumnName { .. } => "name".to_string(),
        Expr::Call { name: fname, args, .. } => call(fname, args, over),

        // The conditional's own chain, one `when` per pair, and the
        // `otherwise` is always written because the sentence always has one.
        Expr::Lookup { subject, pairs, otherwise, .. } => {
            let subj = go(subject);
            let mut out = String::new();
            for (from, to) in pairs {
                out.push_str(&format!(
                    "{}when(({subj} == {}), {})",
                    if out.is_empty() { "F." } else { "." },
                    go(from),
                    go(to)
                ));
            }
            format!("{out}.otherwise({})", go(otherwise))
        }

        // **The guard is written with `F.when` and no `otherwise`**, which is
        // how PySpark says "and missing everywhere else" — the full-window
        // rule. `F.median` refuses a frame on a live Spark session while
        // `F.percentile` takes the same frame and answers the same middle, so
        // the median goes through the second, exactly as the SQL dialect does.
        Expr::Rolling { agg, args, count, .. } => {
            let value = expr_over(&args[0], over.clone());
            let n = match count.as_ref() {
                Expr::Whole { value, .. } => *value,
                _ => unreachable!("the checker admits only a written whole number"),
            };
            let spec = format!(
                "{}.rowsBetween(-{}, Window.currentRow)",
                window(&over, &[]),
                n - 1
            );
            let asked = match agg.as_str() {
                "total" => format!("F.sum({value})"),
                "average" => format!("F.avg({value})"),
                "median" => format!("F.percentile({value}, 0.5)"),
                "smallest" => format!("F.min({value})"),
                "largest" => format!("F.max({value})"),
                "standard_deviation" => format!("F.stddev({value})"),
                other => unreachable!("`{other}` reached the PySpark backend inside `rolling`"),
            };
            format!("F.when(F.count({value}).over({spec}) == {n}, {asked}.over({spec}))")
        }
    }
}

/// How PySpark spells each of the grammar's functions.
fn call(fname: &str, args: &[Expr], over: Over) -> String {
    // An aggregate standing in an `add` is a window: the plain spelling below
    // is rendered first, then given the window its position demands. No
    // `orderBy` goes in it, because an ordered window would turn `sum` into a
    // running total, which is a different word in this grammar.
    if over.windowed && crate::vocabulary::is_aggregate(fname) {
        let plain = call(fname, args, Over { windowed: false, ..over.clone() });
        let spec = if over.partition.is_empty() {
            "Window.partitionBy()".to_string()
        } else {
            let groups: Vec<String> = over.partition.iter().map(|n| text(&n.text)).collect();
            format!("Window.partitionBy({})", groups.join(", "))
        };
        return format!("{plain}.over({spec})");
    }
    let arg = |i: usize| args.get(i).map(|a| expr_over(a, over.clone())).unwrap_or_default();
    match fname {
        // Spark has no `string_agg`: the group is collected into an array and
        // the array is joined. `collect_list` already drops the absent values,
        // which is this word's rule.
        "join_rows" => format!("F.array_join(F.collect_list({}), {})", arg(0), arg(1)),
        // Spark skips the absent value in an aggregate, which is what the
        // grammar's `total` means, so none of these needs an argument saying so.
        "total" => format!("F.sum({})", arg(0)),
        "average" => format!("F.avg({})", arg(0)),
        "median" => format!("F.median({})", arg(0)),
        "smallest" => format!("F.min({})", arg(0)),
        "largest" => format!("F.max({})", arg(0)),
        // `F.stddev` is an alias for `stddev_samp` in Spark's own words, which
        // is the sample deviation the grammar's word names.
        "standard_deviation" => format!("F.stddev({})", arg(0)),
        "first" => format!("F.first({})", arg(0)),
        "last" => format!("F.last({})", arg(0)),
        "unique_count" => format!("F.countDistinct({})", arg(0)),
        "row_count" => "F.count(\"*\")".to_string(),
        "first_present" => format!(
            "F.coalesce({})",
            args.iter().map(as_column).collect::<Vec<_>>().join(", ")
        ),
        // `F.concat` returns null when any argument is, which is the rule.
        // `concat_ws` is the one that skips them, and it is not wanted here.
        "join_text" => format!(
            "F.concat({})",
            args.iter().map(as_column).collect::<Vec<_>>().join(", ")
        ),
        "year" => format!("F.year({})", arg(0)),
        "month" => format!("F.month({})", arg(0)),
        "day" => format!("F.dayofmonth({})", arg(0)),
        "hour" => format!("F.hour({})", arg(0)),
        // **Spark's `dayofweek` starts on Sunday and the grammar starts on
        // Monday**, which is the difference that answers 4 where the grammar
        // says 5. `weekday` is the one that counts from Monday, at 0, so it is
        // shifted by one rather than swapped for the other function.
        "weekday" => format!("(F.weekday({}) + 1)", arg(0)),
        // **The frame is written out.** A running sum with an ordering and no
        // frame ties rows sharing a sort key together and gives them all the
        // same total, which looks right on any fixture with distinct keys.
        "running_total" => format!(
            "F.sum({}).over({}.rowsBetween(Window.unboundedPreceding, Window.currentRow))",
            arg(0),
            window(&over, &[])
        ),
        // **`pmod` and not `%`.** Spark's `%` truncates, so -7 % 2 is -1
        // there; `pmod` is its floored modulo and answers 1, which is the
        // convention the grammar names.
        "remainder" => format!("F.pmod({}, {})", arg(0), arg(1)),
        // **The frame is written out for the same reason `running_total`'s is,
        // and it matters more here.** `F.last` over an ordering with no frame
        // takes Spark's default `RANGE ... CURRENT ROW`, which groups every row
        // sharing a sort key — so on a tie a hole would be filled from a row
        // beside it rather than from the last one above it. Naming the frame is
        // what makes this fill downward and only downward.
        "latest" => format!(
            "F.last({}, ignorenulls=True).over({}.rowsBetween(Window.unboundedPreceding, Window.currentRow))",
            arg(0),
            window(&over, &[])
        ),
        "previous" => {
            format!("F.lag({}{}).over({})", arg(0), super::step(args), window(&over, &[]))
        }
        "following" => {
            format!("F.lead({}{}).over({})", arg(0), super::step(args), window(&over, &[]))
        }
        "to_number" => format!("{}.cast(\"double\")", arg(0)),
        "round_below" => format!("F.floor({})", arg(0)),
        "round_above" => format!("F.ceil({})", arg(0)),
        "to_text" => format!("{}.cast(\"string\")", arg(0)),
        "to_date" => format!("{}.cast(\"date\")", arg(0)),
        "trim" => format!("F.trim({})", arg(0)),
        "characters" => format!("F.length({})", arg(0)),
        // `replace` rather than `regexp_replace`, because the grammar's word
        // looks for text a person typed rather than a pattern.
        "replace_text" => format!(
            "F.replace({}, {}, {})",
            arg(0),
            args.get(1).map(as_column).unwrap_or_default(),
            args.get(2).map(as_column).unwrap_or_default()
        ),
        // Spark counts the pieces from 1, the same way the grammar does.
        "split_text" => format!(
            "F.split_part({}, {}, {})",
            arg(0),
            args.get(1).map(as_column).unwrap_or_default(),
            args.get(2).map(as_column).unwrap_or_default()
        ),
        "between" => format!("{}.between({}, {})", arg(0), arg(1), arg(2)),
        "lower" => format!("F.lower({})", arg(0)),
        "upper" => format!("F.upper({})", arg(0)),
        other => unreachable!("`{other}` reached the PySpark backend without a spelling"),
    }
}
