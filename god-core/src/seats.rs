//! Where each kind of word can stand, in one table.
//!
//! **The vocabulary says what the words are; this module says where they may
//! stand.** A seat is a place in a step that takes a computed value, and the
//! grammar's composition rule is not an enumeration of blessed pairs — it
//! derives from what each seat is for:
//!
//! * **A value seat composes.** `add`'s value, an aggregate's argument, a
//!   `when` branch, `widen`'s cell, `fill_missing`'s filler: scalars nest
//!   freely there, because one value in and one value out stacks.
//! * **An ordering position names a column, never an expression.** `sort`'s
//!   key and `rank`'s argument are the same seat, and a computed key is one
//!   `add` away — a second spelling of that `add` is what the refusal
//!   declines to be (§3, derive rather than enumerate).
//! * **A group's answer stands where the step has a place for it.**
//!   `summarize` is that place; `add` broadcasts it back onto the rows;
//!   `widen` accepts it where two rows want one cell. It may not decide
//!   `keep`'s question or fill a hole, because both work one row at a time
//!   before any group's answer exists — and each refusal says which two-step
//!   spelling to write instead.
//! * **A place along the rows is written onto rows, in a declared order.**
//!   A window stands in `add`, after a `sort` has settled the order it
//!   walks — `rank` alone names its own — and nowhere else: every other
//!   seat either has no row to write onto or no way to say the order.
//!
//! The table below is those four properties, cell by cell, and **the tests
//! run every cell through the real parser and checker**, so this table and
//! the engine cannot drift apart: a rule change that forgets its cell here
//! fails the build, and a cell claimed here that the checker does not
//! enforce fails the same way.
//!
//! What the table does not do is judge usefulness. A sentence these seats
//! accept is answered however strange it is, because well-formed and
//! worthwhile are different questions with different owners (§3, Law 11):
//! the engine settles the first, and the second belongs to whoever reads
//! the answer.

use crate::vocabulary::Kind;

/// A place in a step where a computed value can stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// `keep where ...` — the question that decides which rows survive.
    Condition,
    /// `add [name] as ...` — a value written onto every row, per group when
    /// `by` is present.
    Value,
    /// `summarize [name] as ...` — the answer that stands for a whole group.
    GroupAnswer,
    /// `sort ...`, and the argument of `rank(...)` — an ordering position.
    OrderingKey,
    /// `fill_missing [x] as ...` — what a hole becomes.
    Filler,
    /// `widen ... value ...` — what fills a cell of the wide table.
    CellValue,
}

impl Seat {
    pub fn word(self) -> &'static str {
        match self {
            Seat::Condition => "condition",
            Seat::Value => "value",
            Seat::GroupAnswer => "group_answer",
            Seat::OrderingKey => "ordering_key",
            Seat::Filler => "filler",
            Seat::CellValue => "cell_value",
        }
    }

    /// Where a reader meets the seat, for the table the book prints.
    pub fn shown_as(self) -> &'static str {
        match self {
            Seat::Condition => "keep where ...",
            Seat::Value => "add [name] as ...",
            Seat::GroupAnswer => "summarize [name] as ...",
            Seat::OrderingKey => "sort ..., rank(...)",
            Seat::Filler => "fill_missing [x] as ...",
            Seat::CellValue => "widen ... value ...",
        }
    }
}

/// Every seat, for anything that walks the whole table.
pub const SEATS: &[Seat] = &[
    Seat::Condition,
    Seat::Value,
    Seat::GroupAnswer,
    Seat::OrderingKey,
    Seat::Filler,
    Seat::CellValue,
];

/// Every function kind, in the order the book shelves them.
pub const KINDS: &[Kind] = &[Kind::Scalar, Kind::Aggregate, Kind::Window];

/// What the checker says when a function of this kind heads the value in
/// this seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Stands,
    Refused,
}

/// The table. Every cell is decided here, and the match is total on purpose:
/// a new seat or a new kind will not compile until its whole row or column
/// is answered, which is the completeness this module exists to force.
pub fn rule_for(seat: Seat, kind: Kind) -> Admission {
    use Admission::*;
    match (seat, kind) {
        // A condition asks one row a question. A scalar can; a group's answer
        // and a place along the rows are both worked out after the rows are
        // chosen, so neither can decide which rows those are.
        (Seat::Condition, Kind::Scalar) => Stands,
        (Seat::Condition, Kind::Aggregate) => Refused,
        (Seat::Condition, Kind::Window) => Refused,

        // `add` writes a value onto every row, which is the one seat with a
        // row for every kind of answer: a scalar per row, a group's answer
        // broadcast back, a place once a `sort` has settled the order.
        (Seat::Value, Kind::Scalar) => Stands,
        (Seat::Value, Kind::Aggregate) => Stands,
        (Seat::Value, Kind::Window) => Stands,

        // A group's answer must span its group, so the head of the value is
        // an aggregate — scalars stand inside its argument — and one level
        // only: an aggregate already asks about the whole group, so it
        // cannot hold another that does. A window answers per row, and one
        // row per group has nowhere to put that.
        (Seat::GroupAnswer, Kind::Scalar) => Refused,
        (Seat::GroupAnswer, Kind::Aggregate) => Stands,
        (Seat::GroupAnswer, Kind::Window) => Refused,

        // An ordering position names a column. The computed key someone
        // wants here is one `add` away, and a second spelling of that add
        // is what this refusal declines to be.
        (Seat::OrderingKey, _) => Refused,

        // A hole is filled one row at a time. A scalar fills it; a group's
        // answer does not exist at that row yet, and a value that looks
        // along the rows needs an order no filler ever declares. Each
        // refusal names the column to make first.
        (Seat::Filler, Kind::Scalar) => Stands,
        (Seat::Filler, Kind::Aggregate) => Refused,
        (Seat::Filler, Kind::Window) => Refused,

        // A cell of the wide table holds one value. A scalar computes it, an
        // aggregate is the collision answer — two rows wanting one cell —
        // and a place along the rows has no meaning inside a single cell.
        (Seat::CellValue, Kind::Scalar) => Stands,
        (Seat::CellValue, Kind::Aggregate) => Stands,
        (Seat::CellValue, Kind::Window) => Refused,
    }
}

/// The note a cell carries beyond stands/refused, for the table the book
/// prints. Empty for the cells whose one word is the whole story.
pub fn note_for(seat: Seat, kind: Kind) -> &'static str {
    match (seat, kind) {
        (Seat::Condition, Kind::Aggregate) => "summarize first, then keep",
        (Seat::Condition, Kind::Window) => "add the place as a column, then keep",
        (Seat::Value, Kind::Aggregate) => "the whole table's answer, or each group's with by",
        (Seat::Value, Kind::Window) => "after a sort settles the order; rank names its own",
        (Seat::GroupAnswer, Kind::Scalar) => "stands inside an aggregate's argument",
        (Seat::GroupAnswer, Kind::Aggregate) => "one level; an aggregate cannot hold another",
        (Seat::OrderingKey, _) => "an ordering position names a column; the computed key is an add away",
        (Seat::Filler, Kind::Aggregate) => "make it a column with add ... by, then fill",
        (Seat::Filler, Kind::Window) => "make it a column with sort then add, then fill",
        (Seat::CellValue, Kind::Aggregate) => "the answer when two rows want one cell",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::{Schema, Type};

    /// One sentence per cell, run through the real parser and checker, so the
    /// table above and the engine cannot disagree. Every probe's head is the
    /// same small table, and the window probes carry the sort their order
    /// law demands, so the one thing each cell measures is its seat.
    fn probe(seat: Seat, kind: Kind) -> &'static str {
        match (seat, kind) {
            (Seat::Condition, Kind::Scalar) => "t then keep where (characters([g]) > 2)",
            (Seat::Condition, Kind::Aggregate) => "t then keep where (average([x]) > 2)",
            (Seat::Condition, Kind::Window) => "t then sort [x] then keep where (rank([x]) > 2)",

            (Seat::Value, Kind::Scalar) => "t then add [v] as upper([g])",
            (Seat::Value, Kind::Aggregate) => "t then add [v] as average([x]) by [g]",
            (Seat::Value, Kind::Window) => "t then sort [x] then add [v] as running_total([x])",

            (Seat::GroupAnswer, Kind::Scalar) => "t then summarize [v] as characters([g]) by [g]",
            (Seat::GroupAnswer, Kind::Aggregate) => "t then summarize [v] as total(characters([g])) by [g]",
            (Seat::GroupAnswer, Kind::Window) => "t then sort [x] then summarize [v] as rank([x]) by [g]",

            (Seat::OrderingKey, Kind::Scalar) => "t then sort characters([g])",
            (Seat::OrderingKey, Kind::Aggregate) => "t then sort total([x])",
            (Seat::OrderingKey, Kind::Window) => "t then sort [x] then add [v] as rank(characters([g]))",

            (Seat::Filler, Kind::Scalar) => "t then fill_missing [x] as ([y] * 0)",
            (Seat::Filler, Kind::Aggregate) => "t then fill_missing [x] as average([x])",
            (Seat::Filler, Kind::Window) => "t then fill_missing [x] as previous([x])",

            (Seat::CellValue, Kind::Scalar) => "t then widen name [g], value upper([g]) by [x]",
            (Seat::CellValue, Kind::Aggregate) => "t then widen name [g], value average([x]) by [y]",
            (Seat::CellValue, Kind::Window) => "t then sort [x] then widen name [g], value rank([x]) by [y]",
        }
    }

    fn asked(sentence: &str) -> Result<(), String> {
        let schema = Schema::new([
            ("g", Type::Text),
            ("x", Type::Number),
            ("y", Type::Number),
        ]);
        crate::compile(sentence, &schema, "sql")
            .map(|_| ())
            .map_err(|d| d.message)
    }

    /// Every cell, no cell twice, and each answers the way the table says.
    /// This is the completeness test: a new seat or kind fails `rule_for`'s
    /// total match first and this sweep second, and a checker change that
    /// moves a cell fails here until the table moves with it.
    #[test]
    fn every_cell_of_the_table_is_what_the_checker_does() {
        for &seat in SEATS {
            for &kind in KINDS {
                let sentence = probe(seat, kind);
                let answer = asked(sentence);
                match rule_for(seat, kind) {
                    Admission::Stands => assert!(
                        answer.is_ok(),
                        "the table says {} stands in {} and the checker refused `{}`: {}",
                        format!("{kind:?}").to_lowercase(),
                        seat.word(),
                        sentence,
                        answer.unwrap_err()
                    ),
                    Admission::Refused => assert!(
                        answer.is_err(),
                        "the table says {} is refused in {} and the checker allowed `{}`",
                        format!("{kind:?}").to_lowercase(),
                        seat.word(),
                        sentence
                    ),
                }
            }
        }
    }

    /// The refusals say what to write instead (§3, say what to do): every
    /// refused cell's message carries a backtick — a spelling, not a shrug.
    #[test]
    fn every_refused_cell_offers_a_spelling() {
        for &seat in SEATS {
            for &kind in KINDS {
                if rule_for(seat, kind) == Admission::Refused {
                    let message = asked(probe(seat, kind)).unwrap_err();
                    assert!(
                        message.contains('`'),
                        "the refusal for {} in {} names no spelling: {}",
                        format!("{kind:?}").to_lowercase(),
                        seat.word(),
                        message
                    );
                }
            }
        }
    }

    /// Strange is not a refusal (§3, Law 11). A sentence the seats accept is
    /// answered however pointless: the engine settles well-formed, and
    /// worthwhile belongs to whoever reads the answer.
    #[test]
    fn pointless_but_well_formed_is_answered() {
        for sentence in [
            "t then add [v] as upper(lower(upper([g])))",
            "t then keep where between([x], 400, 100)",
            "t then take 0",
            "t then sort [g] descending, [g]",
            "t then keep where yes",
        ] {
            assert!(
                asked(sentence).is_ok(),
                "refused for taste, which no law permits: `{sentence}`"
            );
        }
    }
}
