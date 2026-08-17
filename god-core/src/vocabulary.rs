//! Every word the grammar has, in one place.
//!
//! **This module is the source of truth, and tests enumerate it rather than
//! restating it.** A test that names its own list of verbs stops enforcing
//! anything the moment a verb is added: the vocabulary grows, no assertion
//! moves, and the suite goes on reporting that it covers a set it no longer
//! covers. So anything asserted about the whole vocabulary is asserted by
//! walking these tables (§4).
//!
//! It is also where the growth test bites. A new word costs an entry here, a
//! spelling in every backend, and a chapter — and the test that walks the
//! backends will fail until the second of those is paid.

/// The verbs. One takes a table and returns a table, always (Law 1).
///
/// **Every one is an imperative English verb, with no exceptions.** That is not
/// tidiness: a pipeline is a sequence of instructions, so a name that is a noun
/// or an adjective reads against the mental model of the thing it belongs to.
/// `columns [a, b]` was the one that failed this and is now `pick [a, b]` — the
/// brackets already say the arguments are columns, so naming the verb after them
/// said it twice.
pub const VERBS: &[&str] = &[
    "keep", "pick", "add", "summarize", "sort", "take", "take_last", "join",
    "add_rows",
    "drop_duplicates", "rename", "drop_missing", "fill_missing",
    // Making the absent combinations appear. It is named for what it does to
    // the table — rows arrive underneath — which is why it joins `add_rows`
    // rather than borrowing tidyr's `complete`: that word is read as an
    // adjective far more often than as an imperative, and the vocabulary
    // already renamed `unique` away for exactly that (§11.0).
    "add_combinations",
    // Reshaping. **Direction is in the name, which is Law 4's own example**:
    // nobody could ever remember which of `melt` and `cast` made data taller.
    // They are not `to_long` and `to_wide`, because `to_` marks a conversion
    // between values and `long` is a numeric type in half the languages a
    // reader knows, so `to_long(x)` reads as an integer cast.
    "lengthen", "widen",
];

/// What a function does to the rows it is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Collapses many rows into one: `total`, `row_count`.
    Aggregate,
    /// One value in, one value out, row by row.
    Scalar,
    /// Looks at every row of the group and answers for each one: `rank`,
    /// `row_number`. Neither an aggregate nor a scalar, and the difference is
    /// load-bearing in two places. It may not stand in `summarize`, which
    /// returns one row per group and has no room for an answer per row; and it
    /// may not stand in `keep`, because a window is worked out after the rows
    /// are chosen and cannot decide which rows those are.
    Window,
}

/// How many arguments a function takes.
///
/// **This was a plain count until 2026-08-07, and the comment on it said no
/// function in the grammar is variadic.** That was true and it stopped being
/// true: `first_present(a, b, c)` is a list of places to look, and a list has no
/// natural length.
///
/// The rule the count was protecting is worth restating, because it still
/// holds. What must never vary with the number of arguments is **the meaning**:
/// "it does one thing with two and another with three" is a rule that has to be
/// memorized per word, and no word here does that. `first_present` means the
/// same thing whatever it is given, which is why it may be variadic while
/// `between(x, low, high)` may not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    Exactly(usize),
    /// Two or more, for a function whose arguments are a list rather than named
    /// roles. One would be legal and pointless, so the floor is two.
    AtLeast(usize),
    /// A required count and an optional tail: `previous([x])` and
    /// `previous([x], 2)`.
    ///
    /// **This does not break the rule above it**, which is that the *meaning*
    /// may never vary with the number of arguments. `previous` means the same
    /// thing either way; the second argument says how far back, and leaving it
    /// out means one. That is a default, not a second meaning, and nobody has
    /// to memorize which reading applies.
    Between(usize, usize),
}

impl Arity {
    pub fn accepts(self, given: usize) -> bool {
        match self {
            Arity::Exactly(n) => given == n,
            Arity::AtLeast(n) => given >= n,
            Arity::Between(low, high) => given >= low && given <= high,
        }
    }

    /// What it wants, for the message when it does not get it.
    pub fn wanted(self) -> String {
        let count = |n: usize| match n {
            0 => "no columns".to_string(),
            1 => "one column".to_string(),
            n => format!("{n} columns"),
        };
        // **`Between` says "at most", not "n or m".** Its second argument is not
        // a column — it is how far `previous` looks — so counting columns to
        // describe it would name the wrong thing. The others are left alone.
        match self {
            Arity::Exactly(n) => count(n),
            Arity::AtLeast(n) => format!("at least {}", count(n)),
            Arity::Between(_, high) => format!("at most {}", count(high)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Function {
    pub name: &'static str,
    pub kind: Kind,
    pub arity: Arity,
}

/// The functions, and nothing else is one.
///
/// The plain words are not a stylistic preference. An aggregation has to be the
/// grammar's own word rather than the host's, because the engine underneath has
/// to recognize it as part of the plan — it cannot run a host language's
/// function inside a group-by. `sum` would have to shadow a name that already
/// exists in both R and Python; `total` shadows nothing. **The plain word is
/// also the safe word**, which is the happy case where taste and mechanics
/// agree.
pub const FUNCTIONS: &[Function] = &[
    Function { name: "total", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    Function { name: "average", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    Function { name: "median", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    Function { name: "smallest", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    Function { name: "largest", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    Function { name: "first", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    Function { name: "last", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    Function { name: "unique_count", kind: Kind::Aggregate, arity: Arity::Exactly(1) },
    // Counting takes no argument because it asks about rows rather than about a
    // column. It keeps the one shape every other function has; a bare
    // `row_count` would be the only name in the grammar spelled without its
    // parentheses, and one exception is still an exception.
    //
    // **It is `row_count` and not `rows` because a function is named for the
    // value it produces**, and `rows` named the things being counted instead.
    // A reader could take it for the rows themselves, or for their order. It is
    // not `count_rows` either: that is imperative, which is how a verb is
    // spelled, and `count` is already a verb. `row_count` pairs with
    // `unique_count` and both read as the quantity they return.
    Function { name: "row_count", kind: Kind::Aggregate, arity: Arity::Exactly(0) },

    // The rank family, which dplyr spells six ways: `row_number`, `min_rank`,
    // `dense_rank`, `percent_rank`, `cume_dist` and `ntile`. Two here, and the
    // other four are refused until somebody asks for one (§20.5).
    //
    // **`rank` is the one a person means when they say rank**: ties share a
    // place and the next value skips it, the way a race is scored. That is
    // dplyr's `min_rank` and SQL's `RANK`, and calling it `min_rank` names the
    // implementation rather than the idea.
    //
    // Both are parsed rather than looked up here, because `rank` takes a column
    // in an *ordering* position and may carry `descending`, which no other
    // function does. They are listed so that the vocabulary is one list: a
    // misspelling finds them, and the tests that walk this table cover them.
    Function { name: "rank", kind: Kind::Window, arity: Arity::Exactly(1) },

    Function { name: "row_number", kind: Kind::Window, arity: Arity::Exactly(0) },

    // **The first value that is there, reading left to right.** SQL calls this
    // `coalesce`, which is a term of art: it says nothing to anyone who has not
    // already met it. `first_present` is built from words the grammar owns.
    // `missing` is its word for the absent value, so `present` is the exact
    // complement, and `first` carries the part people get wrong, which is that
    // the arguments are a priority order rather than a set.
    //
    // **The only value it skips is a missing one.** A zero, an empty text and a
    // `no` are all present and come back. That is the other thing people expect
    // wrongly, and it is what the word `present` is for.
    Function { name: "first_present", kind: Kind::Scalar, arity: Arity::AtLeast(2) },

    // **Text put together, which is `split_text` read the other way.** The
    // grammar could take text apart and not join it back until 2026-08-11, an
    // asymmetry no law asked for and one a sweep of the neighbours found rather
    // than a reader. Every one of them has this and every one spells it
    // differently: `unite`, `paste0`, `str.cat`, `concat_str`, `CONCAT`.
    //
    // **A separator is a value, not a clause.** `join_text([first], " ",
    // [last])` says aloud what it does, and the alternative was a trailing
    // `with " "` that would have cost a grammar word to save repeating a space.
    // Variadic for the same reason `first_present` is: the arguments are a
    // sequence rather than a fixed pair.
    //
    // **Missing anywhere makes the answer missing**, which is the rule
    // arithmetic already follows. Engines disagree about this and the
    // disagreement is quiet: DuckDB's `concat` skips a null and returns the
    // rest, so a label built from an absent middle name would come back looking
    // finished. The backends are told to use the spelling that propagates. To
    // fill a hole instead of losing the row, name it: `first_present([x], "")`.
    Function { name: "join_text", kind: Kind::Scalar, arity: Arity::AtLeast(2) },

    // **Case, which is the answer to a question the name tests could not
    // answer.** `pick where name starts "q"` is case-sensitive and misses
    // `Q1_score`, and the fix is not a flag on the test: it is that a name is
    // text, and text has a case. `pick where lower(name) starts "q"` composes
    // out of words §4.7 had already settled, and the same two work on a value.
    //
    // Folding case always would have been wrong, because two columns may differ
    // only by case and the grammar must not pretend otherwise.
    Function { name: "lower", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "upper", kind: Kind::Scalar, arity: Arity::Exactly(1) },

    // **Every conversion begins `to_`, and nothing else does.** It is a prefix
    // meaning "convert into", which is why the reshaping verbs are `lengthen`
    // and `widen`: they convert nothing. Conversion is always explicit here and
    // never a `cast` metaphor, because the day a grammar converts on your behalf
    // is the day a column quietly changes what it holds.
    //
    // **There were four of these and there are three, because `to_whole` was
    // never a conversion.** The grammar has one number type, so it went number
    // to number and converted nothing at all — it was a rounding wearing this
    // prefix, and that misfiling was the whole of why nobody could say which
    // way it went. It is `round_below` and `round_above` now (2026-08-16).
    Function { name: "to_number", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "to_text", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "to_date", kind: Kind::Scalar, arity: Arity::Exactly(1) },

    // **The whole number below, and the whole number above.** Direction is in
    // the name, which is Law 4, and it is in it *literally* rather than as a
    // metaphor: `floor` and `ceiling` are the words every engine uses and they
    // are the term-of-art kind this vocabulary has turned down three times —
    // `lag`/`lead` became `previous`/`following`, `coalesce` became
    // `first_present`, `melt`/`cast` became `lengthen`/`widen`.
    //
    // **`below` and `above` rather than `down` and `up`, and that is not
    // taste.** Excel's `ROUNDDOWN` goes toward zero, so `ROUNDDOWN(-5.5)` is -5
    // there and this is -6. Naming these `round_down`/`round_up` would have
    // meant one word and two answers against the most used data tool there is,
    // with nothing raised. On a number line "below" has no such second reading.
    //
    // **Neither needs a convention named for it, and that is why this pair won
    // over `round`.** Measured on all five engines: floor and ceiling agree
    // everywhere, negatives included. `round` does not — R and Python break a
    // tie to the even number and DuckDB breaks it away from zero — so it would
    // have been a third `weekday`. The nearest whole number is
    // `round_below([x] + 0.5)`, a composition, so Law 5 refuses a word for it.
    Function { name: "round_below", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "round_above", kind: Kind::Scalar, arity: Arity::Exactly(1) },

    // The string functions past case. **The `_text` suffix on two of them is not
    // decoration**: `base::replace` replaces elements of a vector by position and
    // `base::split` divides one by group, so both would read as one thing and do
    // another. `to_` as a prefix means "convert into" and `_text` as a suffix
    // means "operating on text", so the two never collide.
    Function { name: "trim", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "replace_text", kind: Kind::Scalar, arity: Arity::Exactly(3) },

    // **Three arguments rather than the two §4.7 wrote**, and the reason is that
    // the grammar has no list. Splitting text gives several pieces and every
    // value here is one value, so it says which piece: `split_text([name], " ",
    // 1)` is the first word. Both engines spell it `split_part` and mean exactly
    // this. A form returning a list would need a fifth kind of value in the type
    // system, which is a much larger thing to buy than one argument.
    Function { name: "split_text", kind: Kind::Scalar, arity: Arity::Exactly(3) },

    // How long the text is. **`characters` rather than `length`**, because
    // masking is only honest where the masked name means the same thing: R's
    // `length` counts elements of a vector, so `length([name])` in a pipeline
    // would read as one thing and do another.
    Function { name: "characters", kind: Kind::Scalar, arity: Arity::Exactly(1) },

    // **Three named roles, so it may not be variadic**, which is the rule
    // `Arity`'s own comment states. It is inclusive at both ends, the way SQL's
    // `BETWEEN` and dplyr's `between` both are, so nobody arriving from either
    // has to check.
    Function { name: "between", kind: Kind::Scalar, arity: Arity::Exactly(3) },

    // **What is left over after dividing.** Added 2026-08-16, and it is the one
    // arithmetic operator with no composition in the grammar: every other gap
    // on dplyr's math shelf can be written with what is already here — integer
    // division is `([x] - remainder([x], n)) / n` once this exists, and a square
    // is `[x] * [x]` — so Law 5 refuses a word for those and this one earns it.
    //
    // **`remainder` rather than `%%` or `mod`.** The grammar has no operator
    // punctuation past arithmetic, `%%` is R's alone, and `mod` is an
    // abbreviation of a word nobody says out loud. "The remainder" is what this
    // is called in English before it is called anything in a language.
    //
    // **The sign is named rather than inherited, and this is the second
    // `weekday`.** Asked plainly, R, Python, pandas and polars all answer 1 for
    // `-7 % 2` and DuckDB and Spark both answer -1, with nothing raised. So the
    // grammar names the floored convention — the answer takes the divisor's
    // sign — because that is the one that makes bucketing work, and each SQL
    // dialect is given `((a % b) + b) % b`, which produces it on both.
    Function { name: "remainder", kind: Kind::Scalar, arity: Arity::Exactly(2) },

    // The parts of a date. Four of the five are spelled the same by every
    // engine; `weekday` is not, and it is the reason this family needed
    // measuring rather than writing.
    Function { name: "year", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "month", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "day", kind: Kind::Scalar, arity: Arity::Exactly(1) },

    // **Monday is 1, and that is a decision rather than an engine's default.**
    // Asked plainly, DuckDB answers 5 for a Friday and Spark answers 4, and
    // neither raises: the same sentence, two answers, no complaint. So the
    // grammar names the numbering it means, ISO 8601's, and each dialect is
    // given the spelling that produces it.
    Function { name: "weekday", kind: Kind::Scalar, arity: Arity::Exactly(1) },

    Function { name: "hour", kind: Kind::Scalar, arity: Arity::Exactly(1) },

    // The rest of the windowed family. **All three have to be told the order**,
    // the way `row_number` does and `rank` does not: a running total is a total
    // *so far*, and "so far" means nothing until something says in what order.
    Function { name: "running_total", kind: Kind::Window, arity: Arity::Exactly(1) },

    // `lag` and `lead` are the words everywhere else, and nobody can say which
    // way `lead` goes without checking. These two can be read aloud.
    //
    // **How far back is an optional second argument**, added 2026-08-16 because
    // one row back is the common case and not the only one: a year-over-year
    // comparison on monthly rows is `previous([revenue], 12)`, and every
    // neighbour can already say it — `shift(x, 12)`, `lag(x, n = 12)`,
    // `.shift(12)`, `LAG(x, 12)`. god was the only one that could not.
    //
    // **It is a plain whole number, the way `split_text([name], " ", 1)` takes a
    // position.** It has to be written out rather than computed: a per-row
    // offset is a different operation, and no engine underneath takes a column
    // there. The checker refuses anything but a literal, and refuses 0 and
    // negatives by name — 0 is the column itself and a negative is the other
    // word.
    Function { name: "previous", kind: Kind::Window, arity: Arity::Between(1, 2) },
    Function { name: "following", kind: Kind::Window, arity: Arity::Between(1, 2) },

    // **The last value that was there, reading down.** tidyr calls the verb
    // `fill`, pandas `ffill`, polars `forward_fill`, and the term of art is
    // "last observation carried forward" — which is exactly the kind of phrase
    // §14.1 refuses, because it has to be learned rather than read.
    //
    // `latest` reads correctly on the row that needs it: if this row's value is
    // missing, the latest one you have is the earlier one.
    //
    // **It is a window in `add` rather than a filler in `fill_missing`, and
    // that is deliberate.** §14 refused a window in the filler's seat because a
    // value walking the rows needs their order declared and a filler has no
    // place to ask for a `sort`. `add` already demands one for every window, so
    // this reaches the same answer without reopening the ruling.
    //
    // **It also replaces a workaround that was quietly wrong.** The refusal
    // above told people to write `first_present([x], previous([x]))`, which
    // looks back exactly one row, so a run of two holes left the second one
    // open. Nothing said so.
    Function { name: "latest", kind: Kind::Window, arity: Arity::Exactly(1) },
];

pub fn lookup(name: &str) -> Option<&'static Function> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

pub fn is_aggregate(name: &str) -> bool {
    matches!(lookup(name), Some(f) if f.kind == Kind::Aggregate)
}

pub fn is_window(name: &str) -> bool {
    matches!(lookup(name), Some(f) if f.kind == Kind::Window)
}

/// Words that carry grammar, so a bare one of these is never a value.
///
/// A column called `sort` needs no escape and no backtick: inside `[ ]` it is a
/// column, outside it is grammar, and that one rule covers every collision there
/// is (§13.2). This list exists so a message can say *why* a word was not read
/// as a name, not to restrict what anyone may call a column.
pub const GRAMMAR_WORDS: &[&str] = &[
    "then", "where", "as", "by", "all_but", "descending", "in", "is", "not", "and", "or", "yes",
    "no", "missing", "unmatched",
    // The three text tests, and the word for a column's own name. `name` is
    // grammar only outside brackets: a column actually called `name` is
    // `[name]`, and that one rule covers every collision there is.
    "starts", "ends", "contains", "name", "value",
    // What a column holds. `kind` rather than `type`, because `type` is a
    // Python builtin and shadowing it under `from god import *` would cost
    // something for nothing. The four answers are written as quoted text, the
    // way `unmatched "both"` already writes a fixed set of choices, so they
    // are values rather than words and none of them is listed here.
    "kind",
    // What `widen` produces, written down so that the steps after it can be
    // checked. It is a participle rather than an imperative, which is the rule
    // every marker here follows — `descending`, `by`, `all_but` — so it does not
    // read as a verb and does not invite `sales |> giving(...)`.
    "giving",
    // The conditional, and the word for its catch-all. Both are grammar rather
    // than functions for the reason `matching` is: `when` reads its arguments in
    // pairs and may end with an `otherwise`, so `name([column])` is not a
    // sentence it has, and the table that walks the functions builds exactly
    // that.
    //
    // **`if` is deliberately not the word**, and not for taste. `"A" if [x] > 1`
    // reads better and neither host can say it: R cannot parse a postfix `if`,
    // and Python parses it, calls `__bool__` on the expression, and picks a
    // branch while the sentence is still being built (§19.2).
    "when", "otherwise",
    // `matching` is read as grammar rather than looked up as a function,
    // because its first argument is a table. It belongs here rather than in
    // FUNCTIONS for that reason: the table that walks the functions builds
    // `name([column])` for each, which is not a sentence `matching` has.
    "matching",
    // **The two quantifiers, for a condition asked of many columns at once.**
    // `keep where any name starts "q" as value > 3`. dplyr spells these
    // `when_any` and `when_all`, having renamed them from `if_any`/`if_all`;
    // pandas and polars both spell them `.any(axis=1)` and `.all(axis=1)`.
    //
    // The selector is the same `where name ...` that `pick`, `add`, `summarize`
    // and `lengthen` already take, so the only thing new is which way the
    // matched conditions are joined — `or` for `any`, `and` for `every`. That
    // asymmetry was the gap: four verbs took the selector and `keep` did not,
    // for no reason anybody had written down.
    //
    // **`every` rather than `all`, because `all_but` is already a word.** The
    // two would not collide as tokens, but a reader meeting `all` beside
    // `all_but` has to stop and ask whether they are a pair, and they are not.
    "any", "every",
    // **What `take` keeps at the cut.** `sort [points] descending then take 3
    // with ties` keeps every row tied with the third. Without it `take 3` means
    // exactly three rows, which stays the default because a sentence whose row
    // count cannot be read off it should have to say so.
    //
    // dplyr's `slice_max` has this the other way round — `with_ties = TRUE` is
    // its default — so the same request written in the two tools gave different
    // rows and neither said anything. That is the silent disagreement §3.1
    // exists to refuse, and it is why this is a word rather than a footnote.
    "with", "ties",
];

/// `a, b or c`, for a message that has to offer a closed set of words.
pub fn list_or(words: &[&str]) -> String {
    let quoted: Vec<String> = words.iter().map(|w| format!("`\"{w}\"`")).collect();
    match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The step separator, which may never appear inside an expression.
///
/// This is the one reserved word the grammar cannot bend on, and it was learned
/// by breaking it: a conditional phrased `when [score] >= 90 then "A"` reused
/// the word that divides steps, and the splitter tore the expression in half.
/// The rule that came out of it is that a word doing structural work does that
/// work and nothing else.
pub const FLOW_WORD: &str = "then";

/// Every name in the grammar, in every role, for the audit that has to run
/// whenever a word is added: any name appearing twice is a defect until someone
/// argues for it in writing.
pub fn all_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = VERBS.to_vec();
    names.extend(FUNCTIONS.iter().map(|f| f.name));
    names.extend(GRAMMAR_WORDS.iter().copied());
    names
}
