//! Whether a pipeline means anything, decided before anything runs.
//!
//! **This is the gate, and everything that can refuse lives here.** A question
//! about whether a request is legal is a question about legality, and it belongs
//! upstream of execution. Answered anywhere downstream it cannot be fatal: the
//! work has already started, so the only thing left is to warn and carry on, and
//! a warning followed by a wrong answer is worse than no check at all.
//!
//! The checker walks the whole plan before a backend is handed anything, which
//! is what lets it report a bad column at step two rather than failing at step
//! seven after the work of the first six. It does that by **threading the schema
//! forward**: each step is checked against the columns that reach it, and each
//! step says what columns leave it. `pick` narrows, `add` extends,
//! `summarize` replaces the whole thing with the grouping columns and the values
//! it made.

use crate::diagnostic::{list, nearest, Diagnostic};
use crate::plan::*;
use crate::vocabulary;

/// What the grammar needs to know about a column. Deliberately coarse: the
/// distinctions it draws are the ones that change whether a sentence is legal,
/// and nothing finer, because a type system is the host's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    Number,
    Text,
    Truth,
    Date,
    /// A date that carries a time of day, which a plain date does not.
    ///
    /// **The two are one kind to a reader and two to the checker, and that
    /// split is the whole point of this variant.** `kind` reports both as
    /// `"date"`, so `pick where kind is "date"` picks a column of either —
    /// which is what somebody asking for the date columns means. Every
    /// comparison, sort and join treats them as one, through `agrees_with`.
    ///
    /// One word tells them apart, and it is the word that proves they are not
    /// the same thing: `hour`. A year, a month, a day and a weekday are parts
    /// of the calendar and every date has them; an hour is not, and a plain
    /// date has none. Before this variant existed the bindings collapsed both
    /// into `Date`, so `hour` of a plain date was accepted and answered nought
    /// on DuckDB while printed polars refused the operation outright — one
    /// sentence, two answers, which is the disagreement this grammar exists to
    /// end.
    Timestamp,
    /// A type the grammar has no opinion about, which passes every test rather
    /// than failing them. Refusing what it cannot classify would refuse working
    /// pipelines over columns the grammar simply has not met.
    Unknown,
}

impl Type {
    /// How this kind is written in a sentence, which is the same spelling
    /// `--columns` takes. `Unknown` has none: a column the grammar has no
    /// opinion about cannot be selected by an opinion.
    pub fn word(self) -> &'static str {
        match self {
            Type::Number => "number",
            Type::Text => "text",
            Type::Truth => "truth",
            // **Both answer to `"date"`, deliberately.** This is the spelling
            // `kind is "..."` compares against, and a person asking which
            // columns hold dates means both.
            Type::Date | Type::Timestamp => "date",
            Type::Unknown => "",
        }
    }

    /// Every kind a column can be said to hold, for the message when someone
    /// names one that is not on the list.
    pub fn words() -> Vec<&'static str> {
        vec!["text", "number", "truth", "date"]
    }

    pub fn name(self) -> &'static str {
        match self {
            Type::Number => "a number",
            Type::Text => "text",
            Type::Truth => "yes or no",
            Type::Date => "a date",
            Type::Timestamp => "a date and a time",
            Type::Unknown => "a value",
        }
    }

    fn agrees_with(self, other: Type) -> bool {
        if self == Type::Unknown || other == Type::Unknown || self == other {
            return true;
        }
        // **A date and a date-with-a-time agree everywhere except `hour`.**
        // They compare, they sort, they join, and a reader thinks of them as
        // one kind. Splitting them here would refuse working pipelines to buy
        // nothing: the one place the difference is real is checked on its own,
        // below, where `hour` asks for a time and will not take a date.
        matches!(
            (self, other),
            (Type::Date, Type::Timestamp) | (Type::Timestamp, Type::Date)
        )
    }
}

/// The columns a table has, in order, with what each one holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub columns: Vec<(String, Type)>,
}

impl Schema {
    pub fn new(columns: impl IntoIterator<Item = (impl Into<String>, Type)>) -> Self {
        Schema { columns: columns.into_iter().map(|(n, t)| (n.into(), t)).collect() }
    }

    pub fn names(&self) -> Vec<String> {
        self.columns.iter().map(|(n, _)| n.clone()).collect()
    }

    pub fn get(&self, name: &str) -> Option<Type> {
        self.columns.iter().find(|(n, _)| n == name).map(|(_, t)| *t)
    }

    fn set(&mut self, name: &str, kind: Type) {
        if let Some(slot) = self.columns.iter_mut().find(|(n, _)| n == name) {
            slot.1 = kind;
        } else {
            self.columns.push((name.to_string(), kind));
        }
    }
}

/// The result of checking: the columns the pipeline produces, and anything the
/// grammar chose that the caller did not say.
pub struct Checked {
    /// The plan a backend should render, which is not always the plan that was
    /// parsed.
    ///
    /// `join` may work its key out from the names two tables share, and a
    /// backend cannot: it is handed a plan and never sees a schema for the other
    /// table. So the checker writes what it settled on back into the plan, and
    /// **what gets rendered is exactly what was approved.** It also means
    /// `show --as god` prints the assumption as a visible clause rather than
    /// leaving the reader to guess what matched.
    pub plan: Plan,
    /// What the pipeline ends with.
    pub schema: Schema,
    /// What each step is handed, in order, so a backend can write correct code.
    ///
    /// This is not bookkeeping for its own sake. `add` covers making a column and
    /// remaking one, because to the person writing it those are one act — but SQL
    /// spells them differently and has to be told which it is. **The grammar
    /// keeps one word and the backend does the work**, which is the trade this
    /// design makes everywhere: irregularity belongs to whoever is closest to the
    /// engine, never to the person writing the sentence.
    pub entering: Vec<Schema>,
    pub assumptions: Vec<Diagnostic>,
}

/// The other tables a pipeline names, besides the one at its head.
///
/// Only `join` reaches for these, and it is the first verb that names a second
/// table at all. They are carried separately from the head table rather than in
/// one map including it, because the head is not optional and these are: a
/// pipeline with no join needs none of this, and should not have to say so.
#[derive(Debug, Clone, Default)]
pub struct Tables {
    pub tables: Vec<(String, Schema)>,
}

impl Tables {
    pub fn new(tables: impl IntoIterator<Item = (impl Into<String>, Schema)>) -> Self {
        Tables { tables: tables.into_iter().map(|(n, s)| (n.into(), s)).collect() }
    }

    pub fn empty() -> Self {
        Tables { tables: Vec::new() }
    }

    pub fn get(&self, name: &str) -> Option<&Schema> {
        self.tables.iter().find(|(n, _)| n == name).map(|(_, s)| s)
    }

    pub fn names(&self) -> Vec<String> {
        self.tables.iter().map(|(n, _)| n.clone()).collect()
    }
}

pub fn check(plan: &Plan, input: &Schema) -> Result<Checked, Diagnostic> {
    check_tables(plan, input, &Tables::empty())
}

/// Check a pipeline that may name more than one table.
pub fn check_tables(
    plan: &Plan,
    input: &Schema,
    others: &Tables,
) -> Result<Checked, Diagnostic> {
    // The pipeline's own head table is a described table too. `sales then
    // add_rows sales` used to be refused as "no other table was described",
    // which told a caller who had just described `sales` to describe it. The
    // head joins the list under its own name — appended last, so an explicit
    // description of the same name still wins the lookup.
    let mut others = others.clone();
    others.tables.push((plan.source.clone(), input.clone()));
    let others = &others;

    let mut resolved = plan.clone();
    let mut schema = input.clone();
    let mut entering = Vec::with_capacity(plan.steps.len());
    let mut assumptions = Vec::new();

    // Whether the rows are in an order anybody asked for. `sort` settles it;
    // anything that reshapes or regroups the table unsettles it again. A grouped
    // `take` needs to know, and so does `row_number`, and both need it badly:
    // "the first row of each group" means nothing until something has said first
    // by what, and neither does "which row is this".
    let mut ordered = false;

    let last = plan.steps.len().saturating_sub(1);
    for (at, step) in resolved.steps.iter_mut().enumerate() {
        entering.push(schema.clone());
        // **The one step whose output columns come from the data.** Every other
        // step maps a known schema to a known schema, and the checker's contract
        // is that it can say what each one produces. So a `widen` that does not
        // say what it makes is allowed to be the answer and not allowed to be
        // the middle of one — which keeps the contract at one mode rather than
        // leaking an unknown schema through every step after it.
        if let Step::Widen { giving, span, .. } = step {
            if giving.is_empty() && at != last {
                return Err(Diagnostic::illegal(
                    "the columns `widen` makes come from the data, so the grammar cannot know their names until the query runs, and a step after it would be naming columns nothing has checked. Say what it makes: `giving [q1, q2, q3]`, or let the `widen` be the last step",
                    *span,
                ));
            }
        }
        if let Step::Take { by, last, ties, span, .. } = step {
            // **A tie is a tie in some ordering**, so with nothing sorted there
            // is nothing to be tied on and the word would mean nothing at all.
            // This is the sharpest of the three sort demands here: the other two
            // would give a defensible answer without one, and this would give
            // none.
            if *ties && !ordered {
                return Err(Diagnostic::illegal(
                    "`with ties` keeps the rows level with the last one taken, and nothing has said what they would be level *on*. Sort before it: `then sort [points] descending then take 3 with ties`",
                    *span,
                ));
            }
            if !by.is_empty() && !ordered {
                return Err(Diagnostic::illegal(
                    "`take ... by` gives the first rows of each group, and nothing has said what order the rows are in, so there is no first. Sort before it: `then sort [when] descending then take 1 by [id]`",
                    *span,
                ));
            }
            // **`take_last` needs an order even ungrouped, where `take` does
            // not, and the asymmetry is the honest one.** "The first three
            // rows" of an unsorted table is at least the three the pipeline
            // reached first. "The last three" is a claim about an end, and a
            // table has no end until something says which way it runs.
            if *last && by.is_empty() && !ordered {
                return Err(Diagnostic::illegal(
                    "`take_last` gives the rows at the far end, and nothing has said which end that is. Sort before it: `then sort [when] then take_last 3`. For the rows a pipeline reaches first, `take` needs no sort",
                    *span,
                ));
            }
        }
        // **`row_number` is the one window that is not told what to order by**,
        // so it can only mean the order the rows are already in, and a table has
        // none until a `sort` gives it one. `rank` carries its own key and is
        // never in this position, which is the difference between the two.
        if !ordered {
            if let Some((word, span)) = window_needing_order(step) {
                return Err(Diagnostic::illegal(
                    format!("`{word}` reads the rows in the order they are in, and nothing has said what that order is. Sort before it: `then sort [when] then add [so_far] as {word}`. `rank([revenue] descending)` is the one that says what it goes by, so it needs no sort"),
                    span,
                ));
            }
        }
        schema = check_step(step, &schema, others, &mut assumptions)?;
        ordered = match step {
            Step::Sort { .. } => true,
            // These impose an order of their own, or destroy the one there was.
            Step::Summarize { .. }
            | Step::Join { .. }
            | Step::AddRows { .. }
            | Step::DropDuplicates { .. }
            // Reshaping settles an order of its own, the way `summarize` does,
            // and it is not the one anybody asked for. So whatever a `sort`
            // established before it no longer describes these rows.
            | Step::Lengthen { .. }
            | Step::Widen { .. } => false,
            _ => ordered,
        };
    }

    Ok(Checked { plan: resolved, schema, entering, assumptions })
}

/// Turn `add where name starts "q" as value * 2` into one value per column.
///
/// The matched columns keep their names, because `add` already means make or
/// replace. `summarize` keeps them for the same reason: the answer for `q1` is
/// called `q1`, which is what a reader expects and what dplyr's `across` does
/// when it is not given a template.
fn expand_across(step: &mut Step, schema: &Schema) -> Result<(), Diagnostic> {
    let (values, across, span) = match step {
        Step::Add { values, across, span, .. } if across.is_some() => (values, across, *span),
        Step::Summarize { values, across, span, .. } if across.is_some() => {
            (values, across, *span)
        }
        _ => return Ok(()),
    };
    let rule = across.take().expect("just checked");

    let chosen = columns_matching(&rule.selector, schema)?;
    if chosen.is_empty() {
        return Err(Diagnostic::illegal(
            format!(
                "no column's name matches that, so this would make nothing. The table has: {}",
                list(&schema.names())
            ),
            span,
        ));
    }

    for column in chosen {
        let name = Name { text: column, span };
        let value = substitute_value(&rule.value, &name);
        values.push(Named { name, value });
    }
    Ok(())
}

/// Turn `keep where any name starts "q" as value > 3` into one ordinary
/// condition per matched column, joined by `or` — or by `and` for `every`.
///
/// **The same move `across` makes, for a question instead of a value.** Once
/// this has run there is no such thing as a quantified condition: what the type
/// rules, the seat rules and every backend see is a condition somebody could
/// have written by hand. None of that code knows this exists, which is why it
/// cost no new checking.
///
/// **The empty match is a refusal rather than an answer**, and the reason is
/// that both answers are defensible and neither is guessable: an `any` over no
/// columns is false and an `every` over no columns is true, so the same
/// mistyped selector would silently empty the table or silently keep it whole.
/// `across` refuses the same case for the same reason.
fn expand_quantified(condition: &mut Expr, schema: &Schema) -> Result<(), Diagnostic> {
    let (every, selector, test, span) = match condition {
        Expr::Quantified { every, selector, test, span } => {
            (*every, selector.clone(), test.clone(), *span)
        }
        _ => return Ok(()),
    };

    let chosen = columns_matching(&selector, schema)?;
    if chosen.is_empty() {
        return Err(Diagnostic::illegal(
            format!(
                "no column's name matches that, so there is nothing to ask. The table has: {}",
                list(&schema.names())
            ),
            span,
        ));
    }

    let joiner = if every { Logic::And } else { Logic::Or };
    let mut built: Option<Expr> = None;
    for column in chosen {
        let name = Name { text: column, span };
        let one = substitute_value(&test, &name);
        built = Some(match built {
            None => one,
            Some(so_far) => Expr::Logic {
                op: joiner,
                left: Box::new(so_far),
                right: Box::new(one),
                span,
            },
        });
    }
    *condition = built.expect("the empty case was refused above");
    Ok(())
}

/// `value` becomes the column being worked on, everywhere inside the expression.
fn substitute_value(value: &Expr, column: &Name) -> Expr {
    let recur = |e: &Expr| Box::new(substitute_value(e, column));
    match value {
        Expr::ColumnValue { .. } => Expr::Column(column.clone()),
        Expr::Arithmetic { op, left, right, span } => Expr::Arithmetic {
            op: *op,
            left: recur(left),
            right: recur(right),
            span: *span,
        },
        Expr::Compare { op, left, right, span } => Expr::Compare {
            op: *op,
            left: recur(left),
            right: recur(right),
            span: *span,
        },
        Expr::Logic { op, left, right, span } => Expr::Logic {
            op: *op,
            left: recur(left),
            right: recur(right),
            span: *span,
        },
        Expr::Not { inner, span } => Expr::Not { inner: recur(inner), span: *span },
        Expr::IsMissing { inner, negated, span } => Expr::IsMissing {
            inner: recur(inner),
            negated: *negated,
            span: *span,
        },
        Expr::In { left, set, negated, span } => Expr::In {
            left: recur(left),
            set: set.iter().map(|e| substitute_value(e, column)).collect(),
            negated: *negated,
            span: *span,
        },
        Expr::TextTest { op, left, value: v, span } => Expr::TextTest {
            op: *op,
            left: recur(left),
            value: recur(v),
            span: *span,
        },
        Expr::Call { name, args, span } => Expr::Call {
            name: name.clone(),
            args: args.iter().map(|e| substitute_value(e, column)).collect(),
            span: *span,
        },
        Expr::Rolling { agg, agg_span, args, count, span } => Expr::Rolling {
            agg: agg.clone(),
            agg_span: *agg_span,
            args: args.iter().map(|e| substitute_value(e, column)).collect(),
            count: Box::new(substitute_value(count, column)),
            span: *span,
        },
        Expr::Lookup { subject, pairs, otherwise, span } => Expr::Lookup {
            subject: recur(subject),
            pairs: pairs
                .iter()
                .map(|(f, t)| (substitute_value(f, column), substitute_value(t, column)))
                .collect(),
            otherwise: recur(otherwise),
            span: *span,
        },
        other => other.clone(),
    }
}

/// Look through a `lower(...)` or `upper(...)` to what is inside, and hand back
/// the folding it asked for.
///
/// **This is how the name tests answer case without a flag.** `name` is text and
/// text has a case, so `lower(name) starts "q"` is two words the vocabulary
/// already had, rather than a second spelling of `starts`.
fn unfold(e: &Expr) -> (&Expr, fn(&str) -> String) {
    if let Expr::Call { name, args, .. } = e {
        if args.len() == 1 {
            if name == "lower" {
                return (&args[0], |s| s.to_lowercase());
            }
            if name == "upper" {
                return (&args[0], |s| s.to_uppercase());
            }
        }
    }
    (e, |s| s.to_string())
}

/// The columns whose names answer yes to a `pick where` condition.
///
/// The condition is evaluated once per column, with `name` standing for that
/// column's own name. Only the shapes that can be written about a name are
/// handled, and anything else is refused by `check_expr` before this runs.
fn columns_matching(condition: &Expr, schema: &Schema) -> Result<Vec<String>, Diagnostic> {
    let mut chosen = Vec::new();
    for (name, kind) in &schema.columns {
        if holds_for(condition, name, *kind)? {
            chosen.push(name.clone());
        }
    }
    Ok(chosen)
}

/// The two things a column can be asked about without looking at a row: what it
/// is called, and what it holds.
fn holds_for(condition: &Expr, name: &str, kind: Type) -> Result<bool, Diagnostic> {
    match condition {
        Expr::TextTest { op, left, value, span } => {
            let (subject, fold) = unfold(left.as_ref());
            let Expr::ColumnName { .. } = subject else {
                return Err(Diagnostic::illegal(
                    "`pick where` asks about a column's name, so the thing being tested is `name`: `pick where name starts \"q\"`",
                    left.span(),
                ));
            };
            let Expr::Text { value, .. } = value.as_ref() else {
                return Err(Diagnostic::illegal(
                    "this compares a name against text, written in double quotes: `pick where name starts \"q\"`",
                    *span,
                ));
            };
            let name = fold(name);
            Ok(match op {
                TextOp::Starts => name.starts_with(value.as_str()),
                TextOp::Ends => name.ends_with(value.as_str()),
                TextOp::Contains => name.contains(value.as_str()),
            })
        }
        Expr::Logic { op, left, right, .. } => {
            let l = holds_for(left, name, kind)?;
            let r = holds_for(right, name, kind)?;
            Ok(match op {
                Logic::And => l && r,
                Logic::Or => l || r,
            })
        }
        Expr::Not { inner, .. } => Ok(!holds_for(inner, name, kind)?),
        Expr::Compare { op, left, right, span } => {
            let Expr::Text { value, .. } = right.as_ref() else {
                return Err(Diagnostic::illegal(
                    "this compares a column against text, written in double quotes: `name is \"revenue\"`, or `kind is \"number\"`",
                    *span,
                ));
            };
            // What is being compared: the column's name, or what it holds,
            // either of them possibly with its case folded.
            let subject = match unfold(left.as_ref()) {
                // `name is "revenue"` is worth allowing: it is how someone asks
                // for one exact name inside a longer condition.
                (Expr::ColumnName { .. }, fold) => fold(name),
                (Expr::ColumnKind { .. }, fold) => {
                    let known = Type::words();
                    if !known.contains(&value.as_str()) {
                        return Err(Diagnostic::illegal(
                            format!(
                                "a column holds one of {}, and `\"{value}\"` is none of them",
                                list(&known.iter().map(|w| format!("`{w}`")).collect::<Vec<_>>())
                            ),
                            right.span(),
                        ));
                    }
                    fold(kind.word())
                }
                _ => {
                    return Err(Diagnostic::illegal(
                        "the `where` that chooses columns asks about `name` or `kind`: `pick where kind is \"number\"`",
                        left.span(),
                    ))
                }
            };
            Ok(match op {
                Compare::Is => subject == *value,
                Compare::IsNot => subject != *value,
                _ => {
                    return Err(Diagnostic::illegal(
                        "this is text, so it can be `is`, `is not`, `starts`, `ends` or `contains`, and not put in order",
                        *span,
                    ))
                }
            })
        }
        other => Err(Diagnostic::illegal(
            "the `where` that chooses columns asks about a column's name or what it holds: `pick where name starts \"q\"`, or `pick where kind is \"number\"`. Either can be joined with `and`, `or` and `not`",
            other.span(),
        )),
    }
}

/// Where a bare `row_number()` sits in this step, if one does.
///
/// Only the values of `add` are searched, because that is the only step a window
/// may stand in at all; `summarize` and `keep` refuse one outright, with their
/// own messages, and reporting the order first would send someone to add a sort
/// that will not help.
/// **`rank` is the only window that is told its own order**, so it is the only
/// one missing from this. Every other one means something *so far* or *next
/// along*, and neither means anything until a `sort` has said in what order.
fn window_needing_order(step: &Step) -> Option<(&'static str, Span)> {
    let Step::Add { values, .. } = step else {
        return None;
    };
    let mut found = None;
    for Named { value, .. } in values {
        value.walk(&mut |e| {
            let needs = match e {
                Expr::Window { kind: Window::RowNumber, span, .. } => {
                    Some(("row_number()", *span))
                }
                // A window of the last n rows means nothing until something
                // has said which way the rows run, the same as a total so far.
                Expr::Rolling { span, .. } => Some(("rolling(...)", *span)),
                Expr::Call { name, span, .. }
                    if matches!(
                        name.as_str(),
                        "running_total" | "previous" | "following" | "latest"
                    ) =>
                {
                    Some((
                        match name.as_str() {
                            "running_total" => "running_total(...)",
                            "previous" => "previous(...)",
                            "latest" => "latest(...)",
                            _ => "following(...)",
                        },
                        *span,
                    ))
                }
                _ => None,
            };
            if found.is_none() {
                found = needs;
            }
        });
    }
    found
}

fn check_step(
    step: &mut Step,
    schema: &Schema,
    others: &Tables,
    assumptions: &mut Vec<Diagnostic>,
) -> Result<Schema, Diagnostic> {
    // **`add where` and `summarize where` are expanded before anything else
    // looks at the step**, into one ordinary value per matched column. Every
    // rule below then applies to the expansion rather than to the pattern, which
    // is why an aggregate written across many columns is checked by exactly the
    // code that checks one written by hand.
    expand_across(step, schema)?;
    // The same, for `keep where any ... as ...`. It has to happen before the
    // filtering-join peel below, so that `matching` still sees a condition it
    // recognizes and every rule about what may stand in a `keep` applies to the
    // expansion rather than to the pattern.
    if let Step::Keep { condition, .. } = step {
        expand_quantified(condition, schema)?;
    }

    match step {
        Step::Keep { condition, .. } => {
            // A filtering join is peeled off before the ordinary check, which
            // refuses `matching` anywhere else. What the checker settles on goes
            // back into the plan, so a backend renders the key that was approved
            // rather than working it out a second time from a schema it cannot
            // see.
            if let Some(Expr::Matching { other, by, span }) = filtering_join(condition) {
                let keys = check_matching(schema, other, by, *span, others, assumptions)?;
                *by = keys;
                return Ok(schema.clone());
            }

            let kind = check_expr(condition, schema)?;

            // Filtering on an aggregate is the one shape people reach for that
            // the grammar answers with a different sentence rather than a
            // different verb. Saying so is the whole job of the message.
            if condition.aggregates() {
                return Err(Diagnostic::illegal(
                    "`keep` decides one row at a time, so it cannot ask a question about a whole group. Summarize first, then keep: `then summarize [n] as row_count() by [g] then keep where [n] > 5`",
                    condition.span(),
                ));
            }
            // **A window is worked out over the rows that survive**, so it
            // cannot be what decides which rows survive. The two spellings that
            // work are both worth naming: one keeps the place as a column, the
            // other never makes one.
            if condition.windows() {
                return Err(Diagnostic::illegal(
                    "a place is worked out over the rows that are left, so it cannot be what chooses them. Make it a column first: `then add [place] as rank([revenue] descending) then keep where [place] <= 3`. For the first rows of each group, `then sort [revenue] descending then take 3 by [g]` says it in one step",
                    condition.span(),
                ));
            }
            if !kind.agrees_with(Type::Truth) {
                return Err(Diagnostic::illegal(
                    format!(
                        "`keep where` needs a question that is either yes or no, and this is {}. Compare it to something: `is`, `>`, `<`, or `in {{...}}`",
                        kind.name()
                    ),
                    condition.span(),
                ));
            }
            Ok(schema.clone())
        }

        // Choosing by the shape of a name. The checker knows every column, so it
        // works out which ones matched and writes them into the plan, exactly as
        // it settles `join`'s key. A backend is handed an ordinary list.
        Step::Pick { names, all_but, condition, span } if condition.is_some() => {
            let chosen = columns_matching(condition.as_ref().unwrap(), schema)?;
            if chosen.is_empty() {
                return Err(Diagnostic::illegal(
                    format!(
                        "no column's name matches that, so this would leave the table with no columns. It has: {}",
                        list(&schema.names())
                    ),
                    *span,
                ));
            }
            let columns: Vec<(String, Type)> = schema
                .columns
                .iter()
                .filter(|(n, _)| chosen.contains(n))
                .cloned()
                .collect();
            *names = chosen.into_iter().map(|text| Name { text, span: *span }).collect();
            *all_but = false;
            // Cleared, so the plan a backend renders is the one that was
            // approved and holds no rule it would have to apply a second time.
            *condition = None;
            Ok(Schema { columns })
        }

        Step::Pick { names, all_but, .. } => {
            for name in names.iter() {
                known(name, schema)?;
            }
            let chosen: Vec<String> = names.iter().map(|n| n.text.clone()).collect();
            let columns = if *all_but {
                schema
                    .columns
                    .iter()
                    .filter(|(n, _)| !chosen.contains(n))
                    .cloned()
                    .collect::<Vec<_>>()
            } else {
                // The caller's order, not the table's: `pick [b, a]` is how you
                // reorder, and a verb that silently kept the original order would
                // be accepting a clause and ignoring it.
                names
                    .iter()
                    .map(|n| (n.text.clone(), schema.get(&n.text).unwrap_or(Type::Unknown)))
                    .collect()
            };
            if columns.is_empty() {
                return Err(Diagnostic::illegal(
                    "this would leave the table with no columns at all. Keep at least one: `pick [a]`, or name fewer columns to drop",
                    step.span(),
                ));
            }
            Ok(Schema { columns })
        }

        Step::Add { values, by, .. } => {
            for name in by.iter() {
                known(name, schema)?;
            }
            made_here_is_not_visible(values, schema, "add")?;
            let mut out = schema.clone();
            for Named { name, value } in values.iter() {
                let kind = check_expr(value, schema)?;
                // **A window is the second thing `by` changes**, and it was not
                // always: this reported that `by` did nothing for a `rank`, which
                // is wrong, because the group is what the rank restarts inside.
                // Both kinds look at more than one row, and that is the property
                // `by` is about.
                if !by.is_empty() && !value.aggregates() && !value.windows() {
                    assumptions.push(Diagnostic::assumption(
                        format!(
                            "`[{}]` does not ask anything about a group, so `by` changes nothing for it. Remove `by`, or use a value like `total(...)` that spans the group",
                            name.text
                        ),
                        value.span(),
                    ));
                }
                out.set(&name.text, kind);
            }
            duplicate_names(values)?;
            Ok(out)
        }

        Step::Summarize { values, by, .. } => {
            for name in by.iter() {
                known(name, schema)?;
            }
            duplicate_names(values)?;
            made_here_is_not_visible(values, schema, "summarize")?;

            for Named { name, value } in values.iter() {
                check_expr(value, schema)?;
                // A window answers once per row and `summarize` returns one row
                // per group, so there is nowhere for the answer to go. Its own
                // message, because the general one below would tell someone to
                // wrap a rank in `total(...)`, which is not what they meant.
                if value.windows() {
                    return Err(Diagnostic::illegal(
                        format!(
                            "`summarize` returns one row for each group, and a place is worked out for every row, so `[{}]` has nowhere to go. Add it before the summarize, or ask for the value you want here: `largest(...)`, `first(...)`, `row_count()`",
                            name.text
                        ),
                        value.span(),
                    ));
                }
                // Every value has to collapse the group, because summarize
                // returns one row per group and a value that does not collapse
                // has many candidates and no rule for choosing between them.
                if !value.aggregates() {
                    return Err(Diagnostic::illegal(
                        format!(
                            "`summarize` returns one row for each group, so `[{}]` has to be a value that spans the group. Wrap it: `total(...)`, `average(...)`, `first(...)`, or count the rows with `row_count()`",
                            name.text
                        ),
                        value.span(),
                    ));
                }
            }

            // A grouping column keeps its own name and type; the values follow.
            let mut columns: Vec<(String, Type)> = by
                .iter()
                .map(|n| (n.text.clone(), schema.get(&n.text).unwrap_or(Type::Unknown)))
                .collect();
            for Named { name, value } in values.iter() {
                let kind = check_expr(value, schema)?;
                columns.push((name.text.clone(), kind));
            }
            Ok(Schema { columns })
        }

        Step::Sort { keys, .. } => {
            for key in keys {
                known(&key.column, schema)?;
            }
            Ok(schema.clone())
        }

        Step::Take { by, .. } => {
            for name in by.iter() {
                known(name, schema)?;
            }
            Ok(schema.clone())
        }

        Step::AddRows { other, span } => {
            let Some(right) = others.get(&other.text) else {
                let known = others.names();
                let have = if known.is_empty() {
                    "no other table was described".to_string()
                } else {
                    format!("the tables described are: {}", list(&known))
                };
                return Err(Diagnostic::illegal(
                    format!("this adds rows from a table called `{}`, and {have}.", other.text),
                    other.span,
                ));
            };

            // **The columns have to match, and filling the difference with
            // `missing` is what this refuses.** dplyr's `bind_rows` fills, which
            // is convenient exactly until the day the two tables differ because
            // one of them is wrong, and then it hands back a column that is half
            // empty and says nothing. A column that appears on one side only is
            // either a mistake or a decision, and the caller is the one who
            // knows which.
            let mine = schema.names();
            let theirs = right.names();
            let only_here: Vec<String> =
                mine.iter().filter(|n| right.get(n).is_none()).cloned().collect();
            let only_there: Vec<String> =
                theirs.iter().filter(|n| schema.get(n).is_none()).cloned().collect();
            if !only_here.is_empty() || !only_there.is_empty() {
                let mut said = Vec::new();
                if !only_here.is_empty() {
                    said.push(format!("this table has {} and `{}` does not", list(&only_here), other.text));
                }
                if !only_there.is_empty() {
                    said.push(format!("`{}` has {} and this table does not", other.text, list(&only_there)));
                }
                return Err(Diagnostic::illegal(
                    format!(
                        "`add_rows` needs both tables to have the same columns: {}. Add what is missing, or drop it with `pick`",
                        said.join(", and ")
                    ),
                    *span,
                ));
            }

            for (name, kind) in &schema.columns {
                let theirs = right.get(name).unwrap_or(Type::Unknown);
                if !kind.agrees_with(theirs) {
                    return Err(Diagnostic::illegal(
                        format!(
                            "`[{name}]` is {} here and {} in `{}`, so the two columns cannot be stacked. Convert one of them first, or drop it from both with `pick all_but [{name}]`",
                            kind.name(),
                            theirs.name(),
                            other.text
                        ),
                        *span,
                    ));
                }
            }
            Ok(schema.clone())
        }

        Step::DropDuplicates { .. } => Ok(schema.clone()),

        Step::Rename { values, span } => {
            let mut out = schema.clone();
            for Named { name, value } in values.iter() {
                let Expr::Column(from) = value else {
                    return Err(Diagnostic::illegal(
                        format!(
                            "`rename` takes the column to rename, not a value: `rename [{}] as [old_name]`. To make a column from a value, use `add`",
                            name.text
                        ),
                        value.span(),
                    ));
                };
                let kind = known(from, schema)?;
                if schema.get(&name.text).is_some() {
                    return Err(Diagnostic::illegal(
                        format!(
                            "the table already has a column called `{}`, so renaming `{}` to it would leave two of that name. The new name goes first: `rename [new] as [{}]`",
                            name.text, from.text, from.text
                        ),
                        name.span,
                    ));
                }
                let slot = out
                    .columns
                    .iter_mut()
                    .find(|(n, _)| n == &from.text)
                    .expect("the column was just checked");
                *slot = (name.text.clone(), kind);
            }
            duplicate_names(values)?;
            let _ = span;
            Ok(out)
        }

        Step::DropMissing { names, .. } => {
            for name in names.iter() {
                known(name, schema)?;
            }
            Ok(schema.clone())
        }

        Step::AddCombinations { names, by, span } => {
            check_add_combinations(names, by, *span, schema)
        }

        Step::FillMissing { values, .. } => {
            let mut out = schema.clone();
            for Named { name, value } in values.iter() {
                let kind = known(name, schema)?;
                let filler = check_expr(value, schema)?;
                if !kind.agrees_with(filler) {
                    return Err(Diagnostic::illegal(
                        format!(
                            "`[{}]` is {} and this fills it with {}, which would change what the column holds. Fill it with {} instead, or convert the column first",
                            
                            name.text,
                            kind.name(),
                            filler.name(),
                            kind.name()
                        ),
                        value.span(),
                    ));
                }
                if value.aggregates() {
                    return Err(Diagnostic::illegal(
                        "`fill_missing` fills one row at a time, so it cannot use a value that spans a group. Work it out with `summarize` first",
                        value.span(),
                    ));
                }
                if value.windows() {
                    return Err(Diagnostic::illegal(
                        "`fill_missing` fills each hole from its own row, and a value that looks along the rows needs their order settled first. Fill down in `add`, where the sort is asked for: `then sort [day] then add [x] as latest([x])`",
                        value.span(),
                    ));
                }
                out.set(&name.text, if kind == Type::Unknown { filler } else { kind });
            }
            duplicate_names(values)?;
            Ok(out)
        }

        Step::Lengthen { names, all_but, condition, name, value, resolved, span } => {
            check_lengthen(names, all_but, condition, name, value, resolved, *span, schema)
        }

        Step::Widen { name, value, by, missing, giving, span } => {
            check_widen(name, value, by, missing, giving, *span, schema, assumptions)
        }

        Step::Join { other, by, unmatched, span } => {
            let (out, keys) =
                check_join(schema, other, by, *unmatched, *span, others, assumptions)?;
            // What the checker settled on goes back into the plan, so the
            // backend renders the join that was approved rather than one it
            // would have to infer a second time.
            *by = keys;
            Ok(out)
        }
    }
}

/// What `lengthen` refuses, and what it works out so no backend has to.
///
/// **The pattern is resolved away here**, exactly as `pick where`, `join`'s key
/// and `across` are. The checker knows every column name, so it reads each one
/// apart and writes the literal pieces into the plan. A backend is handed
/// literals and never sees a `{`, which is why this needs no string function in
/// a query and runs unchanged on any engine.
fn check_lengthen(
    names: &mut Vec<Name>,
    all_but: &mut bool,
    condition: &mut Option<Expr>,
    pattern: &Pattern,
    value: &Option<Name>,
    resolved: &mut Option<Lengthened>,
    span: Span,
    schema: &Schema,
) -> Result<Schema, Diagnostic> {
    // The columns are chosen the way `pick` chooses them, and a `where` is
    // resolved into the list it matched for the same reason: a printed pipeline
    // should show what was chosen rather than the rule that chose it.
    let stacked: Vec<String> = if let Some(rule) = condition.as_ref() {
        let chosen = columns_matching(rule, schema)?;
        if chosen.is_empty() {
            return Err(Diagnostic::illegal(
                format!(
                    "no column's name matches that, so there would be nothing to stack. The table has: {}",
                    list(&schema.names())
                ),
                span,
            ));
        }
        *names = chosen.iter().map(|text| Name { text: text.clone(), span }).collect();
        *all_but = false;
        *condition = None;
        chosen
    } else {
        for name in names.iter() {
            known(name, schema)?;
        }
        let listed: Vec<String> = names.iter().map(|n| n.text.clone()).collect();
        if *all_but {
            schema.names().into_iter().filter(|n| !listed.contains(n)).collect()
        } else {
            listed
        }
    };

    if stacked.is_empty() {
        return Err(Diagnostic::illegal(
            "this leaves no columns to stack, so the table would not change. Name the columns that become rows: `lengthen [q1, q2, q3]`",
            span,
        ));
    }

    // **The stacked columns have to agree in type**, because the one column they
    // become holds one kind of thing. This is the fourth place the vocabulary
    // applies that rule, after `join`'s keys, `fill_missing`'s filler and
    // `first_present`'s arguments.
    let mut settled: Option<(String, Type)> = None;
    for column in &stacked {
        let kind = schema.get(column).unwrap_or(Type::Unknown);
        if kind == Type::Unknown {
            continue;
        }
        match &settled {
            None => settled = Some((column.clone(), kind)),
            Some((first, agreed)) if !agreed.agrees_with(kind) => {
                return Err(Diagnostic::illegal(
                    format!(
                        "`[{first}]` is {} and `[{column}]` is {}, so stacking them would put two kinds of thing in one column. Convert one of them first, or lengthen them separately",
                        agreed.name(),
                        kind.name()
                    ),
                    span,
                ));
            }
            _ => {}
        }
    }
    let holds = settled.map(|(_, k)| k).unwrap_or(Type::Unknown);

    // Every stacked column read apart into its pieces.
    let mut read = Vec::with_capacity(stacked.len());
    for column in &stacked {
        let Some(pieces) = pattern.read(column) else {
            return Err(Diagnostic::illegal(
                format!(
                    "`{}` does not look like `{}`, so there is no way to say which piece of it is which. Every column being stacked has to have the same shape of name",
                    column,
                    pattern.text()
                ),
                pattern.span,
            ));
        };
        read.push((column.clone(), pieces));
    }

    let name_columns: Vec<String> =
        pattern.named_parts().into_iter().map(|s| s.to_string()).collect();

    // **`{value}` is the one thing that makes several value columns**, and it is
    // grouped here: the pieces that are not the value piece say which output row
    // a column belongs to, and the value piece says which column of that row.
    let value_at = pattern.parts.iter().position(|p| *p == PatternPart::Value);
    let (value_columns, rows) = match value_at {
        None => {
            let held = value.as_ref().map(|n| n.text.clone()).unwrap_or_else(|| "value".into());
            let rows = read
                .iter()
                .map(|(column, pieces)| LengthenRow {
                    labels: pieces.clone(),
                    sources: vec![column.clone()],
                })
                .collect();
            (vec![held], rows)
        }
        Some(at) => {
            let mut value_columns: Vec<String> = Vec::new();
            let mut groups: Vec<(Vec<String>, Vec<(String, String)>)> = Vec::new();
            for (column, pieces) in &read {
                let held = pieces[at].clone();
                if !value_columns.contains(&held) {
                    value_columns.push(held.clone());
                }
                let labels: Vec<String> = pieces
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != at)
                    .map(|(_, p)| p.clone())
                    .collect();
                match groups.iter_mut().find(|(l, _)| *l == labels) {
                    Some((_, found)) => found.push((held, column.clone())),
                    None => groups.push((labels, vec![(held, column.clone())])),
                }
            }
            // A group missing one of the value columns is a hole, and filling it
            // silently is what `add_rows` already refuses for the same reason:
            // the two sides differ either by mistake or by decision, and only
            // the caller knows which.
            let mut rows = Vec::with_capacity(groups.len());
            for (labels, found) in groups {
                let mut sources = Vec::with_capacity(value_columns.len());
                for held in &value_columns {
                    let Some((_, column)) = found.iter().find(|(h, _)| h == held) else {
                        return Err(Diagnostic::illegal(
                            format!(
                                "there is no column for `{}` where the rest of the name is `{}`, so that row would have a hole in it. Every combination has to be there, or the ones that are not have to be left out of the pattern",
                                held,
                                labels.join(", ")
                            ),
                            pattern.span,
                        ));
                    };
                    sources.push(column.clone());
                }
                rows.push(LengthenRow { labels, sources });
            }
            (value_columns, rows)
        }
    };

    // What did not get stacked, in the order the table had it.
    let keep: Vec<String> =
        schema.names().into_iter().filter(|n| !stacked.contains(n)).collect();

    // **Two columns of one name is a defect rather than a policy**, which is why
    // tidyr's `names_repair` has nothing to answer here. It is also the case the
    // defaults walk into: a table that already has a column called `name` is
    // exactly the table `lengthen [a, b]` would give two of.
    for made in name_columns.iter().chain(value_columns.iter()) {
        if keep.contains(made) {
            return Err(Diagnostic::illegal(
                format!(
                    "the table already has a column called `{made}`, and this would make a second one. Name them: `lengthen ... as name [question], value [answer]`"
                ),
                span,
            ));
        }
    }
    for (i, made) in name_columns.iter().chain(value_columns.iter()).enumerate() {
        let all: Vec<&String> = name_columns.iter().chain(value_columns.iter()).collect();
        if all[..i].contains(&made) {
            return Err(Diagnostic::illegal(
                format!("`{made}` is named twice here, and two columns cannot share one name"),
                span,
            ));
        }
    }

    let mut columns: Vec<(String, Type)> =
        keep.iter().map(|n| (n.clone(), schema.get(n).unwrap_or(Type::Unknown))).collect();
    // A column's name is text, which is what makes `lower(name) starts "q"` work
    // on it in the next step as readily as on any other text column.
    columns.extend(name_columns.iter().map(|n| (n.clone(), Type::Text)));
    columns.extend(value_columns.iter().map(|n| (n.clone(), holds)));

    *resolved = Some(Lengthened { keep, name_columns, value_columns, rows });
    Ok(Schema { columns })
}

/// What `widen` refuses, what it assumes, and the one thing it cannot know.
///
/// **The output columns come from the data, which nothing else in the grammar
/// does.** So a bare `widen` is terminal and a `widen` that means to carry on
/// says what it produces. That is settled in §4.5.5, and the refusal below is
/// where it is enforced.
fn check_widen(
    pattern: &Pattern,
    value: &Expr,
    by: &mut Vec<Name>,
    missing: &Option<Expr>,
    giving: &[Name],
    span: Span,
    schema: &Schema,
    assumptions: &mut Vec<Diagnostic>,
) -> Result<Schema, Diagnostic> {
    // The pieces of the pattern are columns being read here, rather than columns
    // being made, which is the whole difference between the two verbs.
    // **Both columns default to the words `lengthen` makes**, so a table this
    // verb has nothing to read is nearly always one that was never lengthened.
    // Saying that is more use than reporting a missing column, because the
    // reader did not write the name that is missing.
    let untold = |text: &str, span: Span| {
        Diagnostic::illegal(
            format!(
                "`widen` reads the new column names out of `[{text}]` unless it is told otherwise, and this table has no such column. Say where they come from: `widen name [question], value [answer]`. The table has: {}",
                list(&schema.names())
            ),
            span,
        )
    };

    let mut from = Vec::with_capacity(pattern.parts.len());
    for part in &pattern.parts {
        if let PatternPart::Named(text) = part {
            if !pattern.quoted && text == "name" && schema.get(text).is_none() {
                return Err(untold("name", pattern.span));
            }
            known(&Name { text: text.clone(), span: pattern.span }, schema)?;
            from.push(text.clone());
        }
    }

    if let Expr::Column(only) = value {
        if only.text == "value" && schema.get("value").is_none() {
            return Err(untold("value", only.span));
        }
    }
    let holds = check_expr(value, schema)?;
    if value.windows() {
        return Err(Diagnostic::illegal(
            "a place is worked out over the rows that are left, so it cannot be what fills a cell. Make it a column first: `then add [place] as rank([x] descending)`, then widen from that",
            value.span(),
        ));
    }

    // **Which columns identify a row.** Left out it is everything not named
    // above, which is what tidyr does and is tidyr's footgun: one stray column
    // makes every row unique and the answer comes back as tall as it went in.
    // So the assumption is reported rather than made quietly, exactly as
    // `join` reports the key it worked out.
    let mut used = from.clone();
    value.walk(&mut |e| {
        if let Expr::Column(n) = e {
            used.push(n.text.clone());
        }
    });
    if by.is_empty() {
        let rest: Vec<String> =
            schema.names().into_iter().filter(|n| !used.contains(n)).collect();
        if rest.is_empty() {
            return Err(Diagnostic::illegal(
                "every column here is either the one holding the names or the one holding the values, so there is nothing left to say which rows go together and the answer would be one row. Say what identifies a row: `by [student]`",
                span,
            ));
        }
        assumptions.push(Diagnostic::assumption(
            format!(
                "`by` was not given, so the rows that go together are the ones agreeing on every other column: {}. Say it outright if that is not what you meant: `by [student]`",
                list(&rest)
            ),
            span,
        ));
        *by = rest.into_iter().map(|text| Name { text, span }).collect();
    } else {
        for name in by.iter() {
            known(name, schema)?;
        }
        for name in by.iter() {
            if used.contains(&name.text) {
                return Err(Diagnostic::illegal(
                    format!(
                        "`[{}]` is already being used for the names or the values, so it cannot also say which rows go together",
                        name.text
                    ),
                    name.span,
                ));
            }
        }
    }

    if let Some(filler) = missing {
        // **Saying what an empty cell holds needs the grammar to know which
        // cells there are**, and where nothing was declared it does not: the
        // columns come from the data and only the engine ever names them. So
        // this is one clause requiring another rather than a rule of its own.
        if giving.is_empty() {
            return Err(Diagnostic::illegal(
                "to say what an empty cell holds, the grammar has to know which cells there are, and these column names come from the data. Say what this makes: `giving [q1, q2, q3]`",
                filler.span(),
            ));
        }
        let kind = check_expr(filler, schema)?;
        if !kind.agrees_with(holds) {
            return Err(Diagnostic::illegal(
                format!(
                    "the cells hold {} and this fills the empty ones with {}, which would put two kinds of thing in one column. Fill them with {} instead",
                    holds.name(),
                    kind.name(),
                    holds.name()
                ),
                filler.span(),
            ));
        }
        if filler.aggregates() || filler.windows() {
            return Err(Diagnostic::illegal(
                "`missing` fills one cell at a time, so it cannot be a value that spans a group",
                filler.span(),
            ));
        }
    }

    // **A declared column has to be one this pattern could produce.** Reading it
    // back through the pattern is the same code that reads a column apart in
    // `lengthen`, which is what keeps the two directions one idea.
    for made in giving {
        if pattern.read(&made.text).is_none() {
            return Err(Diagnostic::illegal(
                format!(
                    "`{}` does not look like `{}`, so this could never make a column of that name",
                    made.text,
                    pattern.text()
                ),
                made.span,
            ));
        }
        if by.iter().any(|n| n.text == made.text) {
            return Err(Diagnostic::illegal(
                format!(
                    "the table already has a column called `{}`, and this would make a second one",
                    made.text
                ),
                made.span,
            ));
        }
    }

    let mut columns: Vec<(String, Type)> =
        by.iter().map(|n| (n.text.clone(), schema.get(&n.text).unwrap_or(Type::Unknown))).collect();
    // Where nothing was declared, these are the only columns the grammar can
    // name, and no step may follow — so a partial answer here misleads nobody.
    // The refusal that enforces that is in `check_tables`, which is the only
    // place that can see whether anything comes next.
    columns.extend(giving.iter().map(|n| (n.text.clone(), holds)));
    Ok(Schema { columns })
}

/// What `join` refuses, and the one thing it assumes.
///
/// The answers here are §4.4's, decided before the verb was written. What it
/// cannot answer is a duplicate key multiplying rows, because the checker is
/// handed column names and never sees a row; that is recorded as owed rather
/// than quietly dropped.
fn check_join(
    schema: &Schema,
    other: &Name,
    by: &[JoinKey],
    _unmatched: Unmatched,
    span: Span,
    others: &Tables,
    assumptions: &mut Vec<Diagnostic>,
) -> Result<(Schema, Vec<JoinKey>), Diagnostic> {
    let Some(right) = others.get(&other.text) else {
        let known = others.names();
        let suggestion = nearest(&other.text, known.iter().map(|s| s.as_str()))
            .map(|s| format!(" Did you mean `{s}`?"))
            .unwrap_or_default();
        let have = if known.is_empty() {
            "no other table was described".to_string()
        } else {
            format!("the tables described are: {}", list(&known))
        };
        return Err(Diagnostic::illegal(
            format!(
                "this joins a table called `{}`, and {have}.{suggestion}",
                other.text
            ),
            other.span,
        ));
    };

    // The key. Given, or worked out from the names both tables share, which is
    // never fatal and never silent (§10).
    let keys: Vec<JoinKey> = if by.is_empty() {
        let shared: Vec<String> = schema
            .names()
            .into_iter()
            .filter(|n| right.get(n).is_some())
            .collect();
        if shared.is_empty() {
            return Err(Diagnostic::illegal(
                format!(
                    "`join` needs to know which columns say that two rows correspond, and these tables share no column name. Write it: `join {} by [id]`, or `join {} by [customer_id] is [id]` where each table names it differently",
                    other.text, other.text
                ),
                span,
            ));
        }
        assumptions.push(Diagnostic::assumption(
            format!(
                "`join` matched on {}, the column names both tables share. Say it to be sure: `join {} by [{}]`",
                list(&shared),
                other.text,
                shared.join(", ")
            ),
            span,
        ));
        shared.into_iter().map(|text| JoinKey::same(Name { text, span })).collect()
    } else {
        by.to_vec()
    };

    keys_agree(schema, right, &keys, other)?;

    // A column on both tables that is not a key would arrive twice. Suffixing it
    // to `name_x` and `name_y` is how pandas hands back a column nobody asked
    // for; refusing names the two tables and the fix.
    //
    // **A differently-named key is exempt on the other table's side only**, and
    // getting that wrong is how the feature would break the join it was built
    // for: `orders.customer_id` against `customers.id` leaves `id` on the other
    // table, which is not a clash because it never arrives — it holds the key's
    // own value and is dropped the way a same-named key is.
    let clashes: Vec<String> = right
        .names()
        .into_iter()
        .filter(|n| schema.get(n).is_some() && !keys.iter().any(|k| &k.other.text == n))
        .collect();
    if !clashes.is_empty() {
        let quoted: Vec<String> = clashes.iter().map(|c| format!("`{c}`")).collect();
        return Err(Diagnostic::illegal(
            format!(
                "both tables have {}, and a join would bring back two columns of that name. Rename one first, or drop it: `then pick all_but [{}]`",
                list(&quoted),
                clashes.join(", ")
            ),
            span,
        ));
    }

    // This table's columns, then the other's, minus the keys, which are already
    // here and hold the same values by construction.
    //
    // **The surviving name is this table's**, which is the choice a
    // differently-named key forces and the one every neighbour makes: the
    // pipeline goes on being about `orders`, so `customer_id` is the word the
    // rest of the sentence is written in. The other table's `id` is dropped.
    let mut columns = schema.columns.clone();
    for (name, kind) in &right.columns {
        if !keys.iter().any(|k| &k.other.text == name) {
            columns.push((name.clone(), *kind));
        }
    }
    Ok((Schema { columns }, keys))
}

/// Every key is on both tables and means the same kind of thing on each.
///
/// A text id matched against a number id can never match, which is the rule
/// every comparison in the grammar already follows. Shared by `join` and
/// `matching` because the question is the same one: two tables, and the columns
/// that are supposed to say which rows correspond.
fn keys_agree(
    schema: &Schema,
    right: &Schema,
    keys: &[JoinKey],
    other: &Name,
) -> Result<(), Diagnostic> {
    for key in keys {
        let mine = known(&key.this, schema)?;
        let Some(theirs) = right.get(&key.other.text) else {
            let names = right.names();
            let suggestion = nearest(&key.other.text, names.iter().map(|s| s.as_str()))
                .map(|s| format!(" Did you mean `{s}`?"))
                .unwrap_or_default();
            // **The nudge differs by which shape was written.** Where the two
            // sides were named separately the writer already knows they can
            // differ, so repeating that would be noise; where one name was
            // written for both, not knowing the pair form is the likeliest
            // reason to be here at all.
            let pair = if key.is_same() {
                format!(
                    " If `{}` calls it something else, say both: `by [{}] is [their_name]`.",
                    other.text, key.this.text
                )
            } else {
                String::new()
            };
            return Err(Diagnostic::illegal(
                format!(
                    "`{}` has no column called `{}`.{suggestion}{pair} It has: {}",
                    other.text,
                    key.other.text,
                    list(&names)
                ),
                key.other.span,
            ));
        };
        if !mine.agrees_with(theirs) {
            // Named the way it was written, so the message points at the two
            // columns the reader can see rather than at one name they would
            // then have to map back to two.
            let named = if key.is_same() {
                format!("`[{}]`", key.this.text)
            } else {
                format!("`[{}]` against `[{}]`", key.this.text, key.other.text)
            };
            return Err(Diagnostic::illegal(
                format!(
                    "{named} is {} here and {} in `{}`, so the two can never match. Convert one of them first",
                    mine.name(),
                    theirs.name(),
                    other.text
                ),
                key.this.span,
            ));
        }
    }
    Ok(())
}

/// What `matching` refuses, and the one thing it assumes.
///
/// **It is a filtering join and it changes no columns**, so unlike `join` there
/// is no schema to work out and no clash to report: a column both tables have is
/// simply not a problem when only one table's rows come back.
///
/// **It also cannot multiply rows, which is the guarantee `join` could not
/// make.** Whether the other table has one partner for a key or fifty, the
/// answer here is yes or no, so the row count can only go down. That is the
/// reason to spell a semi join this way rather than as a join with a `pick`
/// after it, and it is worth knowing when choosing between the two.
fn check_matching(
    schema: &Schema,
    other: &Name,
    by: &[JoinKey],
    span: Span,
    others: &Tables,
    assumptions: &mut Vec<Diagnostic>,
) -> Result<Vec<JoinKey>, Diagnostic> {
    let Some(right) = others.get(&other.text) else {
        let known = others.names();
        let suggestion = nearest(&other.text, known.iter().map(|s| s.as_str()))
            .map(|s| format!(" Did you mean `{s}`?"))
            .unwrap_or_default();
        let have = if known.is_empty() {
            "no other table was described".to_string()
        } else {
            format!("the tables described are: {}", list(&known))
        };
        return Err(Diagnostic::illegal(
            format!(
                "this asks for rows matching a table called `{}`, and {have}.{suggestion}",
                other.text
            ),
            other.span,
        ));
    };

    let keys: Vec<JoinKey> = if by.is_empty() {
        let shared: Vec<String> = schema
            .names()
            .into_iter()
            .filter(|n| right.get(n).is_some())
            .collect();
        if shared.is_empty() {
            return Err(Diagnostic::illegal(
                format!(
                    "`matching` needs to know which columns say that two rows correspond, and these tables share no column name. Write it: `matching({}, by [id])`",
                    other.text
                ),
                span,
            ));
        }
        assumptions.push(Diagnostic::assumption(
            format!(
                "`matching` used {}, the column names both tables share. Say it to be sure: `matching({}, by [{}])`",
                list(&shared),
                other.text,
                shared.join(", ")
            ),
            span,
        ));
        shared.into_iter().map(|text| JoinKey::same(Name { text, span })).collect()
    } else {
        by.to_vec()
    };

    keys_agree(schema, right, &keys, other)?;
    Ok(keys)
}

/// The `matching` standing as a whole condition, through a `not` if there is one.
///
/// **A filtering join is a table operation in every host underneath** — dplyr
/// spells it `semi_join` and `anti_join`, polars spells it `join(how = "semi")`,
/// pandas has no single call for it at all. SQL's `EXISTS` is the outlier in
/// being an ordinary condition that can sit inside `and` and `or`.
///
/// So `matching` stands alone or negated, and nowhere else. Allowing it to
/// compose would mean one backend rendering a structure the others cannot, which
/// is the disagreement between hosts this project exists to refuse. The
/// restriction can be lifted the day every backend can carry it; a refusal is
/// always safe to relax later, and never safe to tighten.
fn filtering_join(condition: &mut Expr) -> Option<&mut Expr> {
    if matches!(condition, Expr::Matching { .. }) {
        return Some(condition);
    }
    if let Expr::Not { inner, .. } = condition {
        if matches!(**inner, Expr::Matching { .. }) {
            return Some(inner);
        }
    }
    None
}

/// A value in one step reaching for a column another value in that same step makes.
///
/// **Every value in a step is worked out from the table as it arrives**, so the
/// second cannot see the first. That is the same rule SQL's `SELECT` has, and
/// polars' `with_columns`, and it is what makes a step one step rather than a
/// sequence hiding inside one.
///
/// It needs its own message because **dplyr's `mutate` and pandas' `assign` both
/// allow it**, so this is a shape people arrive already expecting, from either
/// language. Left to the general message they would be told there is no column
/// called `margin` while looking straight at the line where they write `margin`,
/// and would go hunting for a typo that is not there. Naming the real rule and
/// the spelling that works is the difference between a refusal and an obstacle.
fn made_here_is_not_visible(
    values: &[Named],
    schema: &Schema,
    verb: &str,
) -> Result<(), Diagnostic> {
    let made: Vec<&str> = values.iter().map(|v| v.name.text.as_str()).collect();

    for Named { value, .. } in values {
        let mut found: Option<(String, Span)> = None;
        value.walk(&mut |e| {
            if let Expr::Column(n) = e {
                // A name already on the table is visible however this step uses
                // it: `add [revenue] as [revenue] * 2` reads the old value and
                // replaces it, which is the ordinary case and not this one.
                if found.is_none()
                    && schema.get(&n.text).is_none()
                    && made.contains(&n.text.as_str())
                {
                    found = Some((n.text.clone(), n.span));
                }
            }
        });

        if let Some((name, span)) = found {
            return Err(Diagnostic::illegal(
                format!(
                    "`[{name}]` is made by this same `{verb}`, so it is not on the table yet. Every value in one step is worked out from the table as it arrives. Make it in a step of its own: `then {verb} [{name}] as ... then {verb} ...`"
                ),
                span,
            ));
        }
    }
    Ok(())
}

/// What `add_combinations` refuses.
///
/// **The schema never changes**, which is the whole reason this verb needs no
/// clause for what a new row holds: every column is still there and still
/// nameable, so `fill_missing` can be written after it.
///
/// **One refusal here is a real rule rather than a guard against nonsense.**
/// Fewer than two columns cannot make a combination that is not already in the
/// table, whatever `by` says: the distinct values of one column, crossed with
/// nothing, are the values that column already has. So the sentence would be
/// answered by handing the table straight back, and a step that cannot do
/// anything is better refused than silently obeyed (§10 — say so rather than
/// write something close).
fn check_add_combinations(
    names: &[Name],
    by: &[Name],
    span: Span,
    schema: &Schema,
) -> Result<Schema, Diagnostic> {
    for name in names.iter().chain(by.iter()) {
        known(name, schema)?;
    }

    if names.len() < 2 {
        let one = names.first().map(|n| n.text.clone());
        let hint = match &one {
            Some(text) => format!(
                "`add_combinations [{text}, ...]` needs a second column to cross `{text}` against"
            ),
            None => "`add_combinations [region, product]`".to_string(),
        };
        return Err(Diagnostic::illegal(
            format!(
                "`add_combinations` crosses two columns or more, and this names {}. One column on its own has no combinations to make — its distinct values are already the values it holds — so the table would come back unchanged. {hint}",
                match names.len() {
                    0 => "none".to_string(),
                    _ => "one".to_string(),
                }
            ),
            span,
        ));
    }

    // A column cannot be both crossed and held fixed. Left to the query it
    // would join a group against itself and the answer would be the table
    // unchanged, which is the same nothing the rule above refuses.
    for name in names.iter() {
        if let Some(held) = by.iter().find(|b| b.text == name.text) {
            let _ = held;
            return Err(Diagnostic::illegal(
                format!(
                    "`[{}]` is being crossed and held fixed at once, and it can only be one. Take it out of `by` to cross it, or out of the brackets to keep it as the group",
                    name.text
                ),
                name.span,
            ));
        }
    }

    repeated_column(names, "the columns being crossed")?;
    repeated_column(by, "`by`")?;

    Ok(schema.clone())
}

/// One column named twice in one list.
fn repeated_column(names: &[Name], where_: &str) -> Result<(), Diagnostic> {
    for (i, a) in names.iter().enumerate() {
        if names[..i].iter().any(|b| b.text == a.text) {
            return Err(Diagnostic::illegal(
                format!("`[{}]` is named twice in {where_}", a.text),
                a.span,
            ));
        }
    }
    Ok(())
}

/// A column the caller named that the table does not have.
///
/// The message names the nearest real column when there is one, and lists what
/// is actually there either way, because the list is what someone needs when the
/// name they wrote was not a typo but a memory of a different table.
fn known(name: &Name, schema: &Schema) -> Result<Type, Diagnostic> {
    if let Some(kind) = schema.get(&name.text) {
        return Ok(kind);
    }
    let names = schema.names();
    let suggestion = nearest(&name.text, names.iter().map(|s| s.as_str()))
        .map(|s| format!(" Did you mean `{s}`?"))
        .unwrap_or_default();
    Err(Diagnostic::illegal(
        format!(
            "there is no column called `{}`.{suggestion} The table has: {}",
            name.text,
            list(&names)
        ),
        name.span,
    ))
}

fn duplicate_names(values: &[Named]) -> Result<(), Diagnostic> {
    for (i, a) in values.iter().enumerate() {
        if let Some(b) = values[..i].iter().find(|b| b.name.text == a.name.text) {
            let _ = b;
            return Err(Diagnostic::illegal(
                format!(
                    "`[{}]` is made twice in one step, so one of the two would be thrown away. Give them different names, or make the second one in a step of its own",
                    a.name.text
                ),
                a.name.span,
            ));
        }
    }
    Ok(())
}

fn check_expr(expr: &Expr, schema: &Schema) -> Result<Type, Diagnostic> {
    match expr {
        Expr::Column(name) => known(name, schema),
        Expr::Text { .. } => Ok(Type::Text),
        Expr::Whole { .. } | Expr::Decimal { .. } => Ok(Type::Number),
        Expr::Truth { .. } => Ok(Type::Truth),
        Expr::Missing { .. } => Ok(Type::Unknown),

        // **Reaching here means it was written somewhere it is not expanded**,
        // which is every seat but the whole condition of a `keep`. It asks one
        // question of many columns and answers yes or no, so it is a condition
        // and nothing else — the same restriction `matching` has, for a
        // related reason.
        Expr::Quantified { every, span, .. } => Err(Diagnostic::illegal(
            format!(
                "`{}` asks a question of many columns, so it is the whole question a `keep` asks and cannot stand here: `keep where {} name starts \"q\" as value > 3`",
                if *every { "every" } else { "any" },
                if *every { "every" } else { "any" }
            ),
            *span,
        )),

        // **Every test has to be a question and every value has to hold the same
        // kind of thing**, because they all end up in one column. Both are rules
        // the grammar already applies elsewhere: `keep` asks the first of a
        // condition, and `join`'s keys, `fill_missing`'s filler, `first_present`
        // and `lengthen` all ask the second.
        Expr::When { arms, otherwise, span } => {
            for (test, _) in arms.iter() {
                let kind = check_expr(test, schema)?;
                if !kind.agrees_with(Type::Truth) {
                    return Err(Diagnostic::illegal(
                        format!(
                            "each thing `when` tests has to be a question that is either yes or no, and this is {}. Compare it to something: `is`, `>`, `<`, or `in {{...}}`",
                            kind.name()
                        ),
                        test.span(),
                    ));
                }
            }

            let values = arms
                .iter()
                .map(|(_, v)| v)
                .chain(otherwise.iter().map(|e| e.as_ref()));
            let mut settled: Option<(Type, Span)> = None;
            for value in values {
                let kind = check_expr(value, schema)?;
                if kind == Type::Unknown {
                    continue;
                }
                match settled {
                    None => settled = Some((kind, value.span())),
                    Some((agreed, _)) if !agreed.agrees_with(kind) => {
                        return Err(Diagnostic::illegal(
                            format!(
                                "`when` gives one column, so all of its answers have to be the same kind of thing. One of them is {} and this is {}",
                                agreed.name(),
                                kind.name()
                            ),
                            value.span(),
                        ));
                    }
                    _ => {}
                }
            }

            if arms.is_empty() {
                return Err(Diagnostic::illegal(
                    "`when` needs at least one question and the answer that goes with it: `when([score] >= 90, \"A\", otherwise \"C\")`",
                    *span,
                ));
            }
            Ok(settled.map(|(k, _)| k).unwrap_or(Type::Unknown))
        }

        Expr::Arithmetic { left, right, op, span } => {
            let l = check_expr(left, schema)?;
            let r = check_expr(right, schema)?;
            for (kind, side) in [(l, left.as_ref()), (r, right.as_ref())] {
                if !kind.agrees_with(Type::Number) {
                    return Err(Diagnostic::illegal(
                        format!("`{op}` works on numbers, and this is {}. Convert it first, or compare it instead: `is`, `>`, `<`", kind.name()),
                        side.span(),
                    ));
                }
            }
            let _ = span;
            Ok(Type::Number)
        }

        Expr::Compare { left, right, span, .. } => {
            let l = check_expr(left, schema)?;
            let r = check_expr(right, schema)?;
            if !l.agrees_with(r) {
                return Err(Diagnostic::illegal(
                    format!(
                        "this compares {} with {}, which can never match. Convert one of them first",
                        l.name(),
                        r.name()
                    ),
                    *span,
                ));
            }
            Ok(Type::Truth)
        }

        Expr::Logic { left, right, op, .. } => {
            let word = match op {
                Logic::And => "and",
                Logic::Or => "or",
            };
            for side in [left.as_ref(), right.as_ref()] {
                let kind = check_expr(side, schema)?;
                if !kind.agrees_with(Type::Truth) {
                    return Err(Diagnostic::illegal(
                        format!("`{word}` joins two questions, and this is {}. Make it a question first: `[column] is \"value\"`, or `[column] > 10`", kind.name()),
                        side.span(),
                    ));
                }
            }
            Ok(Type::Truth)
        }

        Expr::Not { inner, .. } => {
            let kind = check_expr(inner, schema)?;
            if !kind.agrees_with(Type::Truth) {
                return Err(Diagnostic::illegal(
                    format!("`not` turns a question around, and this is {}. Make it a question first: `not ([column] is \"value\")`", kind.name()),
                    inner.span(),
                ));
            }
            Ok(Type::Truth)
        }

        Expr::In { left, set, span, .. } => {
            let l = check_expr(left, schema)?;
            for value in set {
                let kind = check_expr(value, schema)?;
                if !kind.agrees_with(l) {
                    return Err(Diagnostic::illegal(
                        format!(
                            "this set holds {} while the column is {}, so nothing in it could ever match. Write the values as {} instead",
                            kind.name(),
                            l.name(),
                            l.name()
                        ),
                        value.span(),
                    ));
                }
            }
            let _ = span;
            Ok(Type::Truth)
        }

        // Both sides are text, because asking whether a number begins with
        // something is asking about how it happens to be printed.
        Expr::TextTest { op, left, value, span } => {
            for (side, what) in [(left.as_ref(), "this"), (value.as_ref(), "the value")] {
                let kind = check_expr(side, schema)?;
                if !kind.agrees_with(Type::Text) {
                    return Err(Diagnostic::illegal(
                        format!(
                            "`{}` compares text with text, and {what} is {}. Convert it first with `to_text(...)`",
                            op.word(),
                            kind.name()
                        ),
                        side.span(),
                    ));
                }
            }
            let _ = span;
            Ok(Type::Truth)
        }

        // Reaching here means `kind` is outside a `where` that chooses columns,
        // because those resolve the whole condition before this runs.
        Expr::ColumnKind { span } => Err(Diagnostic::illegal(
            "`kind` means what a column holds, and the `where` that chooses columns is the one place that asks. Write it as `pick where kind is \"number\"`",
            *span,
        )),

        // Reaching here means `value` is outside an `add where` or
        // `summarize where`, because those expand it before this runs.
        Expr::ColumnValue { span } => Err(Diagnostic::illegal(
            "`value` means the column being worked on, and only `add where` and `summarize where` work on a column at a time. Name the column you mean, in brackets",
            *span,
        )),

        // Reaching here means `name` is somewhere other than a `pick where`,
        // because that case resolves the whole condition before this runs.
        Expr::ColumnName { span } => Err(Diagnostic::illegal(
            "`name` means the name of a column, and `pick where` is the one place that asks about a name. To test what is *in* a column, write it in brackets: `[name]`",
            *span,
        )),

        // A window is a number: a place, or a position. What it may be *next to*
        // is decided by the step it sits in, not here.
        Expr::Window { key, .. } => {
            if let Some(k) = key {
                known(&k.column, schema)?;
            }
            Ok(Type::Number)
        }

        // Reaching here means `matching` is somewhere other than standing as a
        // whole `keep` condition, because the Keep arm peels that case off first.
        Expr::Matching { other, span, .. } => Err(Diagnostic::illegal(
            format!(
                "`matching` chooses whole rows, so it is the whole question `keep` asks rather than one part of it. Ask it in its own step: `then keep where matching({}, by [id]) then keep where ...`",
                other.text
            ),
            *span,
        )),

        Expr::IsMissing { inner, .. } => {
            check_expr(inner, schema)?;
            Ok(Type::Truth)
        }

        // `look_up([code], "W", "West", …, otherwise [code])` — the lookup
        // table. The pairs are written values on both sides, because the word
        // means a table: a computed test or a computed answer is `when`'s job,
        // and the refusals say so. `otherwise` is any ordinary value — the
        // column itself is the canonical one — so keeping, dropping and
        // defaulting are all the same sentence with a different ending.
        Expr::Lookup { subject, pairs, otherwise, .. } => {
            let subject_kind = check_expr(subject, schema)?;

            // The key of a literal, for spotting the same value written twice.
            // Only the shapes admitted below ever reach it.
            let key_of = |e: &Expr| match e {
                Expr::Text { value, .. } => format!("\"{value}\""),
                Expr::Whole { value, .. } => value.to_string(),
                Expr::Decimal { value, .. } => value.to_string(),
                Expr::Truth { value, .. } => if *value { "yes" } else { "no" }.to_string(),
                _ => unreachable!("refused before the key is taken"),
            };

            let mut seen: Vec<String> = Vec::new();
            let mut becomes: Option<Type> = None;
            for (from, to) in pairs {
                match from {
                    Expr::Text { .. }
                    | Expr::Whole { .. }
                    | Expr::Decimal { .. }
                    | Expr::Truth { .. } => {}
                    Expr::Missing { span } => {
                        return Err(Diagnostic::illegal(
                            "a missing value never equals anything, so it cannot be looked up. `fill_missing` is the word for filling holes, and `first_present` chooses around one",
                            *span,
                        ));
                    }
                    other => {
                        return Err(Diagnostic::illegal(
                            "`look_up` pairs written values, so each value looked up is written out. To test anything computed, `when` is the word: `when([x] > 3, \"high\", otherwise \"low\")`",
                            other.span(),
                        ));
                    }
                }
                let from_kind = check_expr(from, schema)?;
                if !from_kind.agrees_with(subject_kind) {
                    return Err(Diagnostic::illegal(
                        format!(
                            "this looks up {} in a column holding {}, so it could never match. Write the value as {}",
                            from_kind.name(),
                            subject_kind.name(),
                            subject_kind.name()
                        ),
                        from.span(),
                    ));
                }
                let key = key_of(from);
                if seen.contains(&key) {
                    return Err(Diagnostic::illegal(
                        format!("`{key}` is looked up twice, and the second answer could never be reached. Name each value once"),
                        from.span(),
                    ));
                }
                seen.push(key);

                match to {
                    Expr::Text { .. }
                    | Expr::Whole { .. }
                    | Expr::Decimal { .. }
                    | Expr::Truth { .. }
                    | Expr::Missing { .. } => {}
                    other => {
                        return Err(Diagnostic::illegal(
                            "what a value becomes is written out too. For a computed answer, `when` is the word: `when([x] is \"W\", [region], otherwise ...)`",
                            other.span(),
                        ));
                    }
                }
                let to_kind = check_expr(to, schema)?;
                if to_kind != Type::Unknown {
                    match becomes {
                        None => becomes = Some(to_kind),
                        Some(agreed) if !agreed.agrees_with(to_kind) => {
                            return Err(Diagnostic::illegal(
                                format!(
                                    "`look_up` gives one column, so everything it gives has to hold the same kind of thing. One answer is {} and this is {}",
                                    agreed.name(),
                                    to_kind.name()
                                ),
                                to.span(),
                            ));
                        }
                        _ => {}
                    }
                }
            }

            // The fallback is an ordinary value seat, exactly as `when`'s
            // branches are: whatever may stand in one of those may stand here,
            // and the step's own rules govern the rest.
            let otherwise_kind = check_expr(otherwise, schema)?;
            if otherwise_kind != Type::Unknown {
                match becomes {
                    None => becomes = Some(otherwise_kind),
                    Some(agreed) if !agreed.agrees_with(otherwise_kind) => {
                        return Err(Diagnostic::illegal(
                            format!(
                                "`look_up` gives one column, and `otherwise` is {} where the answers are {}. Give the same kind of thing, or convert",
                                otherwise_kind.name(),
                                agreed.name()
                            ),
                            otherwise.span(),
                        ));
                    }
                    _ => {}
                }
            }

            Ok(becomes.unwrap_or(Type::Unknown))
        }

        // `rolling(average([x]), 7)` — an aggregate asked of the last n rows.
        //
        // **Six aggregates may stand inside it, and each exclusion has a
        // reason a message can say.** `first` derives, `last` is the row
        // itself, `row_count` answers the number that was written, and
        // `unique_count` is a sentence one engine cannot write — measured:
        // Spark refuses a distinct aggregate over any window frame, and a
        // sentence that runs on one engine and is refused by another is the
        // quiet disagreement §3.1 exists to end.
        Expr::Rolling { agg, agg_span, args, count, .. } => {
            let Some(function) = vocabulary::lookup(agg) else {
                let suggestion = nearest(agg, vocabulary::FUNCTIONS.iter().map(|f| f.name))
                    .map(|s| format!(" Did you mean `{s}`?"))
                    .unwrap_or_default();
                return Err(Diagnostic::illegal(
                    format!(
                        "there is no function called `{agg}`.{suggestion} The grammar has: {}",
                        vocabulary::FUNCTIONS
                            .iter()
                            .map(|f| f.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    *agg_span,
                ));
            };

            const ROLLING_AGGREGATES: &str =
                "`total`, `average`, `median`, `smallest`, `largest` and `standard_deviation`";
            match function.kind {
                vocabulary::Kind::Window => {
                    return Err(Diagnostic::illegal(
                        format!("`rolling` holds an aggregate — a value that spans rows — and `{agg}` is already a value worked out along them. The aggregates are {ROLLING_AGGREGATES}"),
                        *agg_span,
                    ));
                }
                vocabulary::Kind::Scalar => {
                    return Err(Diagnostic::illegal(
                        format!("`rolling` asks about a stretch of rows, and `{agg}` works on one value at a time. The aggregates are {ROLLING_AGGREGATES}"),
                        *agg_span,
                    ));
                }
                vocabulary::Kind::Aggregate => {}
            }

            match agg.as_str() {
                "first" => {
                    return Err(Diagnostic::illegal(
                        "the first value of the last few rows is a value from a fixed distance back, and `previous` already says that: `previous([x], 6)` is the first of a window of 7",
                        *agg_span,
                    ));
                }
                "last" => {
                    return Err(Diagnostic::illegal(
                        "the last value of the last few rows is the row's own value. Use the column",
                        *agg_span,
                    ));
                }
                "row_count" => {
                    return Err(Diagnostic::illegal(
                        "every full window holds the number of rows that was written, and a window that is not yet full answers `missing`, so counting them answers nothing the sentence did not already say",
                        *agg_span,
                    ));
                }
                "unique_count" => {
                    return Err(Diagnostic::illegal(
                        "no query can count distinct values over a moving window on every engine underneath, so the sentence would answer on one and be refused by another. `unique_count` works at full width in `summarize`",
                        *agg_span,
                    ));
                }
                _ => {}
            }

            if !function.arity.accepts(args.len()) {
                let wants = function.arity.wanted();
                return Err(Diagnostic::illegal(
                    format!(
                        "`{agg}` takes {wants}, and {} {} written",
                        args.len(),
                        if args.len() == 1 { "was" } else { "were" }
                    ),
                    *agg_span,
                ));
            }

            // **A plain column, the ruling an ordering position already has**:
            // the computed value is one `add` away, and a second spelling of
            // that `add` is what this refusal declines to be.
            let Some(Expr::Column(column)) = args.first() else {
                return Err(Diagnostic::illegal(
                    format!("`rolling` reads a column, and the computed value is an `add` away: `then add [v] as ... then add [answer] as rolling({agg}([v]), 7)`"),
                    args.first().map(Expr::span).unwrap_or(*agg_span),
                ));
            };
            let kind = known(column, schema)?;
            if !kind.agrees_with(Type::Number) {
                return Err(Diagnostic::illegal(
                    format!("`{agg}` works on numbers, and this column is {}. Convert the column first", kind.name()),
                    column.span,
                ));
            }

            // How many rows, judged the way `previous` judges how far: a plain
            // whole number, written out, with each wrong shape sent its own
            // sentence. A negative arrives as `0 - n` because the lexer has no
            // negative literal.
            let negated = matches!(
                count.as_ref(),
                Expr::Arithmetic { op: Arith::Subtract, left, right, .. }
                    if matches!(**left, Expr::Whole { value: 0, .. })
                        && matches!(**right, Expr::Whole { .. })
            );
            if negated {
                return Err(Diagnostic::illegal(
                    "the window reaches back from the row itself, so how many rows it holds cannot be negative",
                    count.span(),
                ));
            }
            let Expr::Whole { value, .. } = count.as_ref() else {
                return Err(Diagnostic::illegal(
                    "the second thing `rolling` takes is how many rows the window holds, written as a plain whole number: `rolling(average([revenue]), 7)`. It cannot be worked out per row",
                    count.span(),
                ));
            };
            if *value == 0 {
                return Err(Diagnostic::illegal(
                    "a window of no rows asks nothing. It holds the row itself and the rows above it, so the smallest one is 2",
                    count.span(),
                ));
            }
            if *value == 1 {
                return Err(Diagnostic::illegal(
                    "a window of one row is the row itself, so there is nothing rolling about it. Use the column, or widen the window",
                    count.span(),
                ));
            }

            Ok(Type::Number)
        }

        Expr::Call { name, args, span } => {
            let Some(function) = vocabulary::lookup(name) else {
                // **A word the grammar used to have gets its own sentence.**
                // The nearest-name guess cannot help here — `to_whole` is not a
                // near miss for anything — and the reader wrote a word that
                // worked last week, so what they need is which of the two
                // replaced it rather than a list of thirty-four to search.
                if name == "to_whole" {
                    return Err(Diagnostic::illegal(
                        "`to_whole` is now `round_below` and `round_above`, which say which way they go. It converted nothing — the grammar has one number type — so it was a rounding wearing a conversion's name, and nobody could tell from the name whether -5.5 came back as -5 or -6. `round_below(-5.5)` is -6 and `round_above(-5.5)` is -5. For the nearest whole number, `round_below([x] + 0.5)`",
                        *span,
                    ));
                }
                let suggestion = nearest(name, vocabulary::FUNCTIONS.iter().map(|f| f.name))
                    .map(|s| format!(" Did you mean `{s}`?"))
                    .unwrap_or_default();
                return Err(Diagnostic::illegal(
                    format!(
                        "there is no function called `{name}`.{suggestion} The grammar has: {}",
                        vocabulary::FUNCTIONS
                            .iter()
                            .map(|f| f.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    *span,
                ));
            };

            if !function.arity.accepts(args.len()) {
                let wants = function.arity.wanted();
                return Err(Diagnostic::illegal(
                    format!(
                        "`{name}` takes {wants}, and {} {} written",
                        args.len(),
                        if args.len() == 1 { "was" } else { "were" }
                    ),
                    *span,
                ));
            }

            // An aggregate inside an aggregate asks the same question twice and
            // has no meaning: the inner one has already collapsed the group.
            for arg in args {
                if arg.aggregates() {
                    return Err(Diagnostic::illegal(
                        format!("`{name}` is already asking about a whole group, so it cannot hold another value that does. Use the column itself: `{name}([column])`"),
                        arg.span(),
                    ));
                }
                // **A window inside a function's argument loses the order it
                // walks.** A function renders its arguments without knowing
                // where it stands, so a place worked out along the rows gets no
                // rows to be along — `total(rank([x]))` reached the engine as a
                // window nested in an aggregate, which no engine runs, and a
                // scalar around one dropped the `ORDER BY` and answered in
                // whatever order the rows happened to be. Both are the same
                // refusal: make it a column, then use the column.
                if arg.windows() {
                    return Err(Diagnostic::illegal(
                        format!("a value worked out along the rows cannot stand inside `{name}(...)`, because the order it walks does not reach it there. Make it a column first: `then add [t] as running_total([x]) then ... {name}([t])`"),
                        arg.span(),
                    ));
                }
            }

            let mut kinds = Vec::new();
            for arg in args {
                kinds.push(check_expr(arg, schema)?);
            }

            match name.as_str() {
                // These reduce whatever they are given to a number, so they need
                // numbers to reduce.
                "total" | "average" | "median" | "standard_deviation" => {
                    if !kinds[0].agrees_with(Type::Number) {
                        return Err(Diagnostic::illegal(
                            format!("`{name}` works on numbers, and this column is {}. Count the rows instead with `row_count()`, or convert the column first", kinds[0].name()),
                            args[0].span(),
                        ));
                    }
                    Ok(Type::Number)
                }
                // These pick one of the values they were given, so they hand back
                // whatever kind that was.
                "smallest" | "largest" | "first" | "last" => Ok(kinds[0]),
                "row_count" | "unique_count" => Ok(Type::Number),

                // **Every column it looks in has to hold the same kind of
                // thing**, because they are all candidates for one column and a
                // column holds one kind. This is the rule `join`'s keys and
                // `fill_missing`'s filler already follow.
                // Text in, text out. A number has no case, so asking for one is
                // a mistake worth naming rather than silently converting.
                "lower" | "upper" => {
                    if !kinds[0].agrees_with(Type::Text) {
                        return Err(Diagnostic::illegal(
                            format!("`{name}` changes the case of text, and this column is {}. Only text has a case", kinds[0].name()),
                            args[0].span(),
                        ));
                    }
                    Ok(Type::Text)
                }

                // **A conversion takes anything and says what it gives**, which
                // is the whole point of writing one: the reason to convert is
                // that what you have is not what you want.
                "to_number" => Ok(Type::Number),

                // Both make a whole number out of a number, so both want one.
                // Neither needs a note about which way it goes: the name says
                // it, which is the whole reason they replaced `to_whole`.
                "round_below" | "round_above" => {
                    if !kinds[0].agrees_with(Type::Number) {
                        return Err(Diagnostic::illegal(
                            format!("`{name}` moves a number to a whole one, and this is {}. Convert it first with `to_number`", kinds[0].name()),
                            args[0].span(),
                        ));
                    }
                    Ok(Type::Number)
                }
                "to_text" => Ok(Type::Text),
                "to_date" => Ok(Type::Date),

                // Text in, text out, and the refusal names the conversion,
                // because "this is a number" is only half of what the reader
                // needs.
                "trim" => {
                    if !kinds[0].agrees_with(Type::Text) {
                        return Err(Diagnostic::illegal(
                            format!("`trim` takes the spaces off text, and this is {}. Convert it first with `to_text(...)`", kinds[0].name()),
                            args[0].span(),
                        ));
                    }
                    Ok(Type::Text)
                }
                "characters" => {
                    if !kinds[0].agrees_with(Type::Text) {
                        return Err(Diagnostic::illegal(
                            format!("`characters` counts the characters in text, and this is {}. Convert it first with `to_text(...)`", kinds[0].name()),
                            args[0].span(),
                        ));
                    }
                    Ok(Type::Number)
                }
                "replace_text" => {
                    for (i, kind) in kinds.iter().enumerate() {
                        if !kind.agrees_with(Type::Text) {
                            return Err(Diagnostic::illegal(
                                format!("`replace_text` works on text, and this is {}. It takes the text, what to look for, and what to put there: `replace_text([name], \"a\", \"b\")`", kind.name()),
                                args[i].span(),
                            ));
                        }
                    }
                    Ok(Type::Text)
                }
                // **Every argument is text, and a number is refused rather than
                // converted.** Silently calling `to_text` on a number would be
                // the grammar deciding how it should look: 7 or 7.0, and a date
                // in whose format. The refusal names the word that decides.
                "join_text" => {
                    for (i, kind) in kinds.iter().enumerate() {
                        if !kind.agrees_with(Type::Text) {
                            return Err(Diagnostic::illegal(
                                format!("`join_text` joins text, and this is {}. Convert it first with `to_text(...)`, which is where you say how it should look", kind.name()),
                                args[i].span(),
                            ));
                        }
                    }
                    Ok(Type::Text)
                }
                "split_text" => {
                    for i in [0usize, 1] {
                        if !kinds[i].agrees_with(Type::Text) {
                            return Err(Diagnostic::illegal(
                                format!("`split_text` cuts text apart, and this is {}. It takes the text, what to cut on, and which piece to keep: `split_text([name], \" \", 1)`", kinds[i].name()),
                                args[i].span(),
                            ));
                        }
                    }
                    if !kinds[2].agrees_with(Type::Number) {
                        return Err(Diagnostic::illegal(
                            format!("the last thing `split_text` takes is which piece to keep, counting from 1, and this is {}", kinds[2].name()),
                            args[2].span(),
                        ));
                    }
                    Ok(Type::Text)
                }

                // **All three have to hold the same kind of thing**, which is the
                // rule `join`'s keys, `fill_missing`'s filler, `first_present`,
                // `lengthen` and `when` all follow. Asking whether a word is
                // between two numbers is a question with no answer.
                "between" => {
                    let mut settled = kinds[0];
                    for (i, kind) in kinds.iter().enumerate().skip(1) {
                        if !settled.agrees_with(*kind) {
                            return Err(Diagnostic::illegal(
                                format!(
                                    "`between` compares one thing against two ends, so all three have to hold the same kind of thing. This one is {} and an earlier one is {}",
                                    kind.name(),
                                    settled.name()
                                ),
                                args[i].span(),
                            ));
                        }
                        if settled == Type::Unknown {
                            settled = *kind;
                        }
                    }
                    Ok(Type::Truth)
                }

                // **A calendar part takes a date, and either kind of date has
                // one.** Asking a number for its year is a mistake worth
                // naming, and `to_date(...)` is the answer when the date
                // arrived as text.
                "year" | "month" | "day" | "weekday" => {
                    if !kinds[0].agrees_with(Type::Date) {
                        return Err(Diagnostic::illegal(
                            format!("`{name}` reads part of a date, and this is {}. Convert it first with `to_date(...)`", kinds[0].name()),
                            args[0].span(),
                        ));
                    }
                    Ok(Type::Number)
                }

                // **`hour` reads the clock, and a plain date has no clock.**
                // This is the one place the two date kinds differ, and it is
                // why they are two kinds at all.
                //
                // Answering nought would be handing back a number that quietly
                // assumed midnight, and the engines do not even agree on that
                // much: DuckDB and Spark answer 0 where polars refuses the
                // operation outright. Refusing is the only answer that means
                // one thing everywhere.
                //
                // **`to_date` is named in the message on purpose**, because it
                // is the repair a reader reaches for and the one that cannot
                // work: it makes a date, which is what they already have.
                "hour" => {
                    if !matches!(kinds[0], Type::Timestamp | Type::Unknown) {
                        let why = if kinds[0].agrees_with(Type::Date) {
                            "`hour` reads the time of day a column carries, and a plain date carries none, so this would be nought on every row. It wants a column that arrived carrying one, which `to_date` does not make".to_string()
                        } else {
                            format!("`hour` reads the time of day a column carries, and this is {}. It wants a column that arrived as a date and a time", kinds[0].name())
                        };
                        return Err(Diagnostic::illegal(why, args[0].span()));
                    }
                    Ok(Type::Number)
                }

                "running_total" => {
                    if !kinds[0].agrees_with(Type::Number) {
                        return Err(Diagnostic::illegal(
                            format!("`running_total` adds values up as it goes, and this is {}. Only numbers add up", kinds[0].name()),
                            args[0].span(),
                        ));
                    }
                    Ok(Type::Number)
                }

                // Both sides divide, so both have to be numbers. Dividing by
                // nought is the engine's answer to give rather than the
                // checker's to predict, the same as `/` already is.
                "remainder" => {
                    for i in [0usize, 1] {
                        if !kinds[i].agrees_with(Type::Number) {
                            return Err(Diagnostic::illegal(
                                format!("`remainder` divides one number by another and hands back what is left over, and this is {}", kinds[i].name()),
                                args[i].span(),
                            ));
                        }
                    }
                    Ok(Type::Number)
                }

                // It hands back a value from this column, so it hands back
                // whatever the column holds. A row with nothing before it and
                // nothing of its own stays missing, which is the one case
                // `fill_missing` covers and this does not.
                "latest" => Ok(kinds[0]),

                // These hand back a value from another row, so they hand back
                // whatever that column holds. A row with nothing before it, or
                // nothing after it, gets `missing`.
                //
                // **How far is optional, and it has to be written out.** A
                // column there would be an offset that changes per row, which is
                // a different operation and one no engine underneath takes in
                // that position. The three refusals are separated because they
                // are three different mistakes: a computed offset, a zero, and a
                // negative each want a different sentence back.
                "previous" | "following" => {
                    if let Some(step) = args.get(1) {
                        // **A negative is written `0 - n` by the time it gets
                        // here**, because the lexer has no negative literal and
                        // a leading `-` is parsed as subtraction from nothing.
                        // It is matched rather than left to fall through, so
                        // that `previous([x], -3)` gets the sentence about the
                        // other word instead of the one about per-row offsets.
                        let negated = matches!(
                            step,
                            Expr::Arithmetic { op: Arith::Subtract, left, right, .. }
                                if matches!(**left, Expr::Whole { value: 0, .. })
                                    && matches!(**right, Expr::Whole { .. })
                        );
                        if negated {
                            let other = if name == "previous" { "following" } else { "previous" };
                            return Err(Diagnostic::illegal(
                                format!("`{name}` already says which way it goes, so how far is counted forwards from 1. To look the other way, use `{other}`"),
                                step.span(),
                            ));
                        }
                        let Expr::Whole { value, .. } = step else {
                            return Err(Diagnostic::illegal(
                                format!("the second thing `{name}` takes is how many rows to look, written as a plain whole number: `{name}([revenue], 12)`. It cannot be worked out per row"),
                                step.span(),
                            ));
                        };
                        if *value == 0 {
                            return Err(Diagnostic::illegal(
                                format!("`{name}([column], 0)` is the column itself, so it has no work to do. Use the column"),
                                step.span(),
                            ));
                        }
                    }
                    Ok(kinds[0])
                }

                "first_present" => {
                    let mut settled = kinds[0];
                    for (i, kind) in kinds.iter().enumerate().skip(1) {
                        if !settled.agrees_with(*kind) {
                            return Err(Diagnostic::illegal(
                                format!(
                                    "`first_present` picks one of these to be the answer, so they all have to hold the same kind of thing. This one is {} and an earlier one is {}. Convert one of them first",
                                    kind.name(),
                                    settled.name()
                                ),
                                args[i].span(),
                            ));
                        }
                        // A known kind beats `Unknown`, so a column the grammar
                        // has no opinion about does not erase one it does.
                        if settled == Type::Unknown {
                            settled = *kind;
                        }
                    }
                    Ok(settled)
                }
                _ => Ok(Type::Unknown),
            }
        }
    }
}
