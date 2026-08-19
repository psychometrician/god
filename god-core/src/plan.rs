//! The plan: what a pipeline means, once the words have been read.
//!
//! **The plan is the contract between the front and the back of the grammar.**
//! One parser fills it in and every backend reads it, which is what lets the same
//! sentence produce answers from one engine and readable code for another.
//!
//! It is a Rust type and not a wire format. Nothing serializes it, nothing sends
//! it anywhere, and no two pieces of code have to agree on how a number is
//! written, because only one piece of code ever builds one (§7).
//!
//! Every node carries the span of the text it came from. That is what lets a
//! refusal put a caret under the word that caused it instead of describing the
//! word in prose, and it is the reason a parsed grammar gives better messages
//! than a grammar embedded in a host language, where the host's own error
//! machinery points at the call rather than the clause (§10).

use std::fmt;

/// Where a piece of the plan came from in the text the caller wrote.
///
/// Byte offsets into the original string, so a message can quote the line and
/// mark the column. Kept on every node rather than only on the ones that can
/// fail today, because the node that cannot fail today is the one that grows a
/// refusal next month.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
}

impl Span {
    pub fn new(start: usize, len: usize) -> Self {
        Span { start, len }
    }

    /// The span covering both, for an expression built from two smaller ones.
    pub fn to(self, other: Span) -> Span {
        let start = self.start.min(other.start);
        let end = (self.start + self.len).max(other.start + other.len);
        Span::new(start, end - start)
    }
}

/// A whole pipeline: the table it starts from, and what happens to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub source: String,
    pub source_span: Span,
    pub steps: Vec<Step>,
}

impl Plan {
    /// The same plan with every span flattened.
    ///
    /// Two plans can then be compared for **what they mean** rather than for
    /// where they were written, which is the only useful comparison between a
    /// pipeline and the same pipeline printed back out and read again: the words
    /// have moved, and nothing else may have.
    ///
    /// This exists because the obvious version of that check is not enough.
    /// Comparing a printed pipeline to itself printed twice only proves the
    /// printer is *consistent* — a printer that drops the same clause every time
    /// drops it in both passes and the two agree perfectly, while the sentence
    /// quietly means less than it did. Comparing the plans catches it.
    pub fn without_spans(&self) -> Plan {
        Plan {
            source: self.source.clone(),
            source_span: Span::new(0, 0),
            steps: self.steps.iter().map(Step::without_spans).collect(),
        }
    }

    /// Every table this pipeline reads, in the order it names them.
    ///
    /// The head first, then whatever a `join` reaches for. A launcher asks for
    /// this rather than working it out, because working it out means parsing,
    /// in a host, once per language, and the copies would differ the first day
    /// a pipeline could name two tables. That day is this one.
    ///
    /// **A table can be named inside a condition, not only by a step.**
    /// `keep where matching(products, by [id])` reads `products` without any
    /// step mentioning it, so the conditions are walked too. Missing one here is
    /// silent in the worst way: the launcher hands over a table it was never
    /// asked for, and the engine reports a table it cannot find.
    pub fn tables(&self) -> Vec<String> {
        let mut names = vec![self.source.clone()];
        let note = |other: &Name, names: &mut Vec<String>| {
            if !names.contains(&other.text) {
                names.push(other.text.clone());
            }
        };
        for step in &self.steps {
            match step {
                Step::Join { other, .. } | Step::AddRows { other, .. } => note(other, &mut names),
                Step::Keep { condition, .. } => {
                    let mut found = Vec::new();
                    condition.walk(&mut |e| {
                        if let Expr::Matching { other, .. } = e {
                            found.push(other.clone());
                        }
                    });
                    for other in &found {
                        note(other, &mut names);
                    }
                }
                _ => {}
            }
        }
        names
    }
}

/// One step. Every step takes a table and returns a table, so a reader can stop
/// anywhere in a pipeline and still be looking at a table (Law 1).
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// `keep where <condition>`
    Keep { condition: Expr, span: Span },

    /// `pick [a, b]`, `pick all_but [c]`, or `pick where name starts "q"`.
    Pick {
        names: Vec<Name>,
        /// `all_but` inverts the list rather than adding a second verb. One word,
        /// one job: choosing columns is choosing columns whichever way you say
        /// which ones.
        all_but: bool,
        /// A question about a column's *name*, when the caller chose the columns
        /// by pattern instead of by listing them.
        ///
        /// **The checker resolves this and clears it**, filling `names` with the
        /// columns that matched, exactly as it settles `join`'s key. A backend is
        /// handed a plain list and never has to match a pattern, and
        /// `show --as god` prints the columns that were chosen rather than the
        /// rule that chose them, which is how a reader sees what they actually
        /// asked for.
        condition: Option<Expr>,
        span: Span,
    },

    /// `add [name] as <expr>`, with an optional `by` that broadcasts an
    /// aggregate back over each group instead of collapsing it.
    Add {
        values: Vec<Named>,
        by: Vec<Name>,
        /// `add where name starts "q" as value * 2`: one value, applied to every
        /// column whose name matches.
        ///
        /// **The checker expands this into ordinary values and clears it**, one
        /// per matched column, exactly as it resolves `pick where`. So a backend
        /// is handed `add [q1] as ..., [q2] as ...` and never learns that a
        /// pattern was involved.
        ///
        /// The matched columns keep their names, because `add` already covers
        /// making a column and replacing one. That is why this needs no rule for
        /// naming the results, and why dplyr's `.names` template has nothing to
        /// answer here.
        across: Option<Across>,
        span: Span,
    },

    /// `summarize [name] as <expr>, ... by [g]`
    Summarize {
        values: Vec<Named>,
        by: Vec<Name>,
        /// `summarize where name starts "q" as average(value)`. Same expansion
        /// as `add`'s, and the matched columns keep their names for the same
        /// reason.
        across: Option<Across>,
        span: Span,
    },

    /// `sort [a], [b] descending`
    Sort { keys: Vec<SortKey>, span: Span },

    /// `take 10`, or `take 1 by [id]` for the first n rows of each group.
    ///
    /// **`by` makes this a window rather than a limit**, and it obliges an order:
    /// "the first row of each group" means nothing until something says first by
    /// what. So a grouped `take` requires a `sort` before it, and the backend
    /// carries that sort's keys into the window rather than trusting the engine
    /// to hand rows over in the order they arrived.
    Take {
        count: u64,
        by: Vec<Name>,
        /// `take_last` rather than `take`: the rows at the other end.
        ///
        /// **A flag rather than a second variant, because it is the same
        /// operation read from the other side.** Every backend renders it by
        /// walking the sort backwards and then putting the order back, so a
        /// parallel `Step` would duplicate each of those spellings to change
        /// one word in them.
        ///
        /// **It always needs a sort, where `take` needs one only when grouped.**
        /// "The first three rows" of a table nobody sorted is at least the three
        /// the pipeline reached first; "the last three" is a claim about an end,
        /// and a table has no end until something says which way it runs.
        last: bool,
        /// `with ties`: keep every row that ties with the last one taken.
        ///
        /// **It needs a sort for the same reason `by` does**, and more sharply:
        /// a tie is a tie *in some ordering*, so with nothing sorted there is
        /// nothing to tie on and the word would mean nothing.
        ///
        /// **The row count stops being readable off the sentence**, which is
        /// why this is off by default and has to be asked for. dplyr's
        /// `slice_max` defaults the other way, so the same request in the two
        /// tools returned different rows with neither saying anything — the
        /// disagreement this word exists to end.
        ties: bool,
        span: Span,
    },

    /// `add_rows more_sales`
    ///
    /// The other table's rows, underneath. A column that is on one side only is
    /// refused by the checker, so there is nothing here to configure.
    AddRows { other: Name, span: Span },

    /// `add_combinations [region, product]`, with an optional
    /// `by [store]` that makes the combinations inside each group.
    ///
    /// **The absent combinations, as rows underneath.** Every row that was
    /// already there is handed on untouched, which is why this is spelled like
    /// `add_rows` and not like a reshaping verb: nothing is rearranged, and the
    /// only thing that changes is how many rows there are.
    ///
    /// Three rulings are built into it rather than configurable.
    ///
    /// **The values come from the table and nowhere else.** The combinations are
    /// the distinct values each named column already holds, crossed. A month
    /// with no row anywhere is never invented, because nothing in the table
    /// names it — that would need a literal sequence written into a sentence,
    /// and the grammar has no shape for one.
    ///
    /// **A missing value is not a category, so it makes no combinations.** The
    /// grid is built from the values that are there. No row is lost by that
    /// ruling, because no original row is touched at all.
    ///
    /// **Every other column of a new row is missing, and there is no clause to
    /// say otherwise.** `fill_missing` already says it, in a second step, and
    /// this step never changes the schema so every column is nameable.
    /// `widen`'s `missing` clause is not a precedent the other way: a bare
    /// `widen` may be terminal, so a `fill_missing` after one cannot name
    /// columns it does not yet know.
    AddCombinations {
        /// The columns whose values are crossed. Two or more, always: one
        /// column on its own has no combinations to make, whatever `by` says.
        names: Vec<Name>,
        /// The columns held fixed, with the crossing done inside each of their
        /// groups. `by` in its usual meaning — the columns that establish which
        /// rows correspond to which (§11.1) — and the reason it is worth having
        /// is that a new row keeps them filled in rather than going missing.
        by: Vec<Name>,
        span: Span,
    },

    /// `drop_duplicates`
    DropDuplicates { span: Span },

    /// `rename [margin] as [profit]`, the new name first.
    ///
    /// The value is a column rather than an expression, which is the only thing
    /// separating this from `add`. It reuses `as` and the same shape, so the
    /// grammar gains a verb and no syntax.
    Rename { values: Vec<Named>, span: Span },

    /// `drop_missing [cost]`, or bare for every column.
    DropMissing { names: Vec<Name>, span: Span },

    /// `fill_missing [cost] as 0`
    FillMissing { values: Vec<Named>, span: Span },

    /// `lengthen [q1, q2, q3] as name [question], value [answer]`
    ///
    /// **Those columns become rows, and the table grows taller** — which is what
    /// the name says, because nobody could ever remember which of `melt` and
    /// `cast` did this (Law 4).
    ///
    /// The columns are chosen exactly the way `pick` chooses them, by list, by
    /// `all_but`, or by a question about the name, so the commonest selection of
    /// all — everything except the identifier — is `all_but [id]` and cost
    /// nothing to build.
    Lengthen {
        /// The columns being stacked, as written.
        names: Vec<Name>,
        all_but: bool,
        condition: Option<Expr>,
        /// What the new name columns are called. Defaults to the single part
        /// `name`, which is the grammar's own word for it, so the sentence with
        /// no naming clause teaches the vocabulary the next sentence needs.
        name: Pattern,
        /// What the new value column is called. `None` when the pattern carries
        /// a `{value}`, which says the value columns are named by the data.
        value: Option<Name>,
        /// Filled in by the checker. A backend renders this and never the
        /// pattern above it.
        resolved: Option<Lengthened>,
        span: Span,
    },

    /// `widen name [question], value [answer] by [student] missing 0 giving [q1, q2]`
    ///
    /// **The inverse of `lengthen`, reading the same two words the other way.**
    /// `name` points at the column holding column names in both; here it is being
    /// consumed rather than made, and the verb is what says so.
    Widen {
        /// Where the new column names come from.
        name: Pattern,
        /// What fills the cells. A column means one row per cell, and the query
        /// stops and names the cell if two rows want one. An aggregate says what
        /// to do about that, which is where tidyr spends `values_fn`.
        value: Expr,
        /// The columns that say which rows go together. Empty means every column
        /// not named above, and the checker fills it in — tidyr's default, and
        /// tidyr's footgun, which is why writing it is worth having a word for.
        by: Vec<Name>,
        /// What an empty cell holds. `None` leaves it missing, which is what it
        /// is.
        missing: Option<Expr>,
        /// The columns this produces.
        ///
        /// **Empty means the step is terminal, and that is the whole answer to
        /// the one question in the grammar the checker cannot answer.** Every
        /// other step maps a known schema to a known schema; this one takes its
        /// column names from the data. So a bare `widen` is allowed and must be
        /// last, and a `widen` that means to carry on says what it produces.
        giving: Vec<Name>,
        span: Span,
    },

    /// `join products by [id]`, with an optional `unmatched "both"`.
    ///
    /// The only thing that varies between the four classic joins is what happens
    /// to a row that found no match, so that is what the argument is named for.
    /// There is no right join, and that is Law 5 rather than an omission: it is a
    /// left join with the tables swapped, so it adds no meaning.
    Join {
        other: Name,
        /// Empty when the caller did not say, which is not the same as none:
        /// the checker fills it in from the names both tables share and reports
        /// the choice as an assumption.
        by: Vec<JoinKey>,
        unmatched: Unmatched,
        span: Span,
    },
}

/// Which unmatched rows survive a join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmatched {
    /// `"this"`, the default. A left join.
    This,
    /// `"none"`. An inner join.
    None,
    /// `"both"`. A full join.
    Both,
}

impl Unmatched {
    pub fn word(self) -> &'static str {
        match self {
            Unmatched::This => "this",
            Unmatched::None => "none",
            Unmatched::Both => "both",
        }
    }

    pub fn read(word: &str) -> Option<Unmatched> {
        match word {
            "this" => Some(Unmatched::This),
            "none" => Some(Unmatched::None),
            "both" => Some(Unmatched::Both),
            _ => None,
        }
    }

    /// Every value, for a message that has to list them.
    pub const ALL: &'static [&'static str] = &["this", "none", "both"];
}

impl Step {
    pub fn span(&self) -> Span {
        match self {
            Step::Keep { span, .. }
            | Step::Pick { span, .. }
            | Step::Add { span, .. }
            | Step::Summarize { span, .. }
            | Step::Sort { span, .. }
            | Step::Take { span, .. }
            | Step::Join { span, .. }
            | Step::AddRows { span, .. }
            | Step::AddCombinations { span, .. }
            | Step::DropDuplicates { span }
            | Step::Rename { span, .. }
            | Step::DropMissing { span, .. }
            | Step::FillMissing { span, .. }
            | Step::Lengthen { span, .. }
            | Step::Widen { span, .. } => *span,
        }
    }

    fn without_spans(&self) -> Step {
        let flat = Span::new(0, 0);
        let names = |ns: &[Name]| ns.iter().map(Name::without_span).collect::<Vec<_>>();
        let values = |vs: &[Named]| {
            vs.iter()
                .map(|v| Named { name: v.name.without_span(), value: v.value.without_spans() })
                .collect::<Vec<_>>()
        };
        match self {
            Step::Keep { condition, .. } => {
                Step::Keep { condition: condition.without_spans(), span: flat }
            }
            Step::Pick { names: ns, all_but, condition, .. } => Step::Pick {
                names: names(ns),
                all_but: *all_but,
                condition: condition.as_ref().map(Expr::without_spans),
                span: flat,
            },
            Step::Add { values: vs, by, across, .. } => Step::Add {
                values: values(vs),
                by: names(by),
                across: across.as_ref().map(Across::without_spans),
                span: flat,
            },
            Step::Summarize { values: vs, by, across, .. } => Step::Summarize {
                values: values(vs),
                by: names(by),
                across: across.as_ref().map(Across::without_spans),
                span: flat,
            },
            Step::Sort { keys, .. } => Step::Sort {
                keys: keys
                    .iter()
                    .map(|k| SortKey {
                        column: k.column.without_span(),
                        descending: k.descending,
                    })
                    .collect(),
                span: flat,
            },
            Step::Take { count, by, last, ties, .. } => {
                Step::Take { count: *count, by: names(by), last: *last, ties: *ties, span: flat }
            }
            Step::AddRows { other, .. } => {
                Step::AddRows { other: other.without_span(), span: flat }
            }
            Step::DropDuplicates { .. } => Step::DropDuplicates { span: flat },
            Step::Rename { values: vs, .. } => Step::Rename { values: values(vs), span: flat },
            Step::DropMissing { names: ns, .. } => {
                Step::DropMissing { names: names(ns), span: flat }
            }
            Step::FillMissing { values: vs, .. } => {
                Step::FillMissing { values: values(vs), span: flat }
            }
            Step::Lengthen { names: ns, all_but, condition, name, value, resolved, .. } => {
                Step::Lengthen {
                    names: names(ns),
                    all_but: *all_but,
                    condition: condition.as_ref().map(Expr::without_spans),
                    name: name.without_span(),
                    value: value.as_ref().map(Name::without_span),
                    resolved: resolved.clone(),
                    span: flat,
                }
            }
            Step::Widen { name, value, by, missing, giving, .. } => Step::Widen {
                name: name.without_span(),
                value: value.without_spans(),
                by: names(by),
                missing: missing.as_ref().map(Expr::without_spans),
                giving: names(giving),
                span: flat,
            },
            Step::Join { other, by, unmatched, .. } => Step::Join {
                other: other.without_span(),
                by: by.iter().map(JoinKey::without_span).collect(),
                unmatched: *unmatched,
                span: flat,
            },
            Step::AddCombinations { names: ns, by, .. } => {
                Step::AddCombinations { names: names(ns), by: names(by), span: flat }
            }
        }
    }

    /// The word the caller wrote, for a message that has to name the step.
    pub fn verb(&self) -> &'static str {
        match self {
            Step::Keep { .. } => "keep",
            Step::Pick { .. } => "pick",
            Step::Add { .. } => "add",
            Step::Summarize { .. } => "summarize",
            Step::Sort { .. } => "sort",
            Step::Take { last, .. } => if *last { "take_last" } else { "take" },
            Step::Join { .. } => "join",
            Step::AddRows { .. } => "add_rows",
            Step::AddCombinations { .. } => "add_combinations",
            Step::DropDuplicates { .. } => "drop_duplicates",
            Step::Rename { .. } => "rename",
            Step::DropMissing { .. } => "drop_missing",
            Step::FillMissing { .. } => "fill_missing",
            Step::Lengthen { .. } => "lengthen",
            Step::Widen { .. } => "widen",
        }
    }
}

/// A column name as the caller wrote it, with where they wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub text: String,
    pub span: Span,
}

impl Name {
    fn without_span(&self) -> Name {
        Name { text: self.text.clone(), span: Span::new(0, 0) }
    }
}

/// A column being created: `[margin] as [revenue] - [cost]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Named {
    pub name: Name,
    pub value: Expr,
}

/// One key in a `sort`, and whether it runs the other way.
///
/// **`descending` is a modifier on a column in an ordering position**, which is
/// one idea rather than two: wherever the grammar orders rows, it is spelled
/// this way. There is deliberately no `ascending`, because ascending is what
/// happens when you do not ask for anything, and a word that means "do the
/// default" is a second way to say the same thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    pub column: Name,
    pub descending: bool,
}

/// One key on each side of a `join`: `by [customer_id] is [id]`.
///
/// **The two halves are usually the same word and that is a convenience rather
/// than the rule.** Real schemas name the primary key `id` and the foreign key
/// `<thing>_id`, so `orders.customer_id` against `customers.id` is what a join
/// ordinarily looks like; every neighbouring tool can say it — `on = .(a = b)`,
/// `join_by(a == b)`, `left_on=`/`right_on=`, `ON a.x = b.y` — and god was the
/// only one that could not until 2026-08-16.
///
/// **`is` rather than a new word**, because `is` is already how the grammar
/// writes equality (§2.4), and the parser's own message for `=` has always sent
/// people to it. Nothing was added to the vocabulary to buy this.
///
/// **This table's column is written first.** The verb has already named the
/// other table, so the unqualified half belongs to the table being piped, and
/// the pair reads in the direction the sentence runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinKey {
    /// The column on the table the pipeline is carrying.
    pub this: Name,
    /// The column on the table being joined to. Equal to `this` for the
    /// ordinary case, which is what a bare `by [id]` builds.
    pub other: Name,
}

impl JoinKey {
    /// One name standing for both sides: `by [id]`.
    pub fn same(name: Name) -> JoinKey {
        JoinKey { other: name.clone(), this: name }
    }

    /// Whether the two halves are the same word, which every backend asks
    /// because most of them have a shorter spelling for that case.
    pub fn is_same(&self) -> bool {
        self.this.text == self.other.text
    }

    fn without_span(&self) -> JoinKey {
        JoinKey { this: self.this.without_span(), other: self.other.without_span() }
    }
}

/// One value written once and applied to every column whose name matches.
///
/// This is dplyr's `across`, and it costs no verb and one word. The selector is
/// the same `where name ...` that `pick` takes, and `value` inside the
/// expression stands for the column being worked on. `name` and `value` are
/// already the pair the reshaping verbs use for what a column is called and what
/// it holds (§4.5), so neither word is new to the vocabulary's ear.
#[derive(Debug, Clone, PartialEq)]
pub struct Across {
    /// A question about a column's name, the same shape `pick where` takes.
    pub selector: Expr,
    /// What to make of each matched column, with `value` standing for it.
    pub value: Expr,
}

impl Across {
    fn without_spans(&self) -> Across {
        Across {
            selector: self.selector.without_spans(),
            value: self.value.without_spans(),
        }
    }
}

/// The shape of a column name, with its pieces named: `"{question}_{year}"`.
///
/// **One idea where tidyr spends four arguments** — `names_sep`, `names_pattern`,
/// `names_prefix` and `names_glue`. `lengthen` reads names apart with it and
/// `widen` puts them together with it, and the verb supplies the direction, so
/// there is one thing to learn rather than two spelled differently.
///
/// **It is deliberately not a regex.** A pattern that is unreadable after a
/// two-month gap fails the test this whole project is organized around (§14.1),
/// and `"{question}_{year}"` is readable after two years. It is the glue and
/// f-string idea readers of both host languages have already met.
///
/// `name [question]` is the one-part case of the same idea rather than a second
/// shape: it means exactly what `"{question}"` means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    /// The literal text around the parts. Always one longer than `parts`, so
    /// `"{a}_{b}"` holds `["", "_", ""]`.
    pub literals: Vec<String>,
    pub parts: Vec<PatternPart>,
    pub span: Span,
    /// Whether the caller wrote a quoted pattern or a bare column. It changes
    /// nothing about the meaning and everything about how it prints back.
    pub quoted: bool,
}

/// One `{piece}` of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternPart {
    /// `{question}` — a column holding this piece of the name.
    Named(String),
    /// `{value}` — this piece says *which value column* the rest belongs to.
    ///
    /// tidyr spells this `.value`, a sentinel with no meaning outside the one
    /// argument it appears in, and it is reliably the first thing anyone has to
    /// look up twice. Here it is the word the grammar already uses for what a
    /// column holds.
    Value,
}

impl Pattern {
    /// The `name [question]` case: one part, no literal text around it.
    pub fn single(text: impl Into<String>, span: Span) -> Pattern {
        Pattern {
            literals: vec![String::new(), String::new()],
            parts: vec![PatternPart::Named(text.into())],
            span,
            quoted: false,
        }
    }

    /// The names of the pieces that become columns, in order.
    pub fn named_parts(&self) -> Vec<&str> {
        self.parts
            .iter()
            .filter_map(|p| match p {
                PatternPart::Named(n) => Some(n.as_str()),
                PatternPart::Value => None,
            })
            .collect()
    }

    pub fn has_value(&self) -> bool {
        self.parts.contains(&PatternPart::Value)
    }

    /// Pull a column name apart into one piece per part.
    ///
    /// **Each part runs up to the next literal, taking the first occurrence
    /// rather than the last.** That is the rule a reader can hold: pieces are
    /// read left to right, and the separator that ends a piece is the first one
    /// after it starts. `"{a}_{b}"` reads `q1_score_2020` as `q1` and
    /// `score_2020`, which is the only reading that keeps two parts.
    ///
    /// A piece that would be empty is not a match. An empty column-name piece is
    /// a typo every time, and matching it would make `"{a}_{b}"` accept `_x`.
    pub fn read(&self, text: &str) -> Option<Vec<String>> {
        let mut rest = text.strip_prefix(self.literals[0].as_str())?;
        let mut pieces = Vec::with_capacity(self.parts.len());
        for (i, _) in self.parts.iter().enumerate() {
            let after = &self.literals[i + 1];
            let piece = if after.is_empty() {
                if i + 1 != self.parts.len() {
                    // Two parts with nothing between them cannot be told apart,
                    // and the parser refuses that before this is ever reached.
                    return None;
                }
                let whole = rest;
                rest = "";
                whole
            } else {
                let at = rest.find(after.as_str())?;
                let (piece, tail) = rest.split_at(at);
                rest = &tail[after.len()..];
                piece
            };
            if piece.is_empty() {
                return None;
            }
            pieces.push(piece.to_string());
        }
        if rest.is_empty() {
            Some(pieces)
        } else {
            None
        }
    }

    /// How it was written, for a plan printed back out as the grammar.
    pub fn text(&self) -> String {
        if !self.quoted {
            if let [PatternPart::Named(one)] = self.parts.as_slice() {
                return format!("[{one}]");
            }
        }
        let mut out = String::from("\"");
        for (i, part) in self.parts.iter().enumerate() {
            out.push_str(&self.literals[i]);
            match part {
                PatternPart::Named(n) => out.push_str(&format!("{{{n}}}")),
                PatternPart::Value => out.push_str("{value}"),
            }
        }
        out.push_str(self.literals.last().expect("one more literal than parts"));
        out.push('"');
        out
    }

    fn without_span(&self) -> Pattern {
        Pattern { span: Span::new(0, 0), ..self.clone() }
    }
}

/// What the checker worked out about a `lengthen`, and what the backends read.
///
/// **Every backend is handed literals and never a pattern.** The checker knows
/// every column name, so it works out each piece's value per column and writes
/// the answer here — the same resolve-at-check-time move `pick where`, `join`'s
/// key and `across` all make. It is why this needs no string functions in a
/// query and is portable to any engine without argument.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Lengthened {
    /// The columns that were not stacked, in the order the table had them.
    pub keep: Vec<String>,
    /// The new columns holding pieces of the old names.
    pub name_columns: Vec<String>,
    /// The new columns holding the values. One, unless `{value}` was written.
    pub value_columns: Vec<String>,
    /// One per block of output rows.
    pub rows: Vec<LengthenRow>,
}

/// One block: what the name columns hold, and where the values come from.
#[derive(Debug, Clone, PartialEq)]
pub struct LengthenRow {
    /// The literal value for each name column, in order.
    pub labels: Vec<String>,
    /// The source column for each value column, in order.
    pub sources: Vec<String>,
}

/// The three text tests that sit between their operands, as `is` and `in` do.
///
/// They are word operators rather than functions because they read aloud, and
/// because it keeps the three names clear of dplyr's column selectors, which
/// spell the same words and mean something else entirely (§20.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextOp {
    Starts,
    Ends,
    Contains,
}

impl TextOp {
    pub fn word(self) -> &'static str {
        match self {
            TextOp::Starts => "starts",
            TextOp::Ends => "ends",
            TextOp::Contains => "contains",
        }
    }
}

/// Which window. Two, where dplyr has six (§20.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    /// Ties share a place and the next value skips it, the way a race is
    /// scored: 1, 2, 2, 4. This is what a person means by rank, so it gets the
    /// word. dplyr calls it `min_rank`, which names the implementation.
    Rank,
    /// 1, 2, 3, 4 down the rows, with ties broken by whatever order the rows are
    /// already in. Never equal for two rows, which is the whole difference.
    RowNumber,
}

impl Window {
    pub fn word(self) -> &'static str {
        match self {
            Window::Rank => "rank",
            Window::RowNumber => "row_number",
        }
    }
}

/// Everything that produces a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `[revenue]` — inside brackets it is a column, always.
    Column(Name),
    /// `"West"`
    Text { value: String, span: Span },
    /// A number written without a decimal point. Kept apart from `Decimal` so
    /// that `take 10` and a count come out of a backend as integers.
    Whole { value: i64, span: Span },
    /// A number written with a decimal point.
    Decimal { value: f64, span: Span },
    /// `yes` / `no` — one spelling for a truth value, in every host.
    Truth { value: bool, span: Span },
    /// `missing` — one spelling for the absent value, in every host.
    Missing { span: Span },

    /// `[a] + [b]`, and the other three.
    Arithmetic {
        op: Arith,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// `[a] is "x"`, `[a] > 3`, and the rest.
    Compare {
        op: Compare,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// `<a> and <b>`, `<a> or <b>`.
    Logic {
        op: Logic,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// `not <a>`
    Not { inner: Box<Expr>, span: Span },
    /// `[region] in {"West", "East"}`
    In {
        left: Box<Expr>,
        set: Vec<Expr>,
        negated: bool,
        span: Span,
    },
    /// `[x] is missing` / `[x] is not missing`. A test of its own rather than a
    /// comparison against `missing`, because in every engine underneath, the
    /// absent value compared to anything is neither true nor false.
    IsMissing {
        inner: Box<Expr>,
        negated: bool,
        span: Span,
    },
    /// `total([margin])`, `row_count()`.
    ///
    /// **One shape for every function, whatever its arity.** `total of [x]` reads
    /// better and does not survive contact with a function that takes two
    /// arguments, and a grammar with one shape for short names and another for
    /// long ones has an exception in it.
    Call {
        name: String,
        args: Vec<Expr>,
        span: Span,
    },

    /// `[product] starts "W"`, and the two like it.
    TextTest {
        op: TextOp,
        left: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },

    /// `any name starts "q" as value > 3` — one condition asked of every
    /// column whose name matches, and joined.
    ///
    /// **This is `across` for a question rather than for a value**, and it
    /// reuses every part of it: the selector is the same `where name ...` that
    /// `pick` takes, and `value` stands for the column being asked about, the
    /// same way it does inside `add where`.
    ///
    /// **It is expanded before anything is checked**, into one ordinary
    /// condition per matched column joined by `or` or `and`, so every rule that
    /// applies to a condition written by hand applies here for free and without
    /// knowing this exists. That is the move §13.11 records for `across`, and
    /// the reason this cost no new checking.
    Quantified {
        /// Whether one matching column is enough, or all of them must.
        every: bool,
        /// A question about a column's name or kind, the shape `pick where`
        /// takes.
        selector: Box<Expr>,
        /// The question asked of each matched column, with `value` standing for
        /// it.
        test: Box<Expr>,
        span: Span,
    },

    /// `kind`, meaning what the column being considered holds.
    ///
    /// The other thing you can ask about a column without looking at a row.
    /// `pick where kind is number` is the selection dplyr spells
    /// `where(is.numeric)` and pandas spells `select_dtypes`, and it joins with
    /// a name test for free: `pick where kind is number and name starts "q"`.
    ColumnKind { span: Span },

    /// `value`, meaning the contents of the column being worked on.
    ///
    /// Only legal inside an `add where` or `summarize where`, and resolved away
    /// there into an ordinary column reference, so no backend sees one.
    ColumnValue { span: Span },

    /// `name`, meaning the name of the column being considered.
    ///
    /// **The one place a column's own name is a value.** It exists so that
    /// `pick where name starts "q"` can say what it is testing, rather than
    /// leaving `starts` to mean a value in one place and a name in another with
    /// nothing written to say which. That would be a context rule, which is what
    /// the grammar removed at M0 by splitting `first(10)` from `first(revenue)`.
    ///
    /// It is legal only inside `pick where`, and the checker resolves it away
    /// there, so no backend ever sees one.
    ColumnName { span: Span },

    /// `rank([delay] descending)` and `row_number()`.
    ///
    /// **A window answers for every row by looking at all of them**, which is
    /// neither what an aggregate does nor what a scalar does, and it is why
    /// these are a variant rather than an ordinary call: `rank` takes a column
    /// in an *ordering* position and may carry `descending`, exactly as a sort
    /// key does. One idea, spelled one way, in the two places it appears.
    ///
    /// **They differ in where the order comes from, and that is the whole
    /// distinction between them.** `rank` is told: its own argument says what to
    /// rank by, so it is self-contained and reads the same wherever it stands.
    /// `row_number` is not told anything, so it can only mean the order the
    /// table is already in, and a table has no order until a `sort` gives it
    /// one. That is why one of them requires a preceding `sort` and the other
    /// never does.
    Window {
        kind: Window,
        /// What to rank by. `None` for `row_number`, which takes the order the
        /// rows already have.
        key: Option<SortKey>,
        span: Span,
    },

    /// `when([score] >= 90, "A", [score] >= 70, "B", otherwise "C")`
    ///
    /// **Order is the meaning and the first match wins**, which is the same
    /// reading `first_present` already asks for: an argument list as a priority
    /// order rather than a set. That is why this is variadic, and why §13.9's
    /// rule permits it — what does not change with the argument count is what
    /// the word *means*.
    ///
    /// **It is a variant rather than an entry in the function table** for the
    /// reason `matching` is: its arguments come in pairs and carry a trailing
    /// `otherwise`, so `name([column])` is not a sentence it has, and the test
    /// that walks the functions builds exactly that.
    ///
    /// **The `if` form was refused by mechanism rather than by taste.** `"A" if
    /// [x] >= 90` reads better and neither host can say it: R cannot parse a
    /// postfix `if`, and Python parses it, calls `__bool__` on the expression,
    /// and picks a branch while the sentence is still being built. Nothing
    /// raises, and the condition is discarded.
    When {
        /// Each test and what it gives, in the order they were written.
        arms: Vec<(Expr, Expr)>,
        /// What an unmatched row gets. `None` means `missing`, which is what
        /// SQL's `CASE` without an `ELSE` gives and what anyone arriving from
        /// dplyr's `case_when` already expects.
        otherwise: Option<Box<Expr>>,
        span: Span,
    },

    /// `look_up([code], "W", "West", "E", "East", otherwise [code])`
    ///
    /// **A lookup table: written values become written values, each pair side
    /// by side.** It is `when` specialized to equality on one subject, and a
    /// variant for the same reason `when` is one — the arguments come in pairs
    /// and end with a marker, so `name([column])` is not a sentence it has.
    ///
    /// **`otherwise` is required here where `when` leaves it optional.** The
    /// neighbours split into two words over what happens to an unpaired value
    /// — left alone against sent missing — so a default either way would
    /// surprise half of everyone arriving. The sentence says where they go:
    /// `otherwise [code]` keeps them, `otherwise missing` drops them, and a
    /// written value is a default. The same move `join`'s `unmatched` makes
    /// for rows, applied to values.
    Lookup {
        /// The value being looked up. A column, ordinarily.
        subject: Box<Expr>,
        /// Each written value and what it becomes. At least one pair, every
        /// `from` a literal the checker has verified, no `from` twice.
        pairs: Vec<(Expr, Expr)>,
        /// Where a value with no pair goes. Never absent — the parser refuses
        /// the sentence without it.
        otherwise: Box<Expr>,
        span: Span,
    },

    /// `rolling(average([revenue]), 7)` — an aggregate asked of the last n
    /// rows, answered for every row.
    ///
    /// **A variant rather than an ordinary call, for the reason `matching` is
    /// one**: its first argument is not a value. `average([revenue])` inside it
    /// is the window's parameter — which question to ask of the rows in frame —
    /// not a live aggregate, and a plan that stored it as one would collapse
    /// where it should slide: `aggregates()` answers whether a value collapses
    /// a group, and a rolling aggregate collapses nothing.
    ///
    /// The aggregate's name and argument are held apart so the checker can
    /// judge each — the name against the aggregates a moving window can carry,
    /// the argument against the aggregate's own rules — and so a backend reads
    /// a name and a column rather than pattern-matching a nested call.
    Rolling {
        /// Which aggregate: `total`, `average`, `median`, `smallest`,
        /// `largest` or `standard_deviation`. Checked there, not here, so the
        /// refusals can name what to write instead.
        agg: String,
        agg_span: Span,
        /// What the aggregate reads. A plain column, by the same ruling an
        /// ordering position has: the computed value is one `add` away.
        args: Vec<Expr>,
        /// How many rows the window holds, the row itself included. A plain
        /// whole number of at least two by the time the checker is done.
        count: Box<Expr>,
        span: Span,
    },

    /// `matching(products, by [id])` — does this row have a partner over there?
    ///
    /// **A filtering join, spelled as what it is.** A semi join and an anti join
    /// add no columns; they only decide which rows survive, so they are not
    /// joins and do not get `join`'s name. They are a condition, and the verb
    /// that takes a condition is `keep`.
    ///
    /// **This is the first expression that names a second table**, which is why
    /// it is a variant of its own rather than an entry in the function table:
    /// its first argument is a table rather than a value, it carries a `by`, and
    /// the checker has to resolve that `by` against a schema no backend can see.
    /// `not matching(...)` is the anti join, and it needs no spelling of its own
    /// because `not` already exists.
    ///
    /// **Duplicate keys on the other side cannot multiply rows here**, which is
    /// the one thing `join` could not promise. A row either has a partner or it
    /// does not; how many it has never reaches the answer.
    Matching {
        other: Name,
        /// Empty when the caller did not say, and filled in by the checker from
        /// the names both tables share, exactly as `join` does. One rule for
        /// working out a key, not two.
        ///
        /// **It takes a differently-named pair for the same reason**: a
        /// filtering join asks the identical question a join asks, and a
        /// grammar where `join products by [customer_id] is [id]` is a sentence
        /// and `keep where matching(products, by [customer_id] is [id])` is not
        /// would have an exception in it.
        by: Vec<JoinKey>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Column(n) => n.span,
            Expr::Text { span, .. }
            | Expr::Whole { span, .. }
            | Expr::Decimal { span, .. }
            | Expr::Truth { span, .. }
            | Expr::Missing { span }
            | Expr::Arithmetic { span, .. }
            | Expr::Compare { span, .. }
            | Expr::Logic { span, .. }
            | Expr::Not { span, .. }
            | Expr::In { span, .. }
            | Expr::IsMissing { span, .. }
            | Expr::TextTest { span, .. }
            | Expr::ColumnName { span }
            | Expr::ColumnValue { span }
            | Expr::ColumnKind { span }
            | Expr::Window { span, .. }
            | Expr::When { span, .. }
            | Expr::Matching { span, .. }
            | Expr::Quantified { span, .. }
            | Expr::Rolling { span, .. }
            | Expr::Lookup { span, .. }
            | Expr::Call { span, .. } => *span,
        }
    }

    fn without_spans(&self) -> Expr {
        let flat = Span::new(0, 0);
        let boxed = |e: &Box<Expr>| Box::new(e.without_spans());
        match self {
            Expr::Column(n) => Expr::Column(n.without_span()),
            Expr::Text { value, .. } => Expr::Text { value: value.clone(), span: flat },
            Expr::Whole { value, .. } => Expr::Whole { value: *value, span: flat },
            Expr::Decimal { value, .. } => Expr::Decimal { value: *value, span: flat },
            Expr::Truth { value, .. } => Expr::Truth { value: *value, span: flat },
            Expr::Missing { .. } => Expr::Missing { span: flat },
            Expr::Arithmetic { op, left, right, .. } => Expr::Arithmetic {
                op: *op,
                left: boxed(left),
                right: boxed(right),
                span: flat,
            },
            Expr::Compare { op, left, right, .. } => Expr::Compare {
                op: *op,
                left: boxed(left),
                right: boxed(right),
                span: flat,
            },
            Expr::Logic { op, left, right, .. } => Expr::Logic {
                op: *op,
                left: boxed(left),
                right: boxed(right),
                span: flat,
            },
            Expr::Not { inner, .. } => Expr::Not { inner: boxed(inner), span: flat },
            Expr::In { left, set, negated, .. } => Expr::In {
                left: boxed(left),
                set: set.iter().map(Expr::without_spans).collect(),
                negated: *negated,
                span: flat,
            },
            Expr::IsMissing { inner, negated, .. } => Expr::IsMissing {
                inner: boxed(inner),
                negated: *negated,
                span: flat,
            },
            Expr::Call { name, args, .. } => Expr::Call {
                name: name.clone(),
                args: args.iter().map(Expr::without_spans).collect(),
                span: flat,
            },
            Expr::TextTest { op, left, value, .. } => Expr::TextTest {
                op: *op,
                left: boxed(left),
                value: boxed(value),
                span: flat,
            },
            Expr::ColumnName { .. } => Expr::ColumnName { span: flat },
            Expr::ColumnValue { .. } => Expr::ColumnValue { span: flat },
            Expr::ColumnKind { .. } => Expr::ColumnKind { span: flat },
            Expr::Window { kind, key, .. } => Expr::Window {
                kind: *kind,
                key: key.as_ref().map(|k| SortKey {
                    column: k.column.without_span(),
                    descending: k.descending,
                }),
                span: flat,
            },
            Expr::When { arms, otherwise, .. } => Expr::When {
                arms: arms
                    .iter()
                    .map(|(t, v)| (t.without_spans(), v.without_spans()))
                    .collect(),
                otherwise: otherwise.as_ref().map(|e| Box::new(e.without_spans())),
                span: flat,
            },
            Expr::Matching { other, by, .. } => Expr::Matching {
                other: other.without_span(),
                by: by.iter().map(JoinKey::without_span).collect(),
                span: flat,
            },
            Expr::Quantified { every, selector, test, .. } => Expr::Quantified {
                every: *every,
                selector: Box::new(selector.without_spans()),
                test: Box::new(test.without_spans()),
                span: flat,
            },
            Expr::Rolling { agg, args, count, .. } => Expr::Rolling {
                agg: agg.clone(),
                agg_span: flat,
                args: args.iter().map(Expr::without_spans).collect(),
                count: boxed(count),
                span: flat,
            },
            Expr::Lookup { subject, pairs, otherwise, .. } => Expr::Lookup {
                subject: boxed(subject),
                pairs: pairs
                    .iter()
                    .map(|(f, t)| (f.without_spans(), t.without_spans()))
                    .collect(),
                otherwise: boxed(otherwise),
                span: flat,
            },
        }
    }

    /// Walk this expression and everything inside it.
    pub fn walk(&self, f: &mut impl FnMut(&Expr)) {
        f(self);
        match self {
            Expr::Arithmetic { left, right, .. }
            | Expr::Compare { left, right, .. }
            | Expr::Logic { left, right, .. } => {
                left.walk(f);
                right.walk(f);
            }
            Expr::Not { inner, .. } | Expr::IsMissing { inner, .. } => inner.walk(f),
            Expr::TextTest { left, value, .. } => {
                left.walk(f);
                value.walk(f);
            }
            Expr::In { left, set, .. } => {
                left.walk(f);
                for e in set {
                    e.walk(f);
                }
            }
            Expr::Call { args, .. } => {
                for a in args {
                    a.walk(f);
                }
            }
            // The aggregate's argument and the count are both walked, so the
            // rules that read column references — a column made in this same
            // step, a table a condition names — see inside. What must *not*
            // treat the inside as live is `aggregates()`, which is why that
            // question is answered by its own recursion rather than by this
            // walk.
            Expr::Rolling { args, count, .. } => {
                for a in args {
                    a.walk(f);
                }
                count.walk(f);
            }
            // **Walking into a `when` is what keeps every existing rule
            // applying to it.** `aggregates` and `windows` are how the checker
            // tells the three kinds apart, and a variant they cannot see into is
            // a hole in `summarize`'s rule, in `keep`'s, and in `add`'s `by`.
            Expr::When { arms, otherwise, .. } => {
                for (test, value) in arms {
                    test.walk(f);
                    value.walk(f);
                }
                if let Some(e) = otherwise {
                    e.walk(f);
                }
            }
            Expr::Lookup { subject, pairs, otherwise, .. } => {
                subject.walk(f);
                for (from, to) in pairs {
                    from.walk(f);
                    to.walk(f);
                }
                otherwise.walk(f);
            }
            _ => {}
        }
    }

    /// Whether a window sits anywhere inside this value.
    ///
    /// The mirror of `aggregates`, and needed for the same reason: three places
    /// have to tell the three kinds apart, and none of them should be doing it
    /// by looking for a name.
    pub fn windows(&self) -> bool {
        let mut found = false;
        self.walk(&mut |e| {
            // Two shapes, one question. `rank` and `row_number` are their own
            // variant because they are parsed specially; `running_total`,
            // `previous` and `following` are ordinary calls and are windows
            // because the vocabulary says so. Asking only about the variant is
            // how three separate rules would quietly stop applying to the
            // second three.
            let window = match e {
                Expr::Window { .. } | Expr::Rolling { .. } => true,
                Expr::Call { name, .. } => crate::vocabulary::is_window(name),
                _ => false,
            };
            if window {
                found = true;
            }
        });
        found
    }

    /// Whether this expression collapses many rows into one.
    ///
    /// The question the grammar asks constantly: `summarize` requires it of every
    /// value, and `add` treats it as a broadcast over the group rather than a
    /// collapse. Answered by looking for an aggregating call anywhere inside,
    /// because `total([revenue]) - total([cost])` aggregates and neither half of
    /// it is a bare call.
    ///
    /// **Its own recursion rather than `walk`, and `Rolling` is why.** The
    /// aggregate written inside `rolling(average([x]), 7)` is the window's
    /// parameter, not a live call: a rolling aggregate answers once per row and
    /// collapses nothing, so this stops at the variant instead of descending
    /// into it. `walk` still descends there — the column rules need to see
    /// inside — which is exactly why this question cannot be asked through it.
    pub fn aggregates(&self) -> bool {
        match self {
            Expr::Rolling { .. } => false,
            Expr::Call { name, args, .. } => {
                crate::vocabulary::is_aggregate(name) || args.iter().any(Expr::aggregates)
            }
            Expr::Arithmetic { left, right, .. }
            | Expr::Compare { left, right, .. }
            | Expr::Logic { left, right, .. } => left.aggregates() || right.aggregates(),
            Expr::Not { inner, .. } | Expr::IsMissing { inner, .. } => inner.aggregates(),
            Expr::TextTest { left, value, .. } => left.aggregates() || value.aggregates(),
            Expr::In { left, set, .. } => {
                left.aggregates() || set.iter().any(Expr::aggregates)
            }
            Expr::When { arms, otherwise, .. } => {
                arms.iter().any(|(t, v)| t.aggregates() || v.aggregates())
                    || otherwise.as_ref().is_some_and(|e| e.aggregates())
            }
            Expr::Lookup { subject, pairs, otherwise, .. } => {
                subject.aggregates()
                    || pairs.iter().any(|(f, t)| f.aggregates() || t.aggregates())
                    || otherwise.aggregates()
            }
            // The leaves, named rather than swept up, so that a new variant
            // fails to compile here instead of quietly answering no. `Rolling`
            // nearly slipped through a catch-all the day after one was left.
            Expr::Column(_)
            | Expr::Text { .. }
            | Expr::Whole { .. }
            | Expr::Decimal { .. }
            | Expr::Truth { .. }
            | Expr::Missing { .. }
            | Expr::ColumnName { .. }
            | Expr::ColumnValue { .. }
            | Expr::ColumnKind { .. }
            | Expr::Window { .. }
            | Expr::Matching { .. }
            | Expr::Quantified { .. } => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arith {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl fmt::Display for Arith {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Arith::Add => "+",
            Arith::Subtract => "-",
            Arith::Multiply => "*",
            Arith::Divide => "/",
        })
    }
}

/// The six comparisons.
///
/// `is` and `is not` are the grammar's spelling of equality, because `==` is
/// Python's and R's while `=` is SQL's, and a word that means the same thing in
/// all three is worth more than a symbol that means different things (§13.2).
/// The four orderings are symbols, because `<` and `>` already agree everywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compare {
    Is,
    IsNot,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Logic {
    And,
    Or,
}
