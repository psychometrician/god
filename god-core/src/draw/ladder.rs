//! A checked pipeline, laid out as a ladder.
//!
//! **The one thing this shows that nothing else does is the table between the
//! steps.** A sentence says what to do; the query says how; neither says what
//! the table holds by the time step four reads it. The checker works that out
//! on every run — it has to, or it could not refuse a column that is not there —
//! and then throws it away. This picks it up.
//!
//! Every band is one step: the words it was written with, and the columns the
//! table has once it has run. A column this step makes is marked, a column it
//! takes away is marked, and a reader can stop on any line and read the table as
//! it stands there.
//!
//! **Where the sentence is wrong the ladder is still drawn**, as far as the
//! checker got, with the refusal under the words that caused it. That is the
//! view worth having and it is not available anywhere else: a query planner has
//! nothing to show for a query that does not plan.

use super::scene::{cells, Ink, RowBuilder, Scene};
use crate::backend::god::step_text;
use crate::check::{self, Schema, Tables};
use crate::diagnostic::Diagnostic;
use crate::plan::{Expr, Plan, Span, Step, Unmatched};

/// The spine.
const RAIL: u16 = 0;
/// Where a step's words start.
const LABEL: u16 = 2;
/// Where a line hanging under a step starts.
const NOTE: u16 = 4;
/// Where the elbow of an arriving table sits.
const ELBOW: u16 = 4;
/// Where that table's name starts.
const TRIBUTARY: u16 = 6;
/// How far right the column strip is allowed to be pushed. A step written out
/// longer than this keeps its strip on the line below rather than shoving every
/// other strip across the page.
///
/// **Every strip starting at one column is what makes the drawing readable
/// downward** — a reader scans the same place on every line to see what the
/// table holds — and the cap is what stops one long `summarize` from buying that
/// alignment with a page of white space.
const LABEL_CAP: u16 = 40;
/// How many columns a strip shows before it starts counting instead.
const CHIP_CAP: usize = 12;

/// One column, as it is about to be drawn.
///
/// The kind is kept apart from the name rather than written into it, because a
/// picture wants to draw them differently: the name is what a reader is looking
/// for and the kind is what they check once. Set side by side with no gap, the
/// text ladder shows `revenue:number` either way.
struct Chip {
    text: String,
    kind: Option<String>,
    ink: Ink,
}

impl Chip {
    fn plain(text: impl Into<String>, ink: Ink) -> Self {
        Chip { text: text.into(), kind: None, ink }
    }

    fn holding(text: impl Into<String>, kind: crate::check::Type, ink: Ink) -> Self {
        Chip {
            text: text.into(),
            kind: match kind.word() {
                "" => None,
                word => Some(format!(":{word}")),
            },
            ink,
        }
    }
}

/// Check the pipeline and lay it out — including when the check refuses.
pub fn build(plan: &Plan, source: &str, schema: &Schema, others: &Tables) -> Scene {
    match check::check_tables(plan, schema, others) {
        // **The checked plan, never the parsed one.** A `join` written without a
        // key has one worked out for it, and the checker writes that back; draw
        // the plan it approved and the picture says `by [id]` for the same reason
        // the sentence written back out does, through the same code.
        Ok(checked) => draw(
            &checked.plan,
            source,
            &checked.entering,
            &checked.schema,
            &checked.assumptions,
            others,
            None,
        ),
        Err(refusal) => {
            // **How far did it get?** The step whose words contain the refusal
            // is the one that could not be checked, so everything before it
            // checked cleanly and can be drawn. Running the checker again over
            // that prefix is how those columns are recovered: it costs one more
            // pass over a handful of steps, and it needs the checker to hand
            // back nothing it does not already hand back.
            let stop = refused_step(plan, &refusal);
            // **The span is carried, not the index.** The prefix drawn below
            // stops short of the step that failed, so an index into it would
            // point past the end — and the words the caret goes under have to
            // come from the sentence the person wrote either way.
            let blame = plan.steps.get(stop).map(|s| s.span());
            let prefix = Plan {
                source: plan.source.clone(),
                source_span: plan.source_span,
                steps: plan.steps[..stop].to_vec(),
            };
            match check::check_tables(&prefix, schema, others) {
                Ok(checked) => draw(
                    &checked.plan,
                    source,
                    &checked.entering,
                    &checked.schema,
                    &checked.assumptions,
                    others,
                    Some((blame, &refusal)),
                ),
                // The prefix will not check either. Nothing can be said about
                // the columns, so say only what was refused.
                Err(_) => {
                    let mut scene = Scene::new();
                    scene.push(RowBuilder::new(0).at(RAIL).put(&plan.source, Ink::Source).done());
                    refusal_band(&mut scene, 1, source, blame, &refusal);
                    scene
                }
            }
        }
    }
}

/// Which step could not be checked.
///
/// The refusal carries the exact characters that caused it, and a step carries
/// the characters it was written with, so the step that owns those characters is
/// the one that failed. Where the refusal names no place — which the checker
/// avoids but the type allows — the blame goes to the first step, because a
/// picture that stops early is honest and one that draws a step that never
/// checked is not.
fn refused_step(plan: &Plan, refusal: &Diagnostic) -> usize {
    let Some(at) = refusal.span else {
        return 0;
    };
    for (i, step) in plan.steps.iter().enumerate() {
        if holds(step.span(), at) {
            return i;
        }
    }
    plan.steps.len()
}

fn holds(outer: Span, inner: Span) -> bool {
    inner.start >= outer.start && inner.start + inner.len <= outer.start + outer.len
}

fn draw(
    plan: &Plan,
    source: &str,
    entering: &[Schema],
    final_schema: &Schema,
    assumptions: &[Diagnostic],
    others: &Tables,
    refused: Option<(Option<Span>, &Diagnostic)>,
) -> Scene {
    // The table as it stands before each step, and once more for what the last
    // step leaves. `states[i]` is what step `i` is handed; `states[i + 1]` is
    // what it hands on.
    let mut states: Vec<&Schema> = entering.iter().collect();
    states.push(final_schema);

    let labels: Vec<String> = plan.steps.iter().map(step_text).collect();

    let mut widest = cells(&plan.source);
    for label in &labels {
        widest = widest.max(LABEL + cells(label));
    }
    let strip_col = (widest + 2).min(LABEL_CAP);

    let mut scene = Scene::new();

    // The table the sentence starts from, at the root of the rail and with
    // nothing marked: nothing has happened to it yet. This is the one band that
    // says what each column holds, because it is the one place a reader has not
    // been told yet.
    let head = RowBuilder::new(0).at(RAIL).put(&plan.source, Ink::Source);
    put_strip(&mut scene, head, strip_col, chips_of(states[0]), 0, " ");

    let last = plan.steps.len().saturating_sub(1);
    for (i, step) in plan.steps.iter().enumerate() {
        let band = i as u16 + 1;
        let is_last = i == last && refused.is_none();
        let rail = if is_last { " " } else { "│" };

        let arrivals = tributaries(step);
        let keys: Vec<String> = arrivals.iter().flat_map(|t| t.keys.clone()).collect();

        let (kept, gone) = chips_between(states[i], states[i + 1], &keys);
        let row = RowBuilder::new(band)
            .at(RAIL)
            .put(if is_last { "└" } else { "├" }, Ink::Rail)
            .at(LABEL)
            .put(&labels[i], Ink::Step);
        put_strip(&mut scene, row, strip_col, kept, band, rail);

        // **A table arriving is drawn, not described.** It hangs under the step
        // that reads it, carrying its own columns, so the ones that cross appear
        // twice — once where they came from and once where they landed — and the
        // key appears in both marked as the thing that matched.
        for arrival in &arrivals {
            let head = RowBuilder::new(band)
                .at(RAIL)
                .put(rail, Ink::Rail)
                .at(ELBOW)
                .put("└", Ink::Rail)
                .at(TRIBUTARY)
                .put(&arrival.other, Ink::Table);
            match others.get(&arrival.other) {
                Some(their) => {
                    put_strip(&mut scene, head, strip_col, chips_from(their, &arrival.keys), band, rail)
                }
                None => scene.push(head.done()),
            }
            scene.push(
                RowBuilder::new(band)
                    .at(RAIL)
                    .put(rail, Ink::Rail)
                    .at(TRIBUTARY)
                    .put(arrival.crossing(others.get(&arrival.other)), Ink::Note)
                    .done(),
            );
        }

        if !gone.is_empty() {
            let row = RowBuilder::new(band).at(RAIL).put(rail, Ink::Rail).at(strip_col);
            put_chips(&mut scene, row, gone);
        }

        for note in assumptions.iter().filter(|a| a.span == Some(step.span())) {
            scene.push(
                RowBuilder::new(band)
                    .at(RAIL)
                    .put(rail, Ink::Rail)
                    .at(NOTE)
                    .put(&note.message, Ink::Note)
                    .done(),
            );
        }

        for (text, ink) in rows_notes(step, &arrivals) {
            scene.push(
                RowBuilder::new(band).at(RAIL).put(rail, Ink::Rail).at(NOTE).put(text, ink).done(),
            );
        }
    }

    if let Some((blame, refusal)) = refused {
        refusal_band(&mut scene, plan.steps.len() as u16 + 1, source, blame, refusal);
    }

    scene
}

/// The step that would not check, drawn with the caret under the words.
///
/// The words come from the sentence itself rather than from the plan, because
/// there is no plan for a step that did not check — and because a caret has to
/// point at what the person actually typed to be worth anything.
fn refusal_band(
    scene: &mut Scene,
    band: u16,
    source: &str,
    step_span: Option<Span>,
    refusal: &Diagnostic,
) {
    scene.push(RowBuilder::new(band).at(RAIL).put("│", Ink::Rail).done());

    let words = step_span.and_then(|s| slice(source, s)).unwrap_or("").trim().to_string();

    scene.push(
        RowBuilder::new(band)
            .at(RAIL)
            .put("╳", Ink::Caret)
            .at(LABEL)
            .put(if words.is_empty() { "this sentence" } else { &words }, Ink::Step)
            .done(),
    );

    // The caret, under the part of those words the refusal named. Both spans are
    // byte offsets into the same string, so the distance between them is a
    // slice, and the width of that slice in cells is where the mark goes.
    if let (Some(step_span), Some(at)) = (step_span, refusal.span) {
        if holds(step_span, at) && at.len > 0 {
            let lead = at.start - step_span.start;
            let raw = slice(source, step_span).unwrap_or("");
            let trimmed = raw.len() - raw.trim_start().len();
            if lead >= trimmed && raw.is_char_boundary(lead) {
                let indent = cells(&raw[trimmed..lead]);
                let width = slice(source, at).map(cells).unwrap_or(1).max(1);
                scene.push(
                    RowBuilder::new(band)
                        .at(LABEL + indent)
                        .put("^".repeat(width as usize), Ink::Caret)
                        .done(),
                );
            }
        }
    }

    scene.push(
        RowBuilder::new(band).at(NOTE).put(&refusal.message, Ink::Warn).done(),
    );
}

fn slice(source: &str, span: Span) -> Option<&str> {
    source.get(span.start..span.start + span.len)
}

/// Every column of a table, with nothing marked, and what each one holds.
///
/// **The kind is said where it is news and nowhere else.** On every chip of
/// every band it would double the width of the drawing to repeat something that
/// has not changed since the line above. It belongs on the table you start from,
/// and on a column a step has just made, because those are the two places a
/// reader has not been told.
fn chips_of(schema: &Schema) -> Vec<Chip> {
    schema
        .columns
        .iter()
        .map(|(name, kind)| Chip::holding(name, *kind, Ink::Column))
        .collect()
}

/// The columns of a table that is arriving, with the ones that matched marked.
///
/// **A key is drawn in both strips.** It is on both tables and it arrives once,
/// which is a thing every join does and almost no tool says out loud — pandas
/// hands back two columns with suffixes nobody asked for. Marking it in both
/// places is that rule, drawn.
fn chips_from(their: &Schema, keys: &[String]) -> Vec<Chip> {
    their
        .columns
        .iter()
        .map(|(name, kind)| {
            if keys.iter().any(|k| k == name) {
                Chip::plain(format!("={name}"), Ink::Key)
            } else {
                Chip::holding(name, *kind, Ink::Column)
            }
        })
        .collect()
}

/// What one step did to the table: the columns it hands on, and the ones it
/// does not.
fn chips_between(before: &Schema, after: &Schema, keys: &[String]) -> (Vec<Chip>, Vec<Chip>) {
    let kept = after
        .columns
        .iter()
        .map(|(name, kind)| {
            if before.get(name).is_none() {
                Chip::holding(format!("+{name}"), *kind, Ink::Added)
            } else if keys.iter().any(|k| k == name) {
                Chip::plain(format!("={name}"), Ink::Key)
            } else {
                Chip::plain(name.clone(), Ink::Column)
            }
        })
        .collect();

    let gone = before
        .columns
        .iter()
        .filter(|(name, _)| after.get(name).is_none())
        .map(|(name, _)| Chip::plain(format!("-{name}"), Ink::Dropped))
        .collect();

    (kept, gone)
}

/// Put a strip beside a step, or under it where the step ran long.
///
/// **A label wider than the cap does not push everybody else's strip across the
/// page.** One long `summarize` would otherwise set the column for the whole
/// drawing, and every other band would be mostly blank.
fn put_strip(
    scene: &mut Scene,
    row: RowBuilder,
    strip_col: u16,
    chips: Vec<Chip>,
    band: u16,
    rail: &str,
) {
    if row.end() + 2 > strip_col {
        scene.push(row.done());
        // The rail carries on down the wrapped line. Without it the ladder
        // appears to stop and start again at the next step.
        let next = RowBuilder::new(band).at(RAIL).put(rail, Ink::Rail).at(strip_col);
        put_chips(scene, next, chips);
    } else {
        put_chips(scene, row.at(strip_col), chips);
    }
}

fn put_chips(scene: &mut Scene, mut row: RowBuilder, chips: Vec<Chip>) {
    let mut first = true;
    for chip in cap(chips) {
        if !first {
            row = row.gap(2);
        }
        first = false;
        row = row.put(chip.text, chip.ink);
        // No gap: the kind sits against the name, so the two cells read as
        // `revenue:number` in text and can be drawn apart in a picture.
        if let Some(kind) = chip.kind {
            row = row.put(kind, Ink::Kind);
        }
    }
    scene.push(row.done());
}

/// Keep a strip readable on a wide table.
///
/// **Truncation never hides a change.** The columns dropped from the picture are
/// always ones this step left alone, so a `+` or a `=` is never the thing that
/// went missing — otherwise the one line a reader came for is the one the cap
/// would take.
fn cap(chips: Vec<Chip>) -> Vec<Chip> {
    if chips.len() <= CHIP_CAP {
        return chips;
    }
    let changed = chips.iter().filter(|c| c.ink != Ink::Column).count();
    let mut budget = CHIP_CAP.saturating_sub(changed);
    let mut out: Vec<Chip> = Vec::new();
    let mut elided = 0usize;
    let mut marker = None;

    for chip in chips {
        if chip.ink != Ink::Column {
            out.push(chip);
        } else if budget > 0 {
            budget -= 1;
            out.push(chip);
        } else {
            if marker.is_none() {
                marker = Some(out.len());
            }
            elided += 1;
        }
    }

    if let Some(at) = marker {
        out.insert(at, Chip::plain(format!("({elided} more)"), Ink::Note));
    }
    out
}

/// A second table, arriving partway down the pipeline.
struct Arrival {
    other: String,
    keys: Vec<String>,
    kind: Arriving,
}

/// **There are exactly three things a second table can send across, and telling
/// them apart is most of why this drawing is worth having.** A `join` sends
/// columns and can multiply the rows. A `matching` sends nothing at all and can
/// only ever remove rows. `add_rows` sends rows and leaves the columns alone.
///
/// The middle one is the whole point. Confusing a filtering join with an inner
/// join is the most common mistake anybody makes with two tables, and the
/// difference is invisible in every spelling of it: the sentences look alike, and
/// so do the answers, until a key repeats.
enum Arriving {
    Columns(Unmatched),
    Nothing { negated: bool },
    Rows,
}

impl Arrival {
    /// The line under the arriving table, saying what actually crossed.
    fn crossing(&self, their: Option<&Schema>) -> String {
        let on = if self.keys.is_empty() {
            "matched".to_string()
        } else {
            format!("matched on {}", self.keys.join(", "))
        };
        match &self.kind {
            Arriving::Columns(_) => {
                let crossing: Vec<String> = their
                    .map(|s| {
                        s.names()
                            .into_iter()
                            .filter(|n| !self.keys.iter().any(|k| k == n))
                            .collect()
                    })
                    .unwrap_or_default();
                match crossing.len() {
                    0 => format!("{on} · nothing else crosses"),
                    1 => format!("{on} · {} crosses over", crossing[0]),
                    _ => format!("{on} · {} cross over", crossing.join(", ")),
                }
            }
            Arriving::Nothing { .. } => format!("{on} · no columns cross, only rows go"),
            Arriving::Rows => "the same columns · its rows are added underneath".to_string(),
        }
    }
}

fn tributaries(step: &Step) -> Vec<Arrival> {
    match step {
        Step::Join { other, by, unmatched, .. } => vec![Arrival {
            other: other.text.clone(),
            keys: by.iter().map(|n| n.text.clone()).collect(),
            kind: Arriving::Columns(*unmatched),
        }],
        Step::AddRows { other, .. } => {
            vec![Arrival { other: other.text.clone(), keys: Vec::new(), kind: Arriving::Rows }]
        }
        Step::Keep { condition, .. } => {
            let mut found = Vec::new();
            find_matching(condition, false, &mut found);
            found
        }
        _ => Vec::new(),
    }
}

/// **A table can be named inside a condition rather than by a step**, which is
/// the one place this is easy to miss: `keep where matching(vip, by [id])` reads
/// a second table without any step mentioning it. Missing one would draw a
/// pipeline with a table quietly absent, which is the worst kind of wrong — it
/// looks complete.
///
/// The grammar allows `matching` to stand alone or under a `not` and nowhere
/// else, so `and` and `or` cannot hold one today. They are walked anyway, so that
/// the day the restriction lifts the drawing does not have to be told.
fn find_matching(e: &Expr, negated: bool, out: &mut Vec<Arrival>) {
    match e {
        Expr::Matching { other, by, .. } => out.push(Arrival {
            other: other.text.clone(),
            keys: by.iter().map(|n| n.text.clone()).collect(),
            kind: Arriving::Nothing { negated },
        }),
        Expr::Not { inner, .. } => find_matching(inner, !negated, out),
        Expr::Logic { left, right, .. } => {
            find_matching(left, negated, out);
            find_matching(right, negated, out);
        }
        _ => {}
    }
}

/// What became of the rows, where a reader could not have guessed.
///
/// **Deliberately quiet.** That `keep` returns fewer rows is not news to anybody,
/// and a note on every band trains the eye to skip all of them. What earns a line
/// is a step whose effect on the rows is not written on its face.
fn rows_notes(step: &Step, arrivals: &[Arrival]) -> Vec<(String, Ink)> {
    let mut said = Vec::new();

    match step {
        Step::Summarize { by, .. } => said.push((
            if by.is_empty() {
                "one row, for the whole table".to_string()
            } else {
                "one row per group".to_string()
            },
            Ink::Note,
        )),
        Step::Take { count, by, .. } => said.push((
            if by.is_empty() {
                format!("at most {count} rows")
            } else {
                format!("at most {count} rows per group")
            },
            Ink::Note,
        )),
        Step::DropDuplicates { .. } => {
            said.push(("one row per distinct combination".to_string(), Ink::Note))
        }
        Step::Lengthen { .. } => {
            said.push(("more rows — one per column stacked".to_string(), Ink::Note))
        }
        Step::Widen { .. } => said.push(("fewer rows — one per group".to_string(), Ink::Note)),
        _ => {}
    }

    for arrival in arrivals {
        match &arrival.kind {
            Arriving::Columns(unmatched) => {
                said.push((
                    match unmatched {
                        Unmatched::This => "every row of this table is kept".to_string(),
                        Unmatched::None => "only the rows that matched".to_string(),
                        Unmatched::Both => "every row of both tables is kept".to_string(),
                    },
                    Ink::Note,
                ));
                // **The one thing the grammar cannot promise here.** The checker
                // is handed column names and never sees a row, so whether a key
                // repeats in the other table is not knowable until the query
                // runs. Saying so is better than a picture that implies a count.
                said.push((
                    format!(
                        "rows may multiply — one for each time {} repeats a key",
                        arrival.other
                    ),
                    Ink::Warn,
                ));
            }
            Arriving::Nothing { negated } => said.push((
                format!(
                    "only the rows that {}match {} — never more than it started with",
                    if *negated { "do not " } else { "" },
                    arrival.other
                ),
                Ink::Note,
            )),
            Arriving::Rows => {
                said.push((format!("more rows — however many {} has", arrival.other), Ink::Note))
            }
        }
    }

    said
}
