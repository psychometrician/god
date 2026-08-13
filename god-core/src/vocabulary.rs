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
}

impl Arity {
    pub fn accepts(self, given: usize) -> bool {
        match self {
            Arity::Exactly(n) => given == n,
            Arity::AtLeast(n) => given >= n,
        }
    }

    /// What it wants, for the message when it does not get it.
    pub fn wanted(self) -> String {
        let count = |n: usize| match n {
            0 => "no columns".to_string(),
            1 => "one column".to_string(),
            n => format!("{n} columns"),
        };
        match self {
            Arity::Exactly(n) => count(n),
            Arity::AtLeast(n) => format!("at least {}", count(n)),
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
    Function { name: "to_number", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "to_whole", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "to_text", kind: Kind::Scalar, arity: Arity::Exactly(1) },
    Function { name: "to_date", kind: Kind::Scalar, arity: Arity::Exactly(1) },

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
    Function { name: "previous", kind: Kind::Window, arity: Arity::Exactly(1) },
    Function { name: "following", kind: Kind::Window, arity: Arity::Exactly(1) },
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
