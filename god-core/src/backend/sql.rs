//! A plan as SQL: one step, one common table expression.
//!
//! **The shape of a pipeline and the shape of a chain of CTEs are the same
//! shape**, which is why this file is short. Each step reads from the one before
//! it by name, so the query reads top to bottom in the order the caller wrote,
//! and anyone who knows SQL can see exactly what their sentence asked for.
//!
//! ```sql
//! WITH step0 AS (SELECT * FROM sales),
//!      step1 AS (SELECT * FROM step0 WHERE "region" = 'West'),
//!      step2 AS (SELECT *, "revenue" - "cost" AS "margin" FROM step1)
//! SELECT * FROM step2
//! ```
//!
//! **One assumption is recorded rather than hidden.** `ORDER BY` inside a CTE is
//! not something the SQL standard promises to carry outward. Checked on the two
//! engines this targets — DuckDB and Spark 4.2 — and both carry it. The grammar's
//! own rule covers the rest: where an engine cannot honor what a sentence means,
//! that is a refusal rather than a quiet difference, so an engine that reordered
//! here would get a refusal on `sort` instead of a wrong answer nobody notices.
//!
//! **One renderer, two dialects, and the differences are a struct rather than a
//! second file.** `Dialect` holds them, and there are five. That is still a table
//! of spellings rather than a rewrite, which is the claim, but it is five and not
//! the one this comment used to say: that number was measured before six features
//! were added on top of it, and it went stale without anything noticing.
//!
//! **Two of the five decide whether the answer is right rather than whether the
//! query runs**, and both were found by running the corpus on both engines and
//! comparing rows. `SELECT "region"` is a column on DuckDB and the text
//! `'region'` on Spark, so the wrong quote returns the column's own name in every
//! row of every column, and the query looks perfect. A backslash inside a text
//! value is a backslash on DuckDB and an escape on Spark.
//!
//! **A check that asked "did the query parse" would have passed both.** That is
//! the whole argument for `parity/spark.py` running the corpus on two engines and
//! comparing tables, rather than a test asserting on the SQL.

use super::Backend;
use crate::check::Schema;
use crate::diagnostic::Diagnostic;
use crate::plan::*;
use crate::vocabulary;

/// The name the row number is parked under while a grouped `take` uses it.
///
/// A caller cannot collide with it: §11.2 reserves this one name, and it never
/// survives the step that makes it.
const RANK: &str = "__god_row";

/// The name the count of rows wanting one cell is parked under while `widen`
/// checks it. Reserved the same way `RANK` is, and it never survives its step.
const CELL: &str = "__god_cell";

/// What each column's distinct values are called while `add_combinations`
/// crosses them. Numbered, one per crossed column, and reserved the same way.
const GRID: &str = "__god_grid";

/// What the rows already in the table are called while `add_combinations` asks
/// which combinations are absent from them.
const ROWS: &str = "__god_rows";

/// The keys of the most recent `sort` before this step.
fn last_sort(plan: &Plan, before: usize) -> Option<&[SortKey]> {
    plan.steps[..before]
        .iter()
        .rev()
        .find_map(|step| match step {
            Step::Sort { keys, .. } => Some(keys.as_slice()),
            _ => None,
        })
}

/// How one engine spells the few things engines spell differently.
///
/// **A table of spellings rather than a second backend**, which is the claim
/// §15.2 makes and the reason a third engine is cheap. What is *not* a spelling
/// is recorded as a refusal instead: where an engine cannot say what a sentence
/// means, §3.1 has already settled the answer, which is to say so rather than to
/// write something close.
pub struct Dialect {
    /// What an identifier is wrapped in, and **the most dangerous entry here by
    /// a wide margin.** DuckDB reads `"region"` as a column; Spark reads it as
    /// the text `'region'`. So the wrong one does not fail. It returns the
    /// column's own name once per row, for every row, and the query looks
    /// perfect while every value in it is wrong. Measured rather than assumed,
    /// on a real session, by reading the rows back.
    quote: char,
    /// What follows `SELECT *` to leave a column out.
    exclude: &'static str,
    /// The function that stops a query and says why.
    raise: &'static str,
    /// Whether a backslash inside a text literal escapes the next character.
    ///
    /// **The second silent-wrongness entry in this table**, and it was found the
    /// same way as the first: by running the corpus and reading the rows.
    /// DuckDB reads `'a\\b'` as three characters. Spark reads it as `a` and a
    /// backspace, and reads `'\\'` as an unterminated string, which is what
    /// `starts` emits for its `ESCAPE` clause. So a backslash is doubled going
    /// in, or the same sentence means two things on two engines.
    escapes_backslash: bool,
    /// How this engine is asked which day of the week a date is, **numbered so
    /// that Monday is 1**.
    ///
    /// The plain question is the trap: DuckDB's `dayofweek` and Spark's
    /// `weekday` both exist, both answer, and both answer differently — 5 and 4
    /// for the same Friday, with nothing raised. So the grammar names the
    /// numbering it means rather than passing the word through, and each engine
    /// gets the spelling that produces it.
    weekday: &'static str,
    /// Whether the engine can pivot on a set of values it works out for itself.
    /// Where it cannot, a `widen` that declares nothing is a sentence this
    /// dialect cannot write, and `refuses` says so.
    dynamic_pivot: bool,
    /// How this engine converts a number to a whole one, **truncating toward
    /// zero**, which is the convention the grammar names.
    ///
    /// **Spark's plain `CAST` already truncates and DuckDB's rounds**, so this
    /// is one dialect's correction rather than a shared spelling — and reaching
    /// for the obvious shared one broke Spark outright, because `trunc` there is
    /// a *date* function that wants two arguments. Found by the two-engine
    /// check, which is what it is for.
    to_whole: &'static str,
    /// How this engine is told to skip missing values in `last_value`, which is
    /// what `latest` is built from.
    ///
    /// **The sixth entry, and the third that had to be measured on a live
    /// session.** DuckDB takes the standard `IGNORE NULLS` after the argument;
    /// Spark rejects that syntax outright and takes a second boolean argument
    /// instead. Neither is a superset of the other, so this is a spelling in
    /// the table rather than one form with a workaround.
    ///
    /// `{}` is the value being looked at.
    last_present: &'static str,
}

/// DuckDB, which is what `--as sql` has always meant.
const DUCKDB: Dialect = Dialect {
    quote: '"',
    exclude: "EXCLUDE",
    raise: "error",
    escapes_backslash: false,
    weekday: "isodow({})",
    dynamic_pivot: true,
    to_whole: "CAST(trunc({}) AS BIGINT)",
    last_present: "last_value({} IGNORE NULLS)",
};

/// Spark, measured against a real 4.2 session on 2026-08-07 rather than read
/// out of a manual. Most of these entries were found by running the
/// constructs this file emits and reading what came back.
const SPARK: Dialect = Dialect {
    quote: '`',
    exclude: "EXCEPT",
    raise: "raise_error",
    escapes_backslash: true,
    weekday: "extract(DAYOFWEEK_ISO FROM {})",
    dynamic_pivot: false,
    to_whole: "CAST({} AS BIGINT)",
    last_present: "last_value({}, true)",
};

pub struct Sql;

impl Backend for Sql {
    fn name(&self) -> &'static str {
        "sql"
    }

    fn render(&self, plan: &Plan, entering: &[Schema]) -> String {
        DUCKDB.render(plan, entering)
    }
}

pub struct SparkSql;

impl Backend for SparkSql {
    fn name(&self) -> &'static str {
        "spark"
    }

    /// **The one sentence Spark cannot say.** Its `PIVOT` needs the values
    /// listed in the query, and a `widen` that declares nothing takes them from
    /// the data. This is exactly the case `giving` was built for, so the fix is
    /// a clause the grammar already has rather than anything new.
    fn refuses(&self, plan: &Plan) -> Option<Diagnostic> {
        for step in &plan.steps {
            if let Step::Widen { giving, span, .. } = step {
                if giving.is_empty() {
                    return Some(Diagnostic::illegal(
                        "Spark has to be told which columns a `widen` makes, and this one takes them from the data. Say what it makes: `giving [q1, q2, q3]`",
                        *span,
                    ));
                }
            }
        }
        None
    }

    fn render(&self, plan: &Plan, entering: &[Schema]) -> String {
        SPARK.render(plan, entering)
    }
}

impl Dialect {
    fn render(&self, plan: &Plan, entering: &[Schema]) -> String {
        let mut parts = Vec::new();
        parts.push(format!(
            "step0 AS (SELECT * FROM {})",
            self.table(&plan.source)
        ));

        for (i, step) in plan.steps.iter().enumerate() {
            let from = format!("step{i}");
            let body = match step {
                Step::Keep { condition, .. } => {
                    format!(
                        "SELECT * FROM {from} WHERE {}",
                        self.condition_sql(condition, &from)
                    )
                }
                Step::Pick { names, all_but, .. } => {
                    if *all_but {
                        let dropped: Vec<String> =
                            names.iter().map(|n| self.name(&n.text)).collect();
                        format!(
                            "SELECT * {} ({}) FROM {from}",
                            self.exclude,
                            dropped.join(", ")
                        )
                    } else {
                        let kept: Vec<String> = names.iter().map(|n| self.name(&n.text)).collect();
                        format!("SELECT {} FROM {from}", kept.join(", "))
                    }
                }
                Step::Add { values, by, .. } => {
                    let added: Vec<String> = values
                        .iter()
                        .map(|v| {
                            // An aggregate written in `add` spans the group and
                            // hands the same answer back to every row in it,
                            // which is a window rather than a collapse.
                            let over = Over {
                                partition: by,
                                order: last_sort(plan, i),
                                windowed: true,
                            };
                            // A window writes its own `OVER`, because it needs an
                            // `ORDER BY` inside it that the group alone cannot
                            // supply. An aggregate does not, and gets the group
                            // wrapped around it here.
                            // One path for both kinds. A window writes its
                            // own `OVER` because it needs an `ORDER BY`
                            // inside it; an aggregate has its `OVER`
                            // attached where the aggregate is, which is not
                            // always the outside of the value.
                            let value = self.expr_over(&v.value, over);
                            format!("{value} AS {}", self.name(&v.name.text))
                        })
                        .collect();
                    // `add` covers making a column and remaking one, because to
                    // whoever writes it those are the same act. SQL does not
                    // agree: a name that is already there has to be excluded
                    // first or it arrives twice, and a name that is not there
                    // cannot be excluded at all. So the two cases are told apart
                    // here, from the columns this step was handed.
                    let held = entering.get(i).map(|s| s.names()).unwrap_or_default();
                    let replaced: Vec<String> = values
                        .iter()
                        .filter(|v| held.contains(&v.name.text))
                        .map(|v| self.name(&v.name.text))
                        .collect();
                    let keep = if replaced.is_empty() {
                        "*".to_string()
                    } else {
                        format!("* {} ({})", self.exclude, replaced.join(", "))
                    };
                    // **A window makes the row order the engine's choice, so
                    // the sort has to be said again.** Computing one groups
                    // the rows to do it, and nothing puts them back: the
                    // same sentence returned the two regions in opposite
                    // orders on two engines, with every value identical.
                    //
                    // This is the fourth time this exact shape has been
                    // found, after `summarize`, `drop_duplicates` and
                    // `take ... by`, and all four times by running two
                    // things rather than by reading a query. The rule
                    // underneath is worth stating plainly: **wherever a step
                    // reorders rows to do its work, the order somebody asked
                    // for has to be restated afterwards.**
                    let ordered = if values.iter().any(|v| v.value.windows()) {
                        last_sort(plan, i)
                            .map(|keys| {
                                let written: Vec<String> = keys
                                    .iter()
                                    .map(|k| {
                                        format!(
                                            "{}{}",
                                            self.name(&k.column.text),
                                            if k.descending { " DESC" } else { "" }
                                        )
                                    })
                                    .collect();
                                format!(" ORDER BY {}", written.join(", "))
                            })
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    format!("SELECT {keep}, {} FROM {from}{ordered}", added.join(", "))
                }
                Step::Summarize { values, by, .. } => {
                    let mut selected: Vec<String> = by.iter().map(|n| self.name(&n.text)).collect();
                    selected.extend(values.iter().map(|v| {
                        format!("{} AS {}", self.expr(&v.value), self.name(&v.name.text))
                    }));
                    let mut out = format!("SELECT {} FROM {from}", selected.join(", "));
                    if !by.is_empty() {
                        let groups: Vec<String> = by.iter().map(|n| self.name(&n.text)).collect();
                        out.push_str(&format!(" GROUP BY {}", groups.join(", ")));
                        // **The groups come back in order, and they have to.**
                        // `GROUP BY` promises nothing about row order, so a hash
                        // aggregation hands them over in whatever order its table
                        // yields — which differs between two runs of the same
                        // pipeline, never mind between two hosts. That was found
                        // by running one sentence in R and in Python and getting
                        // the same rows in different orders.
                        //
                        // A pipeline whose answer reorders itself between runs is
                        // not predictable, and predictable is the whole promise.
                        // So the irregularity is made explicit rather than left
                        // to the engine: groups are ordered by the columns that
                        // define them. dplyr and pandas both do this, so it is
                        // also what anyone arriving from either already expects.
                        out.push_str(&format!(" ORDER BY {}", groups.join(", ")));
                    }
                    out
                }
                Step::Sort { keys, .. } => {
                    let ordered: Vec<String> = keys
                        .iter()
                        .map(|k| {
                            format!(
                                "{}{}",
                                self.name(&k.column.text),
                                if k.descending { " DESC" } else { "" }
                            )
                        })
                        .collect();
                    format!("SELECT * FROM {from} ORDER BY {}", ordered.join(", "))
                }
                Step::AddRows { other, .. } => {
                    // `UNION ALL` rather than `UNION`, because adding rows adds
                    // rows. Dropping the repeats would be `drop_duplicates`, and
                    // a verb that quietly did two things is the trap `stack` was
                    // renamed to escape.
                    //
                    // **The columns are written out on both sides rather than
                    // matched by name.** DuckDB has `UNION ALL BY NAME` for
                    // that and Spark has nothing, so naming them is what makes
                    // one query serve both. It is also the more honest query:
                    // the checker has already refused two tables that do not
                    // hold the same columns, so the list exists and saying it
                    // costs nothing. What it buys is that the two sides line up
                    // by name even where the other table holds them in another
                    // order, which is exactly what `BY NAME` was doing.
                    let listed: Vec<String> = entering[i]
                        .columns
                        .iter()
                        .map(|(c, _)| self.name(c))
                        .collect();
                    let columns = listed.join(", ");
                    format!(
                        "SELECT {columns} FROM {from} UNION ALL SELECT {columns} FROM {}",
                        self.table(&other.text)
                    )
                }

                Step::AddCombinations { names, by, .. } => {
                    // **The original rows are not touched, and that is the
                    // shape of the query rather than a promise about it.** The
                    // first half is the table, unchanged and unjoined; the
                    // second half is the combinations that are not in it. So
                    // this is `add_rows` against a grid the query builds, which
                    // is what the verb's name says, and no row can be lost or
                    // reordered by a join that never runs on it.
                    //
                    // **A grid built from non-missing values is what lets the
                    // whole query use plain `=`.** A null-safe comparison is
                    // spelled two ways on the two engines and would be a sixth
                    // dialect entry. It is not needed: a missing value makes no
                    // combination, so nothing in the grid is null, and a row
                    // whose key is missing simply matches nothing and keeps its
                    // place in the first half.
                    let held: Vec<String> =
                        by.iter().map(|n| self.name(&n.text)).collect();

                    // Each crossed column's own distinct values, carrying the
                    // held columns along so the cross can be done inside each
                    // group. With no `by` the join condition is empty and this
                    // is an unrestricted cross, which is the same query with one
                    // group.
                    let parts: Vec<String> = names
                        .iter()
                        .enumerate()
                        .map(|(k, n)| {
                            let mut selected = held.clone();
                            selected.push(self.name(&n.text));
                            format!(
                                "(SELECT DISTINCT {} FROM {from} WHERE {} IS NOT NULL) {GRID}{k}",
                                selected.join(", "),
                                self.name(&n.text)
                            )
                        })
                        .collect();

                    let mut grid = parts[0].clone();
                    for (k, part) in parts.iter().enumerate().skip(1) {
                        let on: Vec<String> = by
                            .iter()
                            .map(|n| {
                                format!(
                                    "{GRID}0.{0} = {GRID}{k}.{0}",
                                    self.name(&n.text)
                                )
                            })
                            .collect();
                        grid.push_str(&if on.is_empty() {
                            format!(" CROSS JOIN {part}")
                        } else {
                            format!(" JOIN {part} ON {}", on.join(" AND "))
                        });
                    }

                    // Every column, in the order the table holds them: the ones
                    // the grid knows come from the grid, and the rest are
                    // missing, which is the ruling this verb was built on.
                    let listed: Vec<String> = entering[i]
                        .columns
                        .iter()
                        .map(|(c, _)| self.name(c))
                        .collect();
                    let taken: Vec<String> = entering[i]
                        .columns
                        .iter()
                        .map(|(c, _)| {
                            let quoted = self.name(c);
                            if let Some(k) = names.iter().position(|n| &n.text == c) {
                                format!("{GRID}{k}.{quoted}")
                            } else if by.iter().any(|n| &n.text == c) {
                                format!("{GRID}0.{quoted}")
                            } else {
                                format!("NULL AS {quoted}")
                            }
                        })
                        .collect();

                    let absent: Vec<String> = names
                        .iter()
                        .enumerate()
                        .map(|(k, n)| {
                            format!(
                                "{ROWS}.{0} = {GRID}{k}.{0}",
                                self.name(&n.text)
                            )
                        })
                        .chain(by.iter().map(|n| {
                            format!("{ROWS}.{0} = {GRID}0.{0}", self.name(&n.text))
                        }))
                        .collect();

                    format!(
                        "SELECT {} FROM {from} UNION ALL SELECT {} FROM {grid} WHERE NOT EXISTS (SELECT 1 FROM {from} {ROWS} WHERE {})",
                        listed.join(", "),
                        taken.join(", "),
                        absent.join(" AND ")
                    )
                }

                Step::DropDuplicates { .. } => {
                    // **The same irregularity `summarize` has, and the same
                    // answer.** `SELECT DISTINCT` promises nothing about the
                    // order it hands rows back in, so a hash implementation
                    // returns them in whatever order its table holds, and that
                    // can differ between engines and between two runs of one.
                    //
                    // Its groups are the distinct rows, defined by every column,
                    // so ordering by every column is the same rule summarize
                    // follows rather than a second one.
                    let all: Vec<String> = entering[i]
                        .columns
                        .iter()
                        .map(|(c, _)| self.name(c))
                        .collect();
                    format!("SELECT DISTINCT * FROM {from} ORDER BY {}", all.join(", "))
                }

                Step::Rename { values, .. } => {
                    // The columns are written out rather than renamed in place,
                    // so the order is the order they were in. `SELECT * EXCLUDE`
                    // plus the new name would move every renamed column to the
                    // end, which is a change nobody asked for.
                    let renamed: Vec<String> = entering[i]
                        .columns
                        .iter()
                        .map(|(column, _)| {
                            match values.iter().find(|v| match &v.value {
                                Expr::Column(from) => &from.text == column,
                                _ => false,
                            }) {
                                Some(v) => {
                                    format!("{} AS {}", self.name(column), self.name(&v.name.text))
                                }
                                None => self.name(column),
                            }
                        })
                        .collect();
                    format!("SELECT {} FROM {from}", renamed.join(", "))
                }

                Step::DropMissing { names, .. } => {
                    let wanted: Vec<String> = if names.is_empty() {
                        entering[i].columns.iter().map(|(c, _)| c.clone()).collect()
                    } else {
                        names.iter().map(|n| n.text.clone()).collect()
                    };
                    let tests: Vec<String> = wanted
                        .iter()
                        .map(|c| format!("{} IS NOT NULL", self.name(c)))
                        .collect();
                    format!("SELECT * FROM {from} WHERE {}", tests.join(" AND "))
                }

                Step::FillMissing { values, .. } => {
                    let filled: Vec<String> = entering[i]
                        .columns
                        .iter()
                        .map(
                            |(column, _)| match values.iter().find(|v| &v.name.text == column) {
                                Some(v) => format!(
                                    "COALESCE({}, {}) AS {}",
                                    self.name(column),
                                    self.expr(&v.value),
                                    self.name(column)
                                ),
                                None => self.name(column),
                            },
                        )
                        .collect();
                    format!("SELECT {} FROM {from}", filled.join(", "))
                }

                // **Every literal here was worked out by the checker**, which is
                // why this is a plain `UNION ALL` over ordinary columns and not
                // a dialect's `UNPIVOT`. No pattern is matched, no string is
                // split, and nothing in it is specific to an engine.
                Step::Lengthen { resolved, .. } => {
                    let it = resolved
                        .as_ref()
                        .expect("the checker resolves every lengthen");
                    let branches: Vec<String> = it
                        .rows
                        .iter()
                        .map(|row| {
                            let mut selected: Vec<String> =
                                it.keep.iter().map(|c| self.name(c)).collect();
                            selected.extend(row.labels.iter().zip(&it.name_columns).map(
                                |(label, into)| {
                                    format!("{} AS {}", self.text(label), self.name(into))
                                },
                            ));
                            selected.extend(row.sources.iter().zip(&it.value_columns).map(
                                |(src, into)| format!("{} AS {}", self.name(src), self.name(into)),
                            ));
                            format!("SELECT {} FROM {from}", selected.join(", "))
                        })
                        .collect();
                    // **Ordered by every column of the result, left to right**,
                    // which is not a new rule but the one `drop_duplicates`
                    // already follows. A union promises nothing about row order,
                    // and the columns that stayed come first, so each original
                    // row's new rows land together — which is what tidyr spends
                    // `cols_vary` on.
                    let ordered: Vec<String> = it
                        .keep
                        .iter()
                        .chain(it.name_columns.iter())
                        .chain(it.value_columns.iter())
                        .map(|c| self.name(c))
                        .collect();
                    format!(
                        "{} ORDER BY {}",
                        branches.join(" UNION ALL "),
                        ordered.join(", ")
                    )
                }

                Step::Widen {
                    name: pattern,
                    value,
                    by,
                    missing,
                    giving,
                    ..
                } => {
                    let groups: Vec<String> = by.iter().map(|n| self.name(&n.text)).collect();
                    let labelled: Vec<String> =
                        pattern.named_parts().iter().map(|c| self.name(c)).collect();

                    // Which rows belong in a declared column, one test per piece
                    // of its name. It is the pattern read backwards, by the same
                    // code `lengthen` reads a column apart with.
                    let belongs = |made: &str| -> String {
                        pattern
                            .read(made)
                            .expect("the checker read every declared column")
                            .iter()
                            .zip(pattern.named_parts())
                            .map(|(piece, column)| {
                                format!("{} = {}", self.name(column), self.text(piece))
                            })
                            .collect::<Vec<_>>()
                            .join(" AND ")
                    };

                    // **Both refusals are worked out once, before anything is
                    // pivoted, and they have to be.** A dialect's `PIVOT` takes
                    // exactly one aggregate, so a `CASE` that counts cannot sit
                    // inside it; and writing them per column would repeat a
                    // whole sentence of English in the query for every column
                    // made. Doing it here also makes the two shapes below behave
                    // identically, which matters more than either: one verb may
                    // not refuse in one spelling and shrug in the other.
                    let cell: Vec<String> = groups.iter().chain(labelled.iter()).cloned().collect();
                    let mut guards = Vec::new();
                    if !giving.is_empty() {
                        let known: Vec<String> = giving
                            .iter()
                            .map(|m| format!("({})", belongs(&m.text)))
                            .collect();
                        guards.push(format!(
                                "WHEN NOT ({}) THEN {}({})",
                                known.join(" OR "),
                                self.raise,
                                self.text("this holds a value that `giving` does not list, so widening would drop those rows without saying so. Add it to `giving`, or keep only the rows you meant first")
                            ));
                    }
                    if !value.aggregates() {
                        guards.push(format!(
                                "WHEN count(*) OVER (PARTITION BY {}) > 1 THEN {}({})",
                                cell.join(", "),
                                self.raise,
                                // No em dash: this text is printed in the manual, so
                                // it is the book's prose as much as the engine's.
                                self.text("two rows want the same cell, and nothing here says which of them wins. Say what to do about that with `value average(...)` or `value first(...)`, or summarize before widening")
                            ));
                    }

                    // An aggregate in `value` is what answers the second of
                    // those, so the guarded column holds what it is aggregating
                    // and the aggregate itself is written around it below.
                    let inner = match aggregate_argument(value) {
                        Some(arg) => arg,
                        None => value.clone(),
                    };
                    let held = if guards.is_empty() {
                        self.expr(&inner)
                    } else {
                        format!("CASE {} ELSE {} END", guards.join(" "), self.expr(&inner))
                    };
                    let counted = format!("(SELECT *, {held} AS {} FROM {from})", self.name(CELL));

                    let guarded = Expr::Column(Name {
                        text: CELL.into(),
                        span: Span::new(0, 0),
                    });
                    let aggregate = match value {
                        Expr::Call {
                            name: fname, args, ..
                        } if value.aggregates() => {
                            if args.is_empty() {
                                // `row_count()` asks about rows rather than a
                                // column, and the guarded cell stands in for the
                                // row so that the guard is still read. `count(*)`
                                // would name nothing and could be optimized past.
                                format!("count({})", self.expr(&guarded))
                            } else {
                                self.call(fname, std::slice::from_ref(&guarded))
                            }
                        }
                        // No aggregate was written, so the one value in the cell
                        // is the answer, and the guard above has already refused
                        // the case where there is more than one.
                        _ => format!("max({})", self.expr(&guarded)),
                    };

                    if giving.is_empty() {
                        // Nothing was declared, so the columns come from the
                        // data and only the engine can name them. The checker
                        // has already refused any step after this one.
                        let on = if labelled.len() == 1 {
                            labelled[0].clone()
                        } else {
                            let mut parts = Vec::new();
                            for (i, column) in labelled.iter().enumerate() {
                                if !pattern.literals[i].is_empty() {
                                    parts.push(self.text(&pattern.literals[i]));
                                }
                                parts.push(column.clone());
                            }
                            if let Some(tail) = pattern.literals.last() {
                                if !tail.is_empty() {
                                    parts.push(self.text(tail));
                                }
                            }
                            parts.join(" || ")
                        };
                        // Only a dialect that can work the value list out for
                        // itself gets here. `refuses` has already turned this
                        // sentence away for one that cannot, which is why this
                        // arm may assume rather than check.
                        debug_assert!(self.dynamic_pivot, "a dialect without a dynamic pivot refuses this sentence before it is rendered");
                        format!(
                                "SELECT * FROM (PIVOT {counted} ON {on} USING {aggregate} GROUP BY {}) ORDER BY {}",
                                groups.join(", "),
                                groups.join(", ")
                            )
                    } else {
                        let cells: Vec<String> = giving
                            .iter()
                            .map(|made| {
                                let one =
                                    format!("{aggregate} FILTER (WHERE {})", belongs(&made.text));
                                let filled = match missing {
                                    Some(f) => format!("COALESCE({one}, {})", self.expr(f)),
                                    None => one,
                                };
                                format!("{filled} AS {}", self.name(&made.text))
                            })
                            .collect();
                        format!(
                            "SELECT {}, {} FROM {counted} GROUP BY {} ORDER BY {}",
                            groups.join(", "),
                            cells.join(", "),
                            groups.join(", "),
                            groups.join(", ")
                        )
                    }
                }

                Step::Join {
                    other,
                    by,
                    unmatched,
                    ..
                } => {
                    let right = self.table(&other.text);
                    let on: Vec<String> = by
                        .iter()
                        .map(|k| {
                            format!(
                                "{from}.{} = {right}.{}",
                                self.name(&k.this.text),
                                self.name(&k.other.text)
                            )
                        })
                        .collect();
                    // **What is dropped is the other table's name for the key**,
                    // which is the same column it always was: for a same-named
                    // key the two halves are one word, and for a pair the value
                    // is already here under this table's name.
                    let dropped: Vec<String> =
                        by.iter().map(|k| self.name(&k.other.text)).collect();
                    let kind = match unmatched {
                        Unmatched::This => "LEFT JOIN",
                        Unmatched::None => "JOIN",
                        Unmatched::Both => "FULL JOIN",
                    };

                    // **A full join has to coalesce the key, and this is where
                    // the obvious query is silently wrong.** Taking the key from
                    // this table's side works for every row that this table has.
                    // A row that exists only in the other table has no left side
                    // at all, so the key comes back empty while the value is
                    // sitting in the other table untouched. The column that says
                    // which rows correspond is the one column that must never be
                    // empty in the answer.
                    //
                    // The other two kinds cannot hit it: `LEFT JOIN` keeps every
                    // row of this table and `JOIN` keeps only matches, so in
                    // both the left key is always there.
                    let left_side = if *unmatched == Unmatched::Both {
                        entering[i]
                            .columns
                            .iter()
                            .map(|(column, _)| {
                                let quoted = self.name(column);
                                // The pair is coalesced across its two names,
                                // and keeps this table's. A row that exists only
                                // in `customers` still answers with its `id`,
                                // under the name `customer_id` the rest of the
                                // sentence is written in.
                                match by.iter().find(|k| &k.this.text == column) {
                                    Some(key) => format!(
                                        "COALESCE({from}.{quoted}, {right}.{}) AS {quoted}",
                                        self.name(&key.other.text)
                                    ),
                                    None => format!("{from}.{quoted}"),
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    } else {
                        format!("{from}.*")
                    };

                    format!(
                        "SELECT {left_side}, {right}.* {} ({}) FROM {from} {kind} {right} ON {}",
                        self.exclude,
                        dropped.join(", "),
                        on.join(" AND ")
                    )
                }
                Step::Take { count, by, last, ties, .. } => {
                    // The order the caller asked for, and the same order walked
                    // backwards. `take_last` is `take` over the second one, with
                    // the first restored afterwards so the answer still reads
                    // the way the `sort` said it should.
                    let written = |keys: &[SortKey], flip: bool| {
                        keys.iter()
                            .map(|k| {
                                let down = if flip { !k.descending } else { k.descending };
                                format!(
                                    "{}{}",
                                    self.name(&k.column.text),
                                    if down { " DESC" } else { "" }
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    };
                    if *ties {
                        // **`with ties` is one query shape for both the grouped
                        // and ungrouped cases**, because `LIMIT` cannot express
                        // it at all: the count is not known until the rows are
                        // ranked. `RANK` is the whole of it — it gives tied rows
                        // the same place and skips the next, so `rank <= n`
                        // keeps every row level with the last one taken.
                        // `ROW_NUMBER` is what the untied case uses and is
                        // exactly the wrong function here, since it breaks ties
                        // arbitrarily and silently.
                        let sorted = last_sort(plan, i)
                            .expect("ties are only reached after a sort");
                        let keys = written(sorted, *last);
                        let restore = written(sorted, false);
                        let partition = if by.is_empty() {
                            String::new()
                        } else {
                            let groups: Vec<String> =
                                by.iter().map(|n| self.name(&n.text)).collect();
                            format!("PARTITION BY {} ", groups.join(", "))
                        };
                        format!(
                            "SELECT * {} ({rank}) FROM (SELECT *, RANK() OVER ({partition}ORDER BY {keys}) AS {rank} FROM {from}) WHERE {rank} <= {count} ORDER BY {restore}",
                            self.exclude,
                            rank = self.name(RANK)
                        )
                    } else if by.is_empty() {
                        if *last {
                            let keys = last_sort(plan, i)
                                .expect("take_last is only reached after a sort");
                            format!(
                                "SELECT * FROM (SELECT * FROM {from} ORDER BY {} LIMIT {count}) ORDER BY {}",
                                written(keys, true),
                                written(keys, false)
                            )
                        } else {
                        format!("SELECT * FROM {from} LIMIT {count}")
                        }
                    } else {
                        // **The window carries the sort's own keys**, rather than
                        // trusting the engine to number rows in the order they
                        // arrived. DuckDB does happen to, and SQL promises
                        // nothing about it, which is the difference between a
                        // query that works and a query that works here. The
                        // checker has already refused this without a sort before
                        // it, so there is always something to find.
                        let sorted = last_sort(plan, i)
                            .expect("a grouped take is only reached after a sort");
                        // Numbering from the far end is the same window counting
                        // the other way, which is why this needs no second query
                        // shape. The `ORDER BY` at the end is always the order
                        // the caller asked for.
                        let keys = written(sorted, *last);
                        let restore = written(sorted, false);
                        let groups: Vec<String> = by.iter().map(|n| self.name(&n.text)).collect();
                        // **The order the sort established has to survive the
                        // window.** Filtering on the row number is a `WHERE`,
                        // and a `WHERE` promises nothing about what order the
                        // rows come out in, so without repeating the keys the
                        // groups come back in whatever order the engine chose.
                        // The parity harness caught exactly that: R and Python
                        // ran this sentence and returned the same two rows the
                        // other way round.
                        format!(
                                "SELECT * {} ({rank}) FROM (SELECT *, ROW_NUMBER() OVER (PARTITION BY {} ORDER BY {keys}) AS {rank} FROM {from}) WHERE {rank} <= {count} ORDER BY {restore}",
                                self.exclude,
                                groups.join(", "),
                                rank = self.name(RANK)
                            )
                    }
                }
            };
            parts.push(format!("step{} AS ({body})", i + 1));
        }

        format!(
            "WITH {}\nSELECT * FROM step{}",
            parts.join(",\n     "),
            plan.steps.len()
        )
    }

    /// An identifier, quoted so that a column called `select` or `순서` is a column.
    /// A text value, quoted so that an apostrophe is an apostrophe and a
    /// backslash is a backslash.
    fn text(&self, value: &str) -> String {
        let mut out = value.to_string();
        if self.escapes_backslash {
            out = out.replace('\\', "\\\\");
        }
        format!("'{}'", out.replace('\'', "''"))
    }

    fn name(&self, text: &str) -> String {
        // Doubling the quote is how both dialects escape one inside an
        // identifier, so the rule is shared and only the character moves.
        let q = self.quote;
        format!("{q}{}{q}", text.replace(q, &format!("{q}{q}")))
    }

    /// A table's name, whose parts are quoted one at a time.
    ///
    /// **Quoting the whole of `main.sales.orders` names one table whose name
    /// contains dots**, which is not a table any catalog has. The query parses,
    /// and it fails looking for something that was never there, so this is a
    /// mistake that reads correctly and cannot work. Each part is an identifier
    /// of its own and is quoted as one.
    ///
    /// A column is *not* run through this and must not be: what is inside `[ ]`
    /// is a name exactly as written, so a column really can be called `a.b`, and
    /// splitting one would break it.
    fn table(&self, text: &str) -> String {
        text.split('.')
            .map(|part| self.name(part))
            .collect::<Vec<_>>()
            .join(".")
    }

    fn expr(&self, e: &Expr) -> String {
        self.expr_over(e, Over::default())
    }

    fn expr_over(&self, e: &Expr, over: Over) -> String {
        match e {
            Expr::Column(n) => self.name(&n.text),
            Expr::Text { value, .. } => self.text(value),
            Expr::Whole { value, .. } => value.to_string(),
            Expr::Decimal { value, .. } => format_decimal(*value),
            Expr::Truth { value, .. } => if *value { "TRUE" } else { "FALSE" }.to_string(),
            Expr::Missing { .. } => "NULL".to_string(),

            Expr::Arithmetic {
                op, left, right, ..
            } => {
                format!(
                    "({} {} {})",
                    self.expr_over(left, over),
                    op,
                    self.expr_over(right, over)
                )
            }
            Expr::Compare {
                op, left, right, ..
            } => {
                let symbol = match op {
                    Compare::Is => "=",
                    Compare::IsNot => "<>",
                    Compare::Less => "<",
                    Compare::LessOrEqual => "<=",
                    Compare::Greater => ">",
                    Compare::GreaterOrEqual => ">=",
                };
                format!(
                    "({} {symbol} {})",
                    self.expr_over(left, over),
                    self.expr_over(right, over)
                )
            }
            Expr::Logic {
                op, left, right, ..
            } => {
                let word = match op {
                    Logic::And => "AND",
                    Logic::Or => "OR",
                };
                format!(
                    "({} {word} {})",
                    self.expr_over(left, over),
                    self.expr_over(right, over)
                )
            }
            Expr::Not { inner, .. } => format!("(NOT {})", self.expr_over(inner, over)),
            Expr::In {
                left, set, negated, ..
            } => {
                let values: Vec<String> = set.iter().map(|v| self.expr_over(v, over)).collect();
                format!(
                    "({} {}IN ({}))",
                    self.expr_over(left, over),
                    if *negated { "NOT " } else { "" },
                    values.join(", ")
                )
            }
            Expr::IsMissing { inner, negated, .. } => {
                format!(
                    "({} IS {}NULL)",
                    self.expr_over(inner, over),
                    if *negated { "NOT " } else { "" }
                )
            }
            // `LIKE` rather than a dialect's `starts_with`, because it is the one
            // spelling every engine has. The value is escaped, so a `%` someone
            // typed is a percent sign and not "anything at all".
            Expr::TextTest {
                op, left, value, ..
            } => {
                let pattern = match value.as_ref() {
                    Expr::Text { value, .. } => {
                        let escaped = value
                            .replace('\\', "\\\\")
                            .replace('%', "\\%")
                            .replace('_', "\\_");
                        match op {
                            TextOp::Starts => self.text(&format!("{escaped}%")),
                            TextOp::Ends => self.text(&format!("%{escaped}")),
                            TextOp::Contains => self.text(&format!("%{escaped}%")),
                        }
                    }
                    // Not reachable from a checked plan, which requires a written
                    // value, and left correct rather than clever if it ever is.
                    other => self.expr_over(other, over),
                };
                // The escape character goes through the same writer every other
                // text value does, because it *is* a text value: on an engine
                // where a backslash escapes, `'\'` is an unterminated string
                // rather than a backslash.
                format!(
                    "({} LIKE {pattern} ESCAPE {})",
                    self.expr_over(left, over),
                    self.text("\\")
                )
            }
            // Resolved away by the checker, which turns `pick where` into a list.
            // Resolved away by the checker, which expands `add where` into ordinary
            // values, one per matched column.
            Expr::ColumnValue { .. } => "NULL".to_string(),
            Expr::ColumnKind { .. } => "NULL".to_string(),
            // A conditional is `CASE WHEN` and always was; the grammar's word
            // for it is just the plain one. `ELSE` is left off where nothing was
            // said, which is exactly what makes an unmatched row missing.
            Expr::When {
                arms, otherwise, ..
            } => {
                let mut out = String::from("CASE");
                for (test, value) in arms {
                    out.push_str(&format!(
                        " WHEN {} THEN {}",
                        self.expr_over(test, over),
                        self.expr_over(value, over)
                    ));
                }
                if let Some(fallback) = otherwise {
                    out.push_str(&format!(" ELSE {}", self.expr_over(fallback, over)));
                }
                out.push_str(" END");
                out
            }
            Expr::ColumnName { .. } => "NULL".to_string(),
            // Expanded by the checker before any backend runs; written out
            // rather than panicking so a drawing of an unchecked sentence has
            // something to show.
            Expr::Quantified { every, .. } => format!(
                "/* {} of the matched columns */",
                if *every { "every" } else { "any" }
            ),
            // **A windowed call writes its own `OVER` too**, and it is here
            // rather than in `call` because `call` renders an expression
            // without knowing where it stands. All three take the order the
            // rows are already in, which the checker has refused to leave
            // unsaid.
            Expr::Call { name, args, .. }
                if matches!(
                    name.as_str(),
                    "running_total" | "previous" | "following" | "latest"
                ) =>
            {
                let mut clauses = Vec::new();
                if !over.partition.is_empty() {
                    let groups: Vec<String> =
                        over.partition.iter().map(|n| self.name(&n.text)).collect();
                    clauses.push(format!("PARTITION BY {}", groups.join(", ")));
                }
                let ordering: Vec<String> = over
                    .order
                    .unwrap_or_default()
                    .iter()
                    .map(|k| {
                        format!(
                            "{}{}",
                            self.name(&k.column.text),
                            if k.descending { " DESC" } else { "" }
                        )
                    })
                    .collect();
                if !ordering.is_empty() {
                    clauses.push(format!("ORDER BY {}", ordering.join(", ")));
                }
                let inner = self.expr_over(&args[0], Over::default());
                match name.as_str() {
                    // **The frame is written out.** A `SUM` with an `ORDER BY`
                    // and no frame defaults to `RANGE`, which ties rows sharing
                    // a sort key together and gives them all the same total.
                    // That is a running total that stalls on a tie, and it is
                    // the sort of thing that looks right on a fixture where the
                    // keys happen to be distinct.
                    "running_total" => format!(
                        "sum({inner}) OVER ({} ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)",
                        clauses.join(" ")
                    ),
                    // **How far is written out only when it was asked for.**
                    // `lag(x)` and `lag(x, 1)` mean the same thing to both
                    // dialects, and the shorter one is what a reader of the
                    // sentence wrote.
                    // **The frame is written out here too, and for a sharper
                    // reason than `running_total`'s.** `last_value` with an
                    // `ORDER BY` and no frame defaults to `RANGE ... CURRENT
                    // ROW`, which on a tie hands every tied row the last value
                    // of the whole tie rather than its own — and with the
                    // default frame some engines look at the entire partition,
                    // so a hole would be filled from *below*. Naming the frame
                    // is what makes this fill downward and only downward.
                    "latest" => format!(
                        "{} OVER ({} ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)",
                        self.last_present.replace("{}", &inner),
                        clauses.join(" ")
                    ),
                    "previous" => {
                        format!("lag({inner}{}) OVER ({})", super::step(args), clauses.join(" "))
                    }
                    _ => format!("lead({inner}{}) OVER ({})", super::step(args), clauses.join(" ")),
                }
            }

            // **An aggregate under a grouping takes its own `OVER`, and it has
            // to be here rather than around the whole value.** `add [share] as
            // [revenue] / total([revenue]) by [product]` means the row's revenue
            // over the group's total, so the window belongs to `sum` alone.
            // Wrapping the finished expression instead produced
            // `("revenue" / sum("revenue")) OVER (...)`, which no engine parses,
            // and the checker had already accepted the sentence. Found by
            // writing the example in the book, which is what the book is for.
            //
            // `Over::default()` carries no partition, so a `summarize` reaches
            // this arm and gets a plain call, which is what collapsing a group
            // means.
            Expr::Call {
                name: fname, args, ..
            } => {
                let written = self.call(fname, args);
                if !vocabulary::is_aggregate(fname) || !over.windowed {
                    return written;
                }
                // With no `by` the window is the whole table, and `OVER ()` has
                // to be said: `add [share] as [x] / total([x])` rendered a bare
                // aggregate and the engine demanded a `GROUP BY` nobody wrote.
                // Found by a cookbook recipe, which is what the book is for.
                if over.partition.is_empty() {
                    format!("{written} OVER ()")
                } else {
                    let groups: Vec<String> =
                        over.partition.iter().map(|n| self.name(&n.text)).collect();
                    format!("{written} OVER (PARTITION BY {})", groups.join(", "))
                }
            }

            Expr::Window { kind, key, .. } => {
                let ordering: Vec<String> = match key {
                    // `rank` says what it ranks by, so its own key is the order.
                    Some(k) => vec![format!(
                        "{}{}",
                        self.name(&k.column.text),
                        if k.descending { " DESC" } else { "" }
                    )],
                    // `row_number` does not, so the order is the one the rows are
                    // already in. The checker has refused this without a `sort`
                    // before it, so there is always something to find.
                    None => over
                        .order
                        .unwrap_or_default()
                        .iter()
                        .map(|k| {
                            format!(
                                "{}{}",
                                self.name(&k.column.text),
                                if k.descending { " DESC" } else { "" }
                            )
                        })
                        .collect(),
                };
                let mut clauses = Vec::new();
                if !over.partition.is_empty() {
                    let groups: Vec<String> =
                        over.partition.iter().map(|n| self.name(&n.text)).collect();
                    clauses.push(format!("PARTITION BY {}", groups.join(", ")));
                }
                if !ordering.is_empty() {
                    clauses.push(format!("ORDER BY {}", ordering.join(", ")));
                }
                let word = match kind {
                    Window::Rank => "RANK",
                    Window::RowNumber => "ROW_NUMBER",
                };
                format!("{word}() OVER ({})", clauses.join(" "))
            }

            // Unreachable in a checked plan: the checker refuses `matching` anywhere
            // but as a whole `keep` condition, and `condition_sql` renders that case
            // before this function is ever reached.
            Expr::Matching { other, .. } => {
                format!("EXISTS (SELECT 1 FROM {})", self.table(&other.text))
            }
        }
    }

    /// A `keep` condition, which is the one place a filtering join can stand.
    ///
    /// **`EXISTS` needs the name of the table being filtered** so the inner query can
    /// compare against it, and `expr` renders an expression without knowing what it
    /// is being applied to. So this case is handled here, where the alias is in
    /// hand, rather than by threading it through every expression in the grammar for
    /// the sake of one of them.
    fn condition_sql(&self, condition: &Expr, from: &str) -> String {
        let exists = |other: &Name, by: &[JoinKey], negated: bool| {
            let right = self.table(&other.text);
            let on: Vec<String> = by
                .iter()
                .map(|k| {
                    format!(
                        "{right}.{} = {from}.{}",
                        self.name(&k.other.text),
                        self.name(&k.this.text)
                    )
                })
                .collect();
            format!(
                "{}EXISTS (SELECT 1 FROM {right} WHERE {})",
                if negated { "NOT " } else { "" },
                on.join(" AND ")
            )
        };

        match condition {
            Expr::Matching { other, by, .. } => exists(other, by, false),
            Expr::Not { inner, .. } => match inner.as_ref() {
                Expr::Matching { other, by, .. } => exists(other, by, true),
                _ => self.expr(condition),
            },
            _ => self.expr(condition),
        }
    }

    /// How SQL spells each of the grammar's functions.
    ///
    /// A function the grammar has and this list does not is caught by a test that
    /// walks the vocabulary, not by someone remembering to come here.
    fn call(&self, fname: &str, args: &[Expr]) -> String {
        let arg = |i: usize| args.get(i).map(|e| self.expr(e)).unwrap_or_default();
        match fname {
            "total" => format!("sum({})", arg(0)),
            "average" => format!("avg({})", arg(0)),
            "median" => format!("median({})", arg(0)),
            "smallest" => format!("min({})", arg(0)),
            "largest" => format!("max({})", arg(0)),
            "first" => format!("first({})", arg(0)),
            "last" => format!("last({})", arg(0)),
            "unique_count" => format!("count(DISTINCT {})", arg(0)),
            // Counting asks about rows rather than about a column, and `count(*)` is
            // the spelling that counts a row whose every column is missing.
            "row_count" => "count(*)".to_string(),
            // The one variadic function, so it renders its whole argument list
            // rather than a fixed number of slots.
            "first_present" => format!(
                "coalesce({})",
                args.iter()
                    .map(|e| self.expr(e))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            // **`||` rather than `concat`, and it was measured.** DuckDB's
            // `concat` *skips* a null and hands back the rest, so a label built
            // from an absent middle name would come back looking finished. `||`
            // propagates, which is the rule the grammar settled on, and both
            // engines spell it the same way, so this is not a dialect entry.
            "join_text" => format!(
                "({})",
                args.iter()
                    .map(|e| self.expr(e))
                    .collect::<Vec<_>>()
                    .join(" || ")
            ),
            // `STRING` rather than `VARCHAR`, and it was measured: Spark refuses a
            // bare `VARCHAR` because it wants a length, and both engines take
            // `STRING`. One spelling serves both, so this is not a dialect entry.
            "year" => format!("year({})", arg(0)),
            "month" => format!("month({})", arg(0)),
            "day" => format!("day({})", arg(0)),
            "hour" => format!("hour({})", arg(0)),
            "weekday" => self.weekday.replace("{}", &arg(0)),
            "to_number" => format!("CAST({} AS DOUBLE)", arg(0)),
            // **`trunc` first, and it is not decoration.** A bare
            // `CAST(7.5 AS BIGINT)` is 8 on DuckDB and 7 on Spark, and R,
            // pandas and polars all answer 7 — so the same sentence meant two
            // things depending on which engine ran it, with nothing raised.
            // Measured, on both engines, rather than read out of a manual.
            //
            // The grammar names truncation toward zero, because that is what a
            // conversion does in every host language and what five of the six
            // targets already did. Rounding is a different operation and would
            // be a different word.
            "to_whole" => self.to_whole.replace("{}", &arg(0)),
            // **The floored remainder, spelled so both dialects produce it.**
            // Both answer -1 for -7 % 2 and R, Python, pandas and polars all
            // answer 1. Spark has `pmod` and DuckDB does not, so the wrap is
            // used for both rather than keeping a dialect entry for one word.
            "remainder" => format!("((({a}) % ({b})) + ({b})) % ({b})", a = arg(0), b = arg(1)),
            "to_text" => format!("CAST({} AS STRING)", arg(0)),
            "to_date" => format!("CAST({} AS DATE)", arg(0)),
            "trim" => format!("trim({})", arg(0)),
            "characters" => format!("length({})", arg(0)),
            "replace_text" => format!("replace({}, {}, {})", arg(0), arg(1), arg(2)),
            "split_text" => format!("split_part({}, {}, {})", arg(0), arg(1), arg(2)),
            // The one here that is not a call. Both engines spell it as an operator
            // and both are inclusive at each end.
            "between" => format!("({} BETWEEN {} AND {})", arg(0), arg(1), arg(2)),
            "lower" => format!("lower({})", arg(0)),
            "upper" => format!("upper({})", arg(0)),
            other => unreachable!("`{other}` reached the SQL backend without a spelling"),
        }
    }
}

/// The `PARTITION BY` and the fallback `ORDER BY` a window needs, carried down
/// through an expression because a window can sit anywhere inside one.
///
/// **A window is the one value that has to know where it is being written.**
/// Everything else in an expression renders the same wherever it stands;
/// `RANK()` needs the group it is ranking inside, which is the step's `by`, and
/// `ROW_NUMBER()` needs the order the rows are in, which is whatever `sort` last
/// established. Neither is visible from the expression itself.
#[derive(Clone, Copy, Default)]
struct Over<'a> {
    partition: &'a [Name],
    /// The keys of the last `sort` before this step. `row_number` is refused
    /// without one, so this is present whenever it is needed.
    order: Option<&'a [SortKey]>,
    /// Whether this expression stands where a group's answer is handed back to
    /// every row (`add`), rather than where groups collapse (`summarize`). An
    /// aggregate there is a window even with no `by`: the group is the whole
    /// table, and SQL still has to be told, or the bare aggregate beside plain
    /// columns is a `GROUP BY` nobody wrote.
    windowed: bool,
}
/// What an aggregate in `widen`'s `value` is working on.
///
/// `value average([answer])` has to come apart, because the guarded cell holds
/// the column being averaged and the average is written around it. An aggregate
/// that names no column stands for the row, so it gets one.
fn aggregate_argument(value: &Expr) -> Option<Expr> {
    match value {
        Expr::Call {
            name: fname, args, ..
        } if crate::vocabulary::is_aggregate(fname) => {
            Some(args.first().cloned().unwrap_or(Expr::Whole {
                value: 1,
                span: Span::new(0, 0),
            }))
        }
        _ => None,
    }
}

/// A number written so it reads back as the same number.
fn format_decimal(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        format!("{value:.1}")
    } else {
        let mut s = format!("{value}");
        if !s.contains('.') && !s.contains('e') {
            s.push_str(".0");
        }
        s
    }
}
