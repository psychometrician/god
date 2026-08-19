//! Text to a plan.
//!
//! **There is one parser, and that is the point.** A grammar spelled natively in
//! each host needs machinery to prove the hosts still agree; a grammar that is
//! one text read by one parser cannot disagree with itself, so the machinery has
//! nothing to do. Every host hands the same characters to this function.
//!
//! The delimiters each do exactly one job, and between them they remove every
//! collision a grammar of this kind normally has to legislate around:
//!
//! | | Means |
//! |---|---|
//! | `[ … ]` | a column, or a list of columns |
//! | `" … "` | a text value |
//! | `( … )` | a group, and a name in front of one applies that name to it |
//! | `{ … }` | a set of values to match |
//!
//! **Inside `[ ]` it is a column, always. Outside, it is grammar.** That single
//! rule is why a column may be called `sort`, or `then`, or `total`, with no
//! backtick, no escape word, and no list of names anyone is forbidden to use.
//!
//! `( )` grouping and `( )` around a function's arguments are one job rather than
//! two: the brackets delimit a sub-expression, and a name written in front of
//! them applies that name to what is inside.

use crate::diagnostic::{nearest, Diagnostic};
use crate::plan::*;
use crate::vocabulary::{self, FLOW_WORD};

pub fn parse(source: &str) -> Result<Plan, Diagnostic> {
    let tokens = lex(source)?;
    Parser { tokens, at: 0, source }.plan()
}

// ---------------------------------------------------------------------------
// Words and marks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    /// `[a]` or `[a, b]`. Read whole by the lexer, because what is inside the
    /// brackets is a name exactly as written and must not be tokenized as
    /// grammar — that is what lets a column be called `then`.
    Columns(Vec<Name>),
    Text(String),
    Whole(i64),
    Decimal(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    OpenParen,
    CloseParen,
    OpenBrace,
    CloseBrace,
    Comma,
    /// The separator between the parts of a qualified table name.
    ///
    /// **It is a token rather than part of a word, and that is the whole
    /// design.** A dot joins names only where a *table* is named: the head of a
    /// pipeline, `join`, `add_rows` and `matching`. Those four positions read a
    /// name rather than an expression, so nothing about `.` leaks into the
    /// expression grammar, and `[a.b]` or `total(x.y)` is refused exactly as it
    /// was before. Lexing `a.b` as one word would have given the dot a meaning
    /// everywhere.
    Dot,
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    span: Span,
}

fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    let bytes: Vec<char> = source.chars().collect();
    // Byte offset of each character, so a span is a byte range into the original
    // string and non-ASCII column names do not shift the caret.
    let offsets: Vec<usize> = source.char_indices().map(|(i, _)| i).collect();
    let byte_at = |i: usize| offsets.get(i).copied().unwrap_or(source.len());

    let mut out = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // A comment runs to the end of the line. `#` is the one host-neutral
        // comment mark: R and Python already agree on it, and SQL's `--` would
        // collide with a minus.
        if c == '#' {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }

        let start = byte_at(i);

        if c == '[' {
            let open = i;
            i += 1;
            let mut names = Vec::new();
            let mut piece_start = i;
            let mut closed = false;
            while i < bytes.len() {
                match bytes[i] {
                    ']' => {
                        push_name(&bytes, &offsets, source, piece_start, i, &mut names);
                        closed = true;
                        i += 1;
                        break;
                    }
                    ',' => {
                        push_name(&bytes, &offsets, source, piece_start, i, &mut names);
                        i += 1;
                        piece_start = i;
                    }
                    '[' => {
                        return Err(Diagnostic::illegal(
                            "a column bracket cannot contain another one. Write `[name]`, and separate several names with commas: `[a, b]`",
                            Span::new(byte_at(i), 1),
                        ));
                    }
                    _ => i += 1,
                }
            }
            if !closed {
                return Err(Diagnostic::illegal(
                    "this column bracket is never closed. Add a `]` after the column name",
                    Span::new(byte_at(open), 1),
                ));
            }
            if names.iter().any(|n| n.text.is_empty()) {
                return Err(Diagnostic::illegal(
                    "a column bracket is empty. Write the column's name between the brackets",
                    Span::new(start, byte_at(i) - start),
                ));
            }
            out.push(Token { tok: Tok::Columns(names), span: Span::new(start, byte_at(i) - start) });
            continue;
        }

        if c == '"' {
            i += 1;
            let mut value = String::new();
            let mut closed = false;
            while i < bytes.len() {
                if bytes[i] == '"' {
                    closed = true;
                    i += 1;
                    break;
                }
                value.push(bytes[i]);
                i += 1;
            }
            if !closed {
                return Err(Diagnostic::illegal(
                    "this text value is never closed. Add a `\"` at the end of it",
                    Span::new(start, 1),
                ));
            }
            out.push(Token { tok: Tok::Text(value), span: Span::new(start, byte_at(i) - start) });
            continue;
        }

        if c.is_ascii_digit() {
            let mut seen_dot = false;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || (bytes[i] == '.' && !seen_dot)) {
                if bytes[i] == '.' {
                    seen_dot = true;
                }
                i += 1;
            }
            let raw = &source[start..byte_at(i)];
            let tok = if seen_dot {
                Tok::Decimal(raw.parse::<f64>().map_err(|_| {
                    Diagnostic::illegal(
                        format!("`{raw}` is not a number the grammar can read. Write digits and at most one point: `10`, or `10.5`"),
                        Span::new(start, byte_at(i) - start),
                    )
                })?)
            } else {
                Tok::Whole(raw.parse::<i64>().map_err(|_| {
                    Diagnostic::illegal(
                        format!("`{raw}` is too large a whole number to read"),
                        Span::new(start, byte_at(i) - start),
                    )
                })?)
            };
            out.push(Token { tok, span: Span::new(start, byte_at(i) - start) });
            continue;
        }

        if c.is_alphabetic() || c == '_' {
            while i < bytes.len() && (bytes[i].is_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            let word = source[start..byte_at(i)].to_string();
            out.push(Token { tok: Tok::Word(word), span: Span::new(start, byte_at(i) - start) });
            continue;
        }

        let (tok, width) = match c {
            '+' => (Tok::Plus, 1),
            '-' => (Tok::Minus, 1),
            '*' => (Tok::Star, 1),
            '/' => (Tok::Slash, 1),
            '(' => (Tok::OpenParen, 1),
            ')' => (Tok::CloseParen, 1),
            '{' => (Tok::OpenBrace, 1),
            '}' => (Tok::CloseBrace, 1),
            ',' => (Tok::Comma, 1),
            // Only the four places that name a table join across one. Anywhere
            // else the parser reports it, and it says the same thing the lexer
            // used to.
            '.' => (Tok::Dot, 1),
            '<' if bytes.get(i + 1) == Some(&'>') => {
                return Err(Diagnostic::illegal(
                    "the grammar writes this as the words `is not`, so that one spelling works in every language. Write `is not` instead of `<>`",
                    Span::new(start, 2),
                ));
            }
            '<' if bytes.get(i + 1) == Some(&'=') => (Tok::LessOrEqual, 2),
            '>' if bytes.get(i + 1) == Some(&'=') => (Tok::GreaterOrEqual, 2),
            '<' => (Tok::Less, 1),
            '>' => (Tok::Greater, 1),
            '=' => {
                return Err(Diagnostic::illegal(
                    "the grammar writes equality as the word `is`, so that one spelling works in every language. Write `is` instead of `=`",
                    Span::new(start, 1),
                ));
            }
            '!' => {
                return Err(Diagnostic::illegal(
                    "the grammar writes negation as the word `not`. Write `is not` instead of `!=`, and `not` instead of `!`",
                    Span::new(start, 1),
                ));
            }
            '&' | '|' => {
                let word = if c == '&' { "and" } else { "or" };
                return Err(Diagnostic::illegal(
                    format!("the grammar writes this as the word `{word}`, so that one spelling works in every language"),
                    Span::new(start, 1),
                ));
            }
            // SQL writes text in single quotes and Python accepts either, so
            // this is the habit most likely to arrive from somewhere else. The
            // grammar takes one spelling and only one — two ways to write a text
            // value is exactly what Law 2 refuses — so the message names the one
            // it takes and, where it can, hands back the same value spelled
            // correctly.
            '\'' => {
                let closing = bytes[i + 1..].iter().position(|&b| b == '\'');
                let suggestion = match closing {
                    Some(n) => {
                        let value: String = bytes[i + 1..i + 1 + n].iter().collect();
                        format!(" Write `\"{value}\"`")
                    }
                    None => " Write `\"…\"`".to_string(),
                };
                return Err(Diagnostic::illegal(
                    format!(
                        "the grammar writes a text value with double quotes, and only double quotes, so there is one spelling rather than two.{suggestion}"
                    ),
                    Span::new(start, 1),
                ));
            }
            _ => {
                return Err(Diagnostic::illegal(
                    format!("`{c}` means nothing in the grammar. A column goes in brackets, a text value in double quotes, and a set in braces"),
                    Span::new(start, c.len_utf8()),
                ));
            }
        };
        i += width;
        out.push(Token { tok, span: Span::new(start, byte_at(i) - start) });
    }

    Ok(out)
}

/// The spelling a person arrived with, and the one word the grammar takes.
///
/// R writes `TRUE` and `NA`, Python writes `True` and `None`, SQL writes `TRUE`
/// and `NULL`. **The grammar takes one of each**, because a value that can be
/// written three ways is three things to read. This table is what turns "unknown
/// word" into "write this instead".
fn neutral_word_for(word: &str) -> Option<&'static str> {
    match word.to_ascii_lowercase().as_str() {
        "true" | "t" => Some("yes"),
        "false" | "f" => Some("no"),
        "null" | "na" | "none" | "nan" | "nil" | "nothing" => Some("missing"),
        _ => None,
    }
}

fn push_name(
    bytes: &[char],
    offsets: &[usize],
    source: &str,
    from: usize,
    to: usize,
    into: &mut Vec<Name>,
) {
    let byte_at = |i: usize| offsets.get(i).copied().unwrap_or(source.len());
    // Trim by character so the span still points at the name itself and not at
    // the whitespace someone left around it.
    let mut a = from;
    let mut b = to;
    while a < b && bytes[a].is_whitespace() {
        a += 1;
    }
    while b > a && bytes[b - 1].is_whitespace() {
        b -= 1;
    }
    let text = source[byte_at(a)..byte_at(b)].to_string();
    into.push(Name { text, span: Span::new(byte_at(a), byte_at(b) - byte_at(a)) });
}

// ---------------------------------------------------------------------------
// Sentences
// ---------------------------------------------------------------------------

struct Parser<'a> {
    tokens: Vec<Token>,
    at: usize,
    source: &'a str,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.at).map(|t| &t.tok)
    }

    fn peek_span(&self) -> Span {
        self.tokens
            .get(self.at)
            .map(|t| t.span)
            .unwrap_or_else(|| Span::new(self.source.len().saturating_sub(1), 1))
    }

    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.at).cloned();
        if t.is_some() {
            self.at += 1;
        }
        t
    }

    fn at_word(&self, word: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(w)) if w == word)
    }

    /// A table name that has already had its first part read, plus any `.part`
    /// that follows it.
    ///
    /// **A catalog names a table in more than one piece and the grammar has to
    /// be able to say so**, or a warehouse table is unreachable and the answer
    /// is a quoted string, which is the escape hatch this grammar exists to
    /// avoid. `main.sales.orders` is one name with three parts, not an
    /// expression, so nothing here reaches the value grammar.
    ///
    /// Each part is an ordinary word, which is what keeps the rule small: a
    /// backend quotes the parts separately, and a dot inside a part is not
    /// something any catalog allows anyway.
    fn qualified(&mut self, first: String, first_span: Span) -> Result<(String, Span), Diagnostic> {
        let mut name = first;
        let mut span = first_span;
        while matches!(self.peek(), Some(Tok::Dot)) {
            let dot = self.peek_span();
            // **The word after the dot is looked at before the dot is taken.**
            // `then` is an ordinary word to the lexer, so `sales. then take 3`
            // would otherwise be read as a table called `sales.then`, and the
            // error would land on the next step complaining that `then` is
            // missing. The grammar's own words can never be part of a name.
            let following = match self.tokens.get(self.at + 1).map(|t| &t.tok) {
                Some(Tok::Word(w))
                    if w != FLOW_WORD && !vocabulary::GRAMMAR_WORDS.contains(&w.as_str()) =>
                {
                    w.clone()
                }
                _ => {
                    return Err(Diagnostic::illegal(
                        format!("`{name}.` names a table in parts, so a part has to follow the dot: `{name}.orders`"),
                        dot,
                    ));
                }
            };
            self.at += 2;
            let part_span = self.tokens[self.at - 1].span;
            name.push('.');
            name.push_str(&following);
            span = span.to(part_span);
        }
        Ok((name, span))
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if self.at_word(word) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect_word(&mut self, word: &str, why: &str) -> Result<Span, Diagnostic> {
        if self.at_word(word) {
            let span = self.peek_span();
            self.at += 1;
            Ok(span)
        } else {
            Err(Diagnostic::illegal(why.to_string(), self.peek_span()))
        }
    }

    fn expect_columns(&mut self, why: &str) -> Result<Vec<Name>, Diagnostic> {
        match self.peek() {
            Some(Tok::Columns(_)) => {
                let Some(Token { tok: Tok::Columns(names), .. }) = self.next() else {
                    unreachable!()
                };
                Ok(names)
            }
            _ => Err(Diagnostic::illegal(why.to_string(), self.peek_span())),
        }
    }

    /// The columns that say which rows of two tables correspond.
    ///
    /// **One parser for `join` and for `matching`**, because it is one question
    /// and a grammar that answered it two ways would have the exception §4.4
    /// spends a paragraph refusing.
    ///
    /// Three shapes, and the third is what was added on 2026-08-16:
    ///
    /// ```text
    /// by [id]                          one key, the same word on both sides
    /// by [region, product]             several, all the same word
    /// by [customer_id] is [id]         one key, named differently on each side
    /// by [region], [customer_id] is [id]      and the two mixed
    /// ```
    ///
    /// `is` is the grammar's equality word everywhere else (§2.4), so nothing
    /// was added to the vocabulary to reach this.
    fn join_keys(&mut self, why: &str) -> Result<Vec<JoinKey>, Diagnostic> {
        let mut keys = Vec::new();
        loop {
            let span = self.peek_span();
            let names = self.expect_columns(why)?;
            if self.eat_word("is") {
                // **The pair is one column against one column.** `by [a, b] is
                // [c, d]` would be two keys wearing one `is`, and which pairs
                // with which is exactly the thing a reader should not have to
                // work out by counting.
                let other_span = self.peek_span();
                let others = self.expect_columns(
                    "`is` needs the other table's column, in brackets: `by [customer_id] is [id]`",
                )?;
                if names.len() != 1 || others.len() != 1 {
                    return Err(Diagnostic::illegal(
                        "a key named differently on each side is one column against one column, so each side gets its own brackets: `by [customer_id] is [id]`. For several, separate them with commas",
                        span.to(other_span),
                    ));
                }
                keys.push(JoinKey {
                    this: names.into_iter().next().unwrap(),
                    other: others.into_iter().next().unwrap(),
                });
            } else {
                keys.extend(names.into_iter().map(JoinKey::same));
            }
            if !matches!(self.peek(), Some(Tok::Comma)) {
                break;
            }
            self.at += 1;
        }
        Ok(keys)
    }

    fn one_column(&mut self, why: &str) -> Result<Name, Diagnostic> {
        let span = self.peek_span();
        let names = self.expect_columns(why)?;
        if names.len() != 1 {
            let listed: Vec<String> =
                names.iter().map(|n| format!("[{}]", n.text)).collect();
            return Err(Diagnostic::illegal(
                format!(
                    "each key gets its own brackets here, so that each can have its own direction. Write `{}`",
                    listed.join(", ")
                ),
                span,
            ));
        }
        Ok(names.into_iter().next().unwrap())
    }

    /// `name <pattern>, value <thing>` — the pair both reshaping verbs take.
    ///
    /// **One reader, because the two verbs read the same two words and differ
    /// only in which way they flow.** `name` always points at the column holding
    /// column names and `value` at the column holding values; `lengthen` makes
    /// that pair and `widen` consumes it, and the verb is what says which.
    ///
    /// `widen`'s value is a whole expression, because an aggregate there is what
    /// answers "two rows want the same cell" without spending a word on it.
    /// `lengthen`'s is a column being created, so it is only ever a name.
    fn naming(
        &mut self,
        verb: &str,
        value_is_expression: bool,
    ) -> Result<(Option<Pattern>, Option<Expr>, Span), Diagnostic> {
        let mut name = None;
        let mut value = None;
        // Every path through the loop below either sets this or returns, which
        // is what lets it have no initial value to be mistaken for one.
        let mut end;
        loop {
            let at = self.peek_span();
            if self.eat_word("name") {
                if name.is_some() {
                    return Err(Diagnostic::illegal("`name` is already here once", at));
                }
                let p = self.pattern(verb)?;
                end = p.span;
                name = Some(p);
            } else if self.eat_word("value") {
                if value.is_some() {
                    return Err(Diagnostic::illegal("`value` is already here once", at));
                }
                let e = if value_is_expression {
                    self.expression()?
                } else {
                    Expr::Column(self.one_column(&format!(
                        "`value` names the column that will hold the values, in brackets: `{verb} ... value [answer]`"
                    ))?)
                };
                end = e.span();
                value = Some(e);
            } else {
                return Err(Diagnostic::illegal(
                    format!(
                        "`{verb}` names its two columns with `name` and `value`: `name [question], value [answer]`"
                    ),
                    at,
                ));
            }
            if !matches!(self.peek(), Some(Tok::Comma)) {
                break;
            }
            self.at += 1;
        }
        Ok((name, value, end))
    }

    /// Either `[question]` or `"{question}_{year}"`.
    ///
    /// The bracketed form is the one-part case of the quoted one rather than a
    /// second shape, so there is one idea here and a shorthand for the common
    /// use of it.
    fn pattern(&mut self, verb: &str) -> Result<Pattern, Diagnostic> {
        let why = format!(
            "`name` takes the column, in brackets, or the shape of the old names in quotes: `{verb} ... name [question]` or `name \"{{question}}_{{year}}\"`"
        );
        match self.next() {
            Some(Token { tok: Tok::Columns(names), span }) => {
                if names.len() != 1 {
                    return Err(Diagnostic::illegal(
                        "`name` takes one column. To split an old name into several, say what the old names look like: `name \"{question}_{year}\"`",
                        span,
                    ));
                }
                Ok(Pattern::single(names.into_iter().next().unwrap().text, span))
            }
            Some(Token { tok: Tok::Text(text), span }) => read_pattern(&text, span),
            Some(Token { span, .. }) => Err(Diagnostic::illegal(why, span)),
            None => Err(Diagnostic::illegal(why, self.peek_span())),
        }
    }

    // -- the pipeline ------------------------------------------------------

    fn plan(&mut self) -> Result<Plan, Diagnostic> {
        let (source, source_span) = match self.next() {
            Some(Token { tok: Tok::Word(w), span }) if w != FLOW_WORD => {
                self.qualified(w, span)?
            }
            Some(Token { tok: Tok::Columns(_), span }) => {
                return Err(Diagnostic::illegal(
                    "a pipeline starts with the name of a table, written plainly. Brackets are for columns",
                    span,
                ));
            }
            Some(Token { span, .. }) => {
                return Err(Diagnostic::illegal(
                    "a pipeline starts with the name of a table, then what happens to it: `sales then take 10`",
                    span,
                ));
            }
            None => {
                return Err(Diagnostic::illegal(
                    "a pipeline starts with the name of a table, and this one is empty. Write one: `sales then take 10`",
                    Span::new(0, 1),
                ));
            }
        };

        let mut steps = Vec::new();
        while self.at < self.tokens.len() {
            let then_span = self.peek_span();
            if !self.eat_word(FLOW_WORD) {
                // A word that is the grammar's own, shouted. SQL is written in
                // capitals by habit, so `IS` and `AND` arrive often, and blaming
                // a missing `then` sends the reader to the wrong end of the line.
                if let Some(Tok::Word(w)) = self.peek() {
                    let lower = w.to_ascii_lowercase();
                    if *w != lower && vocabulary::GRAMMAR_WORDS.contains(&lower.as_str()) {
                        return Err(Diagnostic::illegal(
                            format!("the grammar's words are lowercase. Write `{lower}` instead of `{w}`"),
                            then_span,
                        ));
                    }
                }
                return Err(Diagnostic::illegal(
                    format!("steps are joined by the word `{FLOW_WORD}`, and one is missing here"),
                    then_span,
                ));
            }
            steps.push(self.step()?);
        }

        Ok(Plan { source, source_span, steps })
    }

    fn step(&mut self) -> Result<Step, Diagnostic> {
        let start = self.peek_span();
        let Some(Tok::Word(word)) = self.peek().cloned() else {
            return Err(Diagnostic::illegal(
                format!("`{FLOW_WORD}` has to be followed by a verb: {}", vocabulary::VERBS.join(", ")),
                start,
            ));
        };

        if !vocabulary::VERBS.contains(&word.as_str()) {
            let suggestion = nearest(&word, vocabulary::VERBS.iter().copied())
                .map(|s| format!(" Did you mean `{s}`?"))
                .unwrap_or_default();
            return Err(Diagnostic::illegal(
                format!(
                    "`{word}` is not one of the verbs.{suggestion} The verbs are: {}",
                    vocabulary::VERBS.join(", ")
                ),
                start,
            ));
        }
        self.at += 1;

        match word.as_str() {
            "keep" => {
                self.expect_word(
                    "where",
                    "`keep` reads as a sentence: `keep where [column] is \"value\"`. The word `where` is missing",
                )?;
                // **`any` and `every` ask one question of many columns**, so
                // they are read here rather than inside `expression`: what
                // follows is a column *selector* and then a test, which is the
                // shape `add where ... as ...` takes, not the shape an ordinary
                // condition takes.
                let condition = if self.at_word("any") || self.at_word("every") {
                    let at = self.peek_span();
                    let every = self.at_word("every");
                    self.at += 1;
                    let selector = self.expression()?;
                    self.expect_word(
                        "as",
                        "a question asked of many columns needs the question after `as`: `keep where any name starts \"q\" as value > 3`",
                    )?;
                    let test = self.expression()?;
                    Expr::Quantified {
                        every,
                        span: at.to(test.span()),
                        selector: Box::new(selector),
                        test: Box::new(test),
                    }
                } else {
                    self.expression()?
                };
                let span = start.to(condition.span());
                Ok(Step::Keep { condition, span })
            }

            "pick" => {
                // The words someone arrives with for this. `except` was the
                // grammar's own until 2026-08-07, and SQL and dplyr supply the
                // rest, so all of them reach for the same slot and all of them
                // get told the one word that works.
                for habit in ["except", "exclude", "excluding", "drop", "omit", "without"] {
                    if self.at_word(habit) {
                        return Err(Diagnostic::illegal(
                            format!(
                                "the grammar writes this as `all_but`, so there is one spelling rather than one per language. Write `pick all_but [a, b]` instead of `pick {habit} [a, b]`"
                            ),
                            self.peek_span(),
                        ));
                    }
                }

                // `pick where …` chooses by the shape of a name instead of by
                // listing them. `where` is already the word that introduces a
                // condition, so this adds no construct.
                if self.eat_word("where") {
                    let condition = self.expression()?;
                    let span = start.to(condition.span());
                    return Ok(Step::Pick {
                        names: Vec::new(),
                        all_but: false,
                        condition: Some(condition),
                        span,
                    });
                }

                let all_but = self.eat_word("all_but");
                let names = self.expect_columns(
                    "`pick` needs the columns to choose, in brackets: `pick [a, b]`, or by the shape of the name: `pick where name starts \"q\"`",
                )?;
                if matches!(self.peek(), Some(Tok::Comma)) {
                    let mut listed: Vec<String> =
                        names.iter().map(|n| n.text.clone()).collect();
                    listed.push("…".into());
                    return Err(Diagnostic::illegal(
                        format!(
                            "`pick` takes one list of columns in one bracket. Write `pick [{}]`",
                            listed.join(", ")
                        ),
                        self.peek_span(),
                    ));
                }
                let span = start.to(names.last().map(|n| n.span).unwrap_or(start));
                Ok(Step::Pick { names, all_but, condition: None, span })
            }

            "rename" | "fill_missing" => {
                // The same `[name] as <value>` shape `add` uses, which is the
                // whole reason these two verbs need no syntax of their own.
                let mut values = Vec::new();
                loop {
                    let name = self.one_column(&format!(
                        "`{word}` names the column, in brackets: `{word} [name] as ...`"
                    ))?;
                    self.expect_word(
                        "as",
                        &format!("`{word} [{}]` has to say what it is: write `as` and then the value", name.text),
                    )?;
                    let value = self.expression()?;
                    values.push(Named { name, value });
                    if !matches!(self.peek(), Some(Tok::Comma)) {
                        break;
                    }
                    self.at += 1;
                }
                let end = values.last().map(|v| v.value.span()).unwrap_or(start);
                let span = start.to(end);
                if word == "rename" {
                    Ok(Step::Rename { values, span })
                } else {
                    Ok(Step::FillMissing { values, span })
                }
            }

            "drop_duplicates" => {
                // **Refusing is easy; refusing well is the part that matters.**
                // Naming a subset is the commonest thing anyone will try here,
                // because both pandas and dplyr let them, and the two mean
                // different things by it. Left to the general "unexpected
                // token" message, a reader would think the grammar was broken
                // rather than that it wanted them to say which they meant.
                if let Some(Tok::Columns(names)) = self.peek() {
                    let listed: Vec<String> =
                        names.iter().map(|n| n.text.clone()).collect();
                    let joined = listed.join(", ");
                    return Err(Diagnostic::illegal(
                        format!(
                            "`drop_duplicates` takes no columns, because naming some means two different things and one of them has no answer. For the distinct values of {joined}: `pick [{joined}] then drop_duplicates`. To keep whole rows, one per group: `sort [...] then take 1 by [{joined}]`",
                        ),
                        self.peek_span(),
                    ));
                }
                Ok(Step::DropDuplicates { span: start })
            }

            "drop_missing" => {
                // Bare means every column, which is the common case and reads as
                // what it does. Named columns narrow it.
                let names = if matches!(self.peek(), Some(Tok::Columns(_))) {
                    self.expect_columns("")?
                } else {
                    Vec::new()
                };
                let end = names.last().map(|n| n.span).unwrap_or(start);
                Ok(Step::DropMissing { names, span: start.to(end) })
            }

            "add_rows" => {
                let (other, other_span) = match self.next() {
                    Some(Token { tok: Tok::Word(name), span })
                        if !vocabulary::GRAMMAR_WORDS.contains(&name.as_str()) =>
                    {
                        self.qualified(name, span)?
                    }
                    Some(Token { span, .. }) => {
                        return Err(Diagnostic::illegal(
                            "`add_rows` needs the table whose rows to add, by name: `add_rows more_sales`",
                            span,
                        ))
                    }
                    None => {
                        return Err(Diagnostic::illegal(
                            "`add_rows` needs the table whose rows to add, by name: `add_rows more_sales`",
                            start,
                        ))
                    }
                };
                Ok(Step::AddRows {
                    other: Name { text: other, span: other_span },
                    span: start.to(other_span),
                })
            }

            "add_combinations" => {
                // **A table's name is the mistake worth naming here**, because
                // the verb beside this one in the vocabulary takes one and the
                // two words start alike. Left to the general message, a reader
                // would be told a word was unexpected rather than that they had
                // reached for the neighbour.
                if let Some(Tok::Word(name)) = self.peek() {
                    if !vocabulary::GRAMMAR_WORDS.contains(&name.as_str()) {
                        return Err(Diagnostic::illegal(
                            format!(
                                "`add_combinations` works on this table's own values, so it takes columns rather than a table: `add_combinations [region, product]`. For another table's rows underneath: `add_rows {name}`"
                            ),
                            self.peek_span(),
                        ));
                    }
                }

                let names = self.expect_columns(
                    "`add_combinations` needs the columns whose combinations to make, in brackets: `add_combinations [region, product]`",
                )?;

                // `by` holds its usual meaning — the columns that say which rows
                // correspond — and here it decides where the crossing happens.
                // Without it the whole table is one group.
                let by = if self.eat_word("by") {
                    self.expect_columns(
                        "`by` needs the columns to make the combinations inside, in brackets: `by [store]`",
                    )?
                } else {
                    Vec::new()
                };

                let end = by
                    .last()
                    .or_else(|| names.last())
                    .map(|n| n.span)
                    .unwrap_or(start);
                Ok(Step::AddCombinations { names, by, span: start.to(end) })
            }

            "add" | "summarize" => {
                // `add where name starts "q" as value * 2`: one value for every
                // column whose name matches, instead of a list written out.
                if self.eat_word("where") {
                    let selector = self.expression()?;
                    self.expect_word(
                        "as",
                        &format!("`{word} where ...` has to say what to make of each column: write `as` and then the value, with `value` standing for the column"),
                    )?;
                    let value = self.expression()?;
                    let by = if self.eat_word("by") {
                        self.expect_columns("`by` needs the columns that say which rows go together, in brackets: `by [product]`")?
                    } else {
                        Vec::new()
                    };
                    let end = by.last().map(|n| n.span).unwrap_or_else(|| value.span());
                    let span = start.to(end);
                    let across = Some(Across { selector, value });
                    return Ok(if word == "add" {
                        Step::Add { values: Vec::new(), by, across, span }
                    } else {
                        Step::Summarize { values: Vec::new(), by, across, span }
                    });
                }

                let mut values = Vec::new();
                loop {
                    let name = self.one_column(&format!(
                        "`{word}` names the column it is making, in brackets: `{word} [name] as ...`"
                    ))?;
                    self.expect_word(
                        "as",
                        &format!("`{word} [{}]` has to say what it is: write `as` and then the value", name.text),
                    )?;
                    let value = self.expression()?;
                    values.push(Named { name, value });
                    if !matches!(self.peek(), Some(Tok::Comma)) {
                        break;
                    }
                    self.at += 1;
                }

                let by = if self.eat_word("by") {
                    self.expect_columns("`by` needs the columns that say which rows go together, in brackets: `by [product]`")?
                } else {
                    Vec::new()
                };

                let end = by
                    .last()
                    .map(|n| n.span)
                    .unwrap_or_else(|| values.last().map(|v| v.value.span()).unwrap_or(start));
                let span = start.to(end);
                if word == "add" {
                    Ok(Step::Add { values, by, across: None, span })
                } else {
                    Ok(Step::Summarize { values, by, across: None, span })
                }
            }

            "sort" => {
                // `sort` deliberately does not take the word `by`. `by` names the
                // columns that say which rows go together, in `summarize` and in
                // `add`; a sort key says nothing about which rows correspond. One
                // word doing one job is worth more than the extra syllable.
                if self.at_word("by") {
                    return Err(Diagnostic::illegal(
                        "`sort` does not take the word `by`. Write `sort [column]`, and `descending` after it to run the other way",
                        self.peek_span(),
                    ));
                }
                let mut keys = Vec::new();
                loop {
                    let column = self.one_column(
                        "`sort` needs the column to order by, in brackets: `sort [amount]`",
                    )?;
                    let descending = self.eat_word("descending");
                    if self.at_word("ascending") {
                        return Err(Diagnostic::illegal(
                            "there is no word `ascending`, because ascending is what `sort` does when nothing is asked of it. Write `sort [column]`",
                            self.peek_span(),
                        ));
                    }
                    keys.push(SortKey { column, descending });
                    if !matches!(self.peek(), Some(Tok::Comma)) {
                        break;
                    }
                    self.at += 1;
                }
                let span = start.to(keys.last().map(|k| k.column.span).unwrap_or(start));
                Ok(Step::Sort { keys, span })
            }

            // One arm for both ends. They differ in which rows survive and in
            // nothing else, so the shape, the `by` clause and the two messages
            // are shared rather than written twice with one word changed.
            "take" | "take_last" => match self.next() {
                Some(Token { tok: Tok::Whole(n), span }) if n >= 0 => {
                    let by = if self.eat_word("by") {
                        self.expect_columns("`by` needs the columns that say which rows go together, in brackets: `by [id]`")?
                    } else {
                        Vec::new()
                    };
                    // `with ties` reads as two words and is one marker. `with`
                    // introduces it and `ties` is the only thing it introduces,
                    // so a `with` followed by anything else is a mistake worth
                    // naming here rather than letting it reach the next step.
                    let mut end = by.last().map(|n| n.span).unwrap_or(span);
                    let mut ties = false;
                    if self.eat_word("with") {
                        let where_ties = self.peek_span();
                        if !self.eat_word("ties") {
                            return Err(Diagnostic::illegal(
                                format!("the only thing `{word}` takes after `with` is `ties`: `{word} 3 with ties`"),
                                where_ties,
                            ));
                        }
                        ties = true;
                        end = where_ties;
                    }
                    Ok(Step::Take {
                        count: n as u64,
                        by,
                        last: word == "take_last",
                        ties,
                        span: start.to(end),
                    })
                }
                Some(Token { span, .. }) => Err(Diagnostic::illegal(
                    format!("`{word}` needs a whole number of rows: `{word} 10`"),
                    span,
                )),
                None => Err(Diagnostic::illegal(
                    format!("`{word}` needs a whole number of rows: `{word} 10`"),
                    start,
                )),
            },

            "lengthen" => {
                // **The columns are chosen exactly the way `pick` chooses
                // them**, which is why the commonest reshaping of all — every
                // column except the identifier — is `all_but [id]` and cost
                // nothing to build. tidyr spends an argument and a family of
                // selector helpers on this.
                let mut names = Vec::new();
                let mut all_but = false;
                let mut condition = None;
                let mut end = start;
                if self.eat_word("where") {
                    let e = self.expression()?;
                    end = e.span();
                    condition = Some(e);
                } else {
                    all_but = self.eat_word("all_but");
                    names = self.expect_columns(if all_but {
                        "`all_but` needs the columns to leave where they are, in brackets: `lengthen all_but [id]`"
                    } else {
                        "`lengthen` needs the columns that become rows, in brackets: `lengthen [q1, q2, q3]`. `all_but [id]` names the ones to leave instead, and `where name starts \"q\"` chooses them by the shape of their name"
                    })?;
                    if let Some(last) = names.last() {
                        end = last.span;
                    }
                }

                // **The two new columns default to `name` and `value`**, which
                // are the grammar's own words for what a column is called and
                // what it holds. So the sentence with no naming clause teaches
                // the vocabulary that the next sentence needs, and `lengthen
                // [a, b] then widen` is the round trip spelled with nothing at
                // all.
                let mut name = Pattern::single("name", start);
                let mut value = None;
                if self.eat_word("as") {
                    let (n, v, at) = self.naming("lengthen", false)?;
                    if let Some(n) = n {
                        name = n;
                    }
                    // `naming` hands back an expression because `widen`'s value
                    // is one. `lengthen`'s is a column being created, and the
                    // reader was already held to brackets when it was read.
                    value = match v {
                        Some(Expr::Column(c)) => Some(c),
                        Some(other) => {
                            return Err(Diagnostic::illegal(
                                "`value` names the column that will hold the values, so it is a name rather than a value: `value [answer]`",
                                other.span(),
                            ))
                        }
                        None => None,
                    };
                    end = at;
                }

                if name.has_value() && value.is_some() {
                    return Err(Diagnostic::illegal(
                        "the pattern already says where the value columns are named, with `{value}`, so there is nothing for `value` to name. Drop one of the two",
                        name.span,
                    ));
                }

                Ok(Step::Lengthen {
                    names,
                    all_but,
                    condition,
                    name,
                    value,
                    resolved: None,
                    span: start.to(end),
                })
            }

            "widen" => {
                // **The defaults are the same two words `lengthen` makes**, so
                // `lengthen [a, b] then widen` is the round trip written with
                // nothing at all. That is not a trick: these two verbs are
                // inverses, and a reader who sees that in one line has learned
                // what both of them do.
                let mut end = start;
                let (name, value) = if self.at_word("name") || self.at_word("value") {
                    let (n, v, at) = self.naming("widen", true)?;
                    end = at;
                    (n, v)
                } else {
                    (None, None)
                };
                let name = name.unwrap_or_else(|| Pattern::single("name", start));
                let value = value.unwrap_or_else(|| {
                    Expr::Column(Name { text: "value".into(), span: start })
                });
                if name.has_value() {
                    return Err(Diagnostic::illegal(
                        "`{value}` says a piece of the name picks which value column a row belongs to, which is a question for `lengthen`. Here `value` says what fills the cells, and it takes one value: `widen name \"{question}_{year}\", value [answer]`",
                        name.span,
                    ));
                }

                // `by`, `missing` and `giving` are markers on the step, the way
                // `join` carries `by` and `unmatched`. They are read in any
                // order, because there is no reading in which one has to come
                // before another and refusing an order nobody could predict is
                // a rule to memorize for nothing.
                let mut by = Vec::new();
                let mut missing = None;
                let mut giving = Vec::new();
                loop {
                    let at = self.peek_span();
                    if self.eat_word("by") {
                        if !by.is_empty() {
                            return Err(Diagnostic::illegal("`by` is already here once", at));
                        }
                        by = self.expect_columns(
                            "`by` needs the columns that say which rows go together, in brackets: `by [student]`",
                        )?;
                        end = by.last().map(|n| n.span).unwrap_or(end);
                    } else if self.eat_word("missing") {
                        if missing.is_some() {
                            return Err(Diagnostic::illegal("`missing` is already here once", at));
                        }
                        let e = self.expression()?;
                        end = e.span();
                        missing = Some(e);
                    } else if self.eat_word("giving") {
                        if !giving.is_empty() {
                            return Err(Diagnostic::illegal("`giving` is already here once", at));
                        }
                        giving = self.expect_columns(
                            "`giving` needs the columns this makes, in brackets: `giving [q1, q2, q3]`",
                        )?;
                        end = giving.last().map(|n| n.span).unwrap_or(end);
                    } else {
                        break;
                    }
                }

                Ok(Step::Widen { name, value, by, missing, giving, span: start.to(end) })
            }

            "join" => {
                // The other table is a bare name, the way the head of the
                // pipeline is. It is the only place in the grammar besides the
                // head where a table is named, and it is spelled the same way
                // there so that reading one teaches the other.
                let (other, other_span) = match self.next() {
                    Some(Token { tok: Tok::Word(name), span })
                        if !vocabulary::GRAMMAR_WORDS.contains(&name.as_str()) =>
                    {
                        self.qualified(name, span)?
                    }
                    Some(Token { span, .. }) => {
                        return Err(Diagnostic::illegal(
                            "`join` needs the table to join to, by name: `join products by [id]`",
                            span,
                        ))
                    }
                    None => {
                        return Err(Diagnostic::illegal(
                            "`join` needs the table to join to, by name: `join products by [id]`",
                            start,
                        ))
                    }
                };

                // `by` is optional here and not in `summarize`, because two
                // tables can say for themselves which columns correspond and one
                // table cannot. Leaving it out is answered by an assumption
                // rather than a refusal (§10).
                let by = if self.eat_word("by") {
                    self.join_keys(
                        "`by` needs the columns that say which rows correspond, in brackets: `by [id]`, or `by [customer_id] is [id]` where the two tables name it differently",
                    )?
                } else {
                    Vec::new()
                };

                let mut unmatched = Unmatched::This;
                let mut end = other_span;
                if self.eat_word("unmatched") {
                    match self.next() {
                        Some(Token { tok: Tok::Text(word), span }) => {
                            unmatched = Unmatched::read(&word).ok_or_else(|| {
                                Diagnostic::illegal(
                                    format!(
                                        "`unmatched` takes {}, and `\"{word}\"` is none of them. It says whose unmatched rows survive: `\"this\"` keeps this table's, `\"none\"` keeps neither, `\"both\"` keeps both",
                                        vocabulary::list_or(Unmatched::ALL)
                                    ),
                                    span,
                                )
                            })?;
                            end = span;
                        }
                        Some(Token { span, .. }) => {
                            return Err(Diagnostic::illegal(
                                format!(
                                    "`unmatched` takes {}, written in double quotes: `unmatched \"both\"`",
                                    vocabulary::list_or(Unmatched::ALL)
                                ),
                                span,
                            ))
                        }
                        None => {
                            return Err(Diagnostic::illegal(
                                format!(
                                    "`unmatched` takes {}, written in double quotes: `unmatched \"both\"`",
                                    vocabulary::list_or(Unmatched::ALL)
                                ),
                                start,
                            ))
                        }
                    }
                }

                let span = start.to(end);
                Ok(Step::Join {
                    other: Name { text: other, span: other_span },
                    by,
                    unmatched,
                    span,
                })
            }

            _ => unreachable!("the verb list and this match are the same list"),
        }
    }

    // -- expressions -------------------------------------------------------

    fn expression(&mut self) -> Result<Expr, Diagnostic> {
        self.or()
    }

    fn or(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.and()?;
        while self.at_word("or") {
            self.at += 1;
            let right = self.and()?;
            let span = left.span().to(right.span());
            left = Expr::Logic { op: Logic::Or, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn and(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.negation()?;
        while self.at_word("and") {
            self.at += 1;
            let right = self.negation()?;
            let span = left.span().to(right.span());
            left = Expr::Logic { op: Logic::And, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn negation(&mut self) -> Result<Expr, Diagnostic> {
        if self.at_word("not") {
            let start = self.peek_span();
            self.at += 1;
            let inner = self.negation()?;
            let span = start.to(inner.span());
            return Ok(Expr::Not { inner: Box::new(inner), span });
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, Diagnostic> {
        let left = self.additive()?;

        // `is`, and everything that can follow it: a value, `not`, or `missing`.
        if self.at_word("is") {
            self.at += 1;
            let negated = self.eat_word("not");
            if self.at_word("missing") {
                let end = self.peek_span();
                self.at += 1;
                let span = left.span().to(end);
                return Ok(Expr::IsMissing { inner: Box::new(left), negated, span });
            }
            let right = self.additive()?;
            let span = left.span().to(right.span());
            let op = if negated { Compare::IsNot } else { Compare::Is };
            return Ok(Expr::Compare { op, left: Box::new(left), right: Box::new(right), span });
        }

        // `in { … }`, and `not in { … }`.
        let negated_in = if self.at_word("not") && self.tokens.get(self.at + 1).map(|t| &t.tok)
            == Some(&Tok::Word("in".into()))
        {
            self.at += 1;
            true
        } else {
            false
        };
        if self.at_word("in") {
            self.at += 1;
            let (set, end) = self.value_set()?;
            let span = left.span().to(end);
            return Ok(Expr::In { left: Box::new(left), set, negated: negated_in, span });
        }

        // `starts`, `ends` and `contains`, which sit between their operands the
        // way `is` and `in` do.
        for (word, op) in
            [("starts", TextOp::Starts), ("ends", TextOp::Ends), ("contains", TextOp::Contains)]
        {
            if self.at_word(word) {
                self.at += 1;
                let value = self.additive()?;
                let span = left.span().to(value.span());
                return Ok(Expr::TextTest {
                    op,
                    left: Box::new(left),
                    value: Box::new(value),
                    span,
                });
            }
        }

        let op = match self.peek() {
            Some(Tok::Less) => Compare::Less,
            Some(Tok::LessOrEqual) => Compare::LessOrEqual,
            Some(Tok::Greater) => Compare::Greater,
            Some(Tok::GreaterOrEqual) => Compare::GreaterOrEqual,
            _ => return Ok(left),
        };
        self.at += 1;
        let right = self.additive()?;
        let span = left.span().to(right.span());
        Ok(Expr::Compare { op, left: Box::new(left), right: Box::new(right), span })
    }

    fn value_set(&mut self) -> Result<(Vec<Expr>, Span), Diagnostic> {
        let open = self.peek_span();
        if !matches!(self.peek(), Some(Tok::OpenBrace)) {
            return Err(Diagnostic::illegal(
                "`in` needs a set of values in braces: `in {\"West\", \"East\"}`",
                open,
            ));
        }
        self.at += 1;
        let mut set = Vec::new();
        loop {
            if matches!(self.peek(), Some(Tok::CloseBrace)) {
                break;
            }
            set.push(self.primary()?);
            if matches!(self.peek(), Some(Tok::Comma)) {
                self.at += 1;
                continue;
            }
            break;
        }
        let close = self.peek_span();
        if !matches!(self.peek(), Some(Tok::CloseBrace)) {
            return Err(Diagnostic::illegal(
                "this set of values is never closed. Add a `}` after the last one",
                open,
            ));
        }
        self.at += 1;
        if set.is_empty() {
            return Err(Diagnostic::illegal(
                "this set has no values in it, so nothing could ever match it. Write the values between the braces: `in {\"West\", \"East\"}`",
                open.to(close),
            ));
        }
        Ok((set, close))
    }

    fn additive(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Plus) => Arith::Add,
                Some(Tok::Minus) => Arith::Subtract,
                _ => break,
            };
            self.at += 1;
            let right = self.multiplicative()?;
            let span = left.span().to(right.span());
            left = Expr::Arithmetic { op, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> Result<Expr, Diagnostic> {
        let mut left = self.primary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Star) => Arith::Multiply,
                Some(Tok::Slash) => Arith::Divide,
                _ => break,
            };
            self.at += 1;
            let right = self.primary()?;
            let span = left.span().to(right.span());
            left = Expr::Arithmetic { op, left: Box::new(left), right: Box::new(right), span };
        }
        Ok(left)
    }

    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let span = self.peek_span();
        match self.peek().cloned() {
            Some(Tok::Columns(names)) => {
                self.at += 1;
                if names.len() != 1 {
                    return Err(Diagnostic::illegal(
                        "several columns were written where one value belongs. Write one column here",
                        span,
                    ));
                }
                Ok(Expr::Column(names.into_iter().next().unwrap()))
            }
            Some(Tok::Text(value)) => {
                self.at += 1;
                Ok(Expr::Text { value, span })
            }
            Some(Tok::Whole(value)) => {
                self.at += 1;
                Ok(Expr::Whole { value, span })
            }
            Some(Tok::Decimal(value)) => {
                self.at += 1;
                Ok(Expr::Decimal { value, span })
            }
            Some(Tok::Minus) => {
                self.at += 1;
                let inner = self.primary()?;
                let full = span.to(inner.span());
                Ok(Expr::Arithmetic {
                    op: Arith::Subtract,
                    left: Box::new(Expr::Whole { value: 0, span }),
                    right: Box::new(inner),
                    span: full,
                })
            }
            Some(Tok::OpenParen) => {
                self.at += 1;
                let inner = self.expression()?;
                if !matches!(self.peek(), Some(Tok::CloseParen)) {
                    return Err(Diagnostic::illegal(
                        "this group is never closed. Add a `)` after it",
                        span,
                    ));
                }
                self.at += 1;
                Ok(inner)
            }
            Some(Tok::Word(word)) => self.word_value(word, span),
            _ => Err(Diagnostic::illegal("a value belongs here: a column in brackets, a number, or text in double quotes", span)),
        }
    }

    fn word_value(&mut self, word: String, span: Span) -> Result<Expr, Diagnostic> {
        // The flow word is structural and may never appear inside an expression.
        // Reaching it here means a clause was left unfinished, and saying so is
        // more use than reporting whatever the next step's verb turned out to be.
        if word == FLOW_WORD {
            return Err(Diagnostic::illegal(
                format!("`{FLOW_WORD}` separates steps and cannot appear inside a value. Something is missing before it"),
                span,
            ));
        }

        match word.as_str() {
            "yes" | "no" => {
                self.at += 1;
                return Ok(Expr::Truth { value: word == "yes", span });
            }
            "missing" => {
                self.at += 1;
                return Ok(Expr::Missing { span });
            }
            // The column's own name, which only `pick where` asks about. The
            // checker refuses it anywhere else, in its own words.
            "name" => {
                self.at += 1;
                return Ok(Expr::ColumnName { span });
            }
            // The column being worked on, inside `add where` / `summarize where`.
            "value" => {
                self.at += 1;
                return Ok(Expr::ColumnValue { span });
            }
            // What the column holds, inside a `where` that chooses columns.
            "kind" => {
                self.at += 1;
                return Ok(Expr::ColumnKind { span });
            }
            _ => {}
        }

        // The spellings a person arrives with. Each host writes truth and the
        // absent value its own way, and each of those ways is somebody's habit,
        // so the grammar names the one word it takes rather than reporting that
        // an unknown name appeared. Without this, `is TRUE` is told to write
        // `[TRUE]`, which invites making a column out of a typo.
        if let Some(fix) = neutral_word_for(&word) {
            return Err(Diagnostic::illegal(
                format!(
                    "the grammar writes this as `{fix}`, so there is one spelling rather than one per language. Write `{fix}` instead of `{word}`"
                ),
                span,
            ));
        }

        self.at += 1;

        // `matching` is the one condition whose first argument is a table rather
        // than a value, so it is read here rather than by the argument loop
        // below, which parses values and would report a table name as a bare
        // word. Everything after the table is `join`'s own clause, spelled the
        // same way, because working out which columns correspond is one idea.
        if word == "matching" {
            return self.matching(span);
        }

        // `rank` takes a column in an *ordering* position, which is a sort key
        // and not a value, so it may carry `descending` exactly as `sort` does.
        // No other function takes one, which is why it is read here.
        if word == "rank" || word == "row_number" {
            return self.window(&word, span);
        }

        // `when` reads its arguments in pairs and may end with an `otherwise`,
        // so `name(value, value, ...)` is not the sentence it has. Same reason
        // `matching` is read here rather than looked up in the function table.
        if word == "when" {
            return self.conditional(span);
        }

        // `rolling` takes an aggregate call where every other function takes a
        // value, so it is read here for the reason `matching` is: the argument
        // loop below parses values, and would hand the checker a shape it
        // could only refuse with the wrong words.
        if word == "rolling" {
            return self.rolling(span);
        }

        // `look_up` reads its arguments in pairs and ends with an `otherwise`,
        // which is `when`'s shape, so it is read the way `when` is.
        if word == "look_up" {
            return self.lookup(span);
        }

        // A name in front of a group applies that name to what is inside.
        if !matches!(self.peek(), Some(Tok::OpenParen)) {
            let suggestion = nearest(&word, vocabulary::FUNCTIONS.iter().map(|f| f.name))
                .map(|s| format!(" Did you mean `{s}(...)`?"))
                .unwrap_or_default();
            return Err(Diagnostic::illegal(
                format!(
                    "`{word}` is a bare word where a value belongs. A column is written in brackets, as `[{word}]`, and a function is written with parentheses.{suggestion}"
                ),
                span,
            ));
        }
        self.at += 1;

        let mut args = Vec::new();
        if !matches!(self.peek(), Some(Tok::CloseParen)) {
            loop {
                args.push(self.expression()?);
                if matches!(self.peek(), Some(Tok::Comma)) {
                    self.at += 1;
                    continue;
                }
                break;
            }
        }
        let close = self.peek_span();
        if !matches!(self.peek(), Some(Tok::CloseParen)) {
            return Err(Diagnostic::illegal(
                format!("`{word}(` is never closed. Add a `)` after its arguments"),
                span,
            ));
        }
        self.at += 1;

        Ok(Expr::Call { name: word, args, span: span.to(close) })
    }

    /// `rank([delay])`, `rank([delay] descending)` and `row_number()`, with
    /// `self.at` sitting on the `(`.
    /// `when(test, value, test, value, …, otherwise value)`.
    ///
    /// **The arguments come in pairs and the first match wins**, which is the
    /// same reading `first_present` asks for: a list in priority order rather
    /// than a set. An odd count is the mistake this shape invites, so it is
    /// refused by naming the test that has no answer beside it rather than by
    /// counting arguments at the reader.
    ///
    /// `otherwise` is a word rather than a named argument because the text form
    /// has no `=`. It reads the way `unmatched "both"` and `by [id]` do, which
    /// is the shape the grammar already uses for a marker and its value.
    fn conditional(&mut self, span: Span) -> Result<Expr, Diagnostic> {
        if !matches!(self.peek(), Some(Tok::OpenParen)) {
            return Err(Diagnostic::illegal(
                "`when` needs its questions and answers in parentheses: `when([score] >= 90, \"A\", otherwise \"C\")`",
                span,
            ));
        }
        self.at += 1;

        let mut arms: Vec<(Expr, Expr)> = Vec::new();
        let mut otherwise = None;
        loop {
            if matches!(self.peek(), Some(Tok::CloseParen)) {
                break;
            }
            if self.eat_word("otherwise") {
                let value = self.expression()?;
                otherwise = Some(Box::new(value));
                // Nothing may follow it: an answer written after the catch-all
                // could never be reached, and silently ignoring it would be the
                // sort of quiet wrongness the grammar refuses elsewhere.
                if matches!(self.peek(), Some(Tok::Comma)) {
                    return Err(Diagnostic::illegal(
                        "`otherwise` is the answer when nothing else matched, so it comes last. Anything written after it could never be reached",
                        self.peek_span(),
                    ));
                }
                break;
            }

            let test = self.expression()?;
            if !matches!(self.peek(), Some(Tok::Comma)) {
                return Err(Diagnostic::illegal(
                    "each question `when` asks needs the answer that goes with it, right after it: `when([score] >= 90, \"A\", otherwise \"C\")`",
                    test.span(),
                ));
            }
            self.at += 1;
            if self.at_word("otherwise") {
                return Err(Diagnostic::illegal(
                    "this question has no answer beside it. Every question `when` asks is followed by what it gives: `when([score] >= 90, \"A\", otherwise \"C\")`",
                    test.span(),
                ));
            }
            let value = self.expression()?;
            arms.push((test, value));

            if matches!(self.peek(), Some(Tok::Comma)) {
                self.at += 1;
                continue;
            }
            break;
        }

        let close = self.peek_span();
        if !matches!(self.peek(), Some(Tok::CloseParen)) {
            return Err(Diagnostic::illegal(
                "`when(` is never closed. Add a `)` after its answers",
                span,
            ));
        }
        self.at += 1;

        Ok(Expr::When { arms, otherwise, span: span.to(close) })
    }

    fn window(&mut self, word: &str, span: Span) -> Result<Expr, Diagnostic> {
        let kind = if word == "rank" { Window::Rank } else { Window::RowNumber };
        let shape = match kind {
            Window::Rank => "`rank` needs the column to rank by, in brackets: `rank([revenue] descending)`",
            Window::RowNumber => "`row_number()` takes nothing, because it numbers the rows in the order they are already in. To number by a column, `rank([revenue])` says what it goes by",
        };

        if !matches!(self.peek(), Some(Tok::OpenParen)) {
            return Err(Diagnostic::illegal(shape, span));
        }
        self.at += 1;

        let key = if kind == Window::Rank {
            let columns = self.expect_columns(shape)?;
            if columns.len() != 1 {
                return Err(Diagnostic::illegal(
                    "`rank` ranks by one column. Write one, and sort by the rest first if the order of ties matters",
                    span,
                ));
            }
            let column = columns.into_iter().next().unwrap();
            let descending = self.eat_word("descending");
            Some(SortKey { column, descending })
        } else {
            // Saying so beats reporting an unclosed parenthesis, because the
            // reader's mistake is about meaning rather than about punctuation.
            if !matches!(self.peek(), Some(Tok::CloseParen)) {
                return Err(Diagnostic::illegal(shape, self.peek_span()));
            }
            None
        };

        let close = self.peek_span();
        if !matches!(self.peek(), Some(Tok::CloseParen)) {
            return Err(Diagnostic::illegal(
                format!("`{word}(` is never closed. Add a `)` after it"),
                span,
            ));
        }
        self.at += 1;

        Ok(Expr::Window { kind, key, span: span.to(close) })
    }

    /// `look_up([code], "W", "West", …, otherwise [code])`, with `self.at`
    /// sitting on the `(`.
    ///
    /// **The `otherwise` is required, and the refusal is the design.** The
    /// neighbours split into two words over what happens to a value with no
    /// pair — left alone against sent missing — so a default either way would
    /// surprise half of everyone arriving. The sentence says where they go,
    /// the way `join`'s `unmatched` says it for rows.
    fn lookup(&mut self, span: Span) -> Result<Expr, Diagnostic> {
        const SHAPE: &str = "`look_up` maps written values to written values, pairs side by side, and says where the rest go: `look_up([code], \"W\", \"West\", otherwise [code])`";

        if !matches!(self.peek(), Some(Tok::OpenParen)) {
            return Err(Diagnostic::illegal(SHAPE, span));
        }
        self.at += 1;

        let subject = self.expression()?;
        if !matches!(self.peek(), Some(Tok::Comma)) {
            return Err(Diagnostic::illegal(SHAPE, self.peek_span()));
        }
        self.at += 1;

        let mut pairs: Vec<(Expr, Expr)> = Vec::new();
        let mut otherwise = None;
        loop {
            if matches!(self.peek(), Some(Tok::CloseParen)) {
                break;
            }
            if self.eat_word("otherwise") {
                otherwise = Some(self.expression()?);
                // Nothing may follow it: a pair written after the catch-all
                // could never be reached, exactly as in `when`.
                if matches!(self.peek(), Some(Tok::Comma)) {
                    return Err(Diagnostic::illegal(
                        "`otherwise` is where the values with no pair go, so it comes last. Anything written after it could never be reached",
                        self.peek_span(),
                    ));
                }
                break;
            }

            let from = self.expression()?;
            if !matches!(self.peek(), Some(Tok::Comma)) {
                return Err(Diagnostic::illegal(
                    "each value `look_up` maps needs what it becomes, right after it: `look_up([code], \"W\", \"West\", otherwise [code])`",
                    from.span(),
                ));
            }
            self.at += 1;
            if self.at_word("otherwise") {
                return Err(Diagnostic::illegal(
                    "this value has nothing beside it to become. Every value looked up is followed by its answer: `look_up([code], \"W\", \"West\", otherwise [code])`",
                    from.span(),
                ));
            }
            let to = self.expression()?;
            pairs.push((from, to));

            if matches!(self.peek(), Some(Tok::Comma)) {
                self.at += 1;
                continue;
            }
            break;
        }

        let close = self.peek_span();
        if !matches!(self.peek(), Some(Tok::CloseParen)) {
            return Err(Diagnostic::illegal(
                "`look_up(` is never closed. Add a `)` after its `otherwise`",
                span,
            ));
        }
        self.at += 1;
        let whole = span.to(close);

        let Some(otherwise) = otherwise else {
            return Err(Diagnostic::illegal(
                "`look_up` says where a value with no pair goes, so it ends with `otherwise`: keep those values with `otherwise [code]`, drop them with `otherwise missing`, or write a default",
                whole,
            ));
        };
        if pairs.is_empty() {
            return Err(Diagnostic::illegal(
                "`look_up` needs at least one pair to look up: `look_up([code], \"W\", \"West\", otherwise [code])`",
                whole,
            ));
        }

        Ok(Expr::Lookup {
            subject: Box::new(subject),
            pairs,
            otherwise: Box::new(otherwise),
            span: whole,
        })
    }

    /// `rolling(average([revenue]), 7)`, with `self.at` sitting on the `(`.
    ///
    /// **The aggregate call is taken apart here rather than kept whole**,
    /// because the plan holds the name and the arguments separately: the call
    /// inside is the window's parameter, not a live aggregate, and storing it
    /// as one would make the plan answer "this collapses a group" about a
    /// value that slides along it. What the name may be — and what the count
    /// may be — is the checker's question, asked there so the refusals can
    /// name what to write instead.
    fn rolling(&mut self, span: Span) -> Result<Expr, Diagnostic> {
        const SHAPE: &str = "`rolling` asks an aggregate of the last few rows, so it takes the aggregate and how many rows: `rolling(average([revenue]), 7)`";

        if !matches!(self.peek(), Some(Tok::OpenParen)) {
            return Err(Diagnostic::illegal(SHAPE, span));
        }
        self.at += 1;

        let first = self.expression()?;
        let (agg, agg_span, args) = match first {
            Expr::Call { name, args, span } => (name, span, args),
            // `rank` and `row_number` parse to their own variant, so a reader
            // who reaches for one here is told the real difference rather than
            // shown the general shape.
            Expr::Window { span, .. } => {
                return Err(Diagnostic::illegal(
                    "`rolling` holds an aggregate — a value that spans rows — and this is already a value worked out along them. The aggregates are `total`, `average`, `median`, `smallest`, `largest` and `standard_deviation`",
                    span,
                ));
            }
            other => return Err(Diagnostic::illegal(SHAPE, other.span())),
        };

        if !matches!(self.peek(), Some(Tok::Comma)) {
            return Err(Diagnostic::illegal(
                "`rolling` also needs how many rows the window holds, after the aggregate: `rolling(average([revenue]), 7)`",
                self.peek_span(),
            ));
        }
        self.at += 1;
        let count = self.expression()?;

        let close = self.peek_span();
        if !matches!(self.peek(), Some(Tok::CloseParen)) {
            return Err(Diagnostic::illegal(
                "`rolling(` is never closed. Add a `)` after how many rows",
                span,
            ));
        }
        self.at += 1;

        Ok(Expr::Rolling {
            agg,
            agg_span,
            args,
            count: Box::new(count),
            span: span.to(close),
        })
    }

    /// `matching(products)` or `matching(products, by [id])`, with `self.at`
    /// sitting on the `(`.
    fn matching(&mut self, span: Span) -> Result<Expr, Diagnostic> {
        const SHAPE: &str =
            "`matching` asks whether a row has a partner in another table, so it needs that table by name: `matching(products, by [id])`";

        if !matches!(self.peek(), Some(Tok::OpenParen)) {
            return Err(Diagnostic::illegal(SHAPE, span));
        }
        self.at += 1;

        let (other, other_span) = match self.next() {
            Some(Token { tok: Tok::Word(name), span }) if name != FLOW_WORD => {
                self.qualified(name, span)?
            }
            Some(Token { span, .. }) => return Err(Diagnostic::illegal(SHAPE, span)),
            None => return Err(Diagnostic::illegal(SHAPE, span)),
        };

        // `by` is optional for the same reason it is optional on `join`: two
        // tables can say for themselves which columns correspond, and leaving it
        // out is answered by an assumption rather than a refusal.
        let mut by = Vec::new();
        if matches!(self.peek(), Some(Tok::Comma)) {
            self.at += 1;
            if !self.eat_word("by") {
                return Err(Diagnostic::illegal(
                    "the only thing `matching` takes after the table is the columns that say which rows correspond: `matching(products, by [id])`",
                    self.peek_span(),
                ));
            }
            by = self.join_keys(
                "`by` needs the columns that say which rows correspond, in brackets: `matching(products, by [id])`, or `by [customer_id] is [id]` where the two tables name it differently",
            )?;
        }

        let close = self.peek_span();
        if !matches!(self.peek(), Some(Tok::CloseParen)) {
            return Err(Diagnostic::illegal(
                "`matching(` is never closed. Add a `)` after the table it names",
                span,
            ));
        }
        self.at += 1;

        Ok(Expr::Matching {
            other: Name { text: other, span: other_span },
            by,
            span: span.to(close),
        })
    }
}

/// Read `"{question}_{year}"` into the literals and the pieces between them.
///
/// **This is the one place a pattern is read at all**, and it is deliberately
/// tiny: braces mark a piece, everything else is literal text, and there is no
/// third rule. A regex would cover more and would be unreadable after a
/// two-month gap, which is the failure this project is organized against
/// (§14.1).
fn read_pattern(text: &str, span: Span) -> Result<Pattern, Diagnostic> {
    let mut literals = Vec::new();
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '{' => {
                let mut inner = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    inner.push(c);
                }
                if !closed {
                    return Err(Diagnostic::illegal(
                        "this `{` is never closed. Each piece of the name goes in a pair of braces: `\"{question}_{year}\"`",
                        span,
                    ));
                }
                if inner.trim().is_empty() {
                    return Err(Diagnostic::illegal(
                        "`{}` has nothing in it, so there is no column for that piece to become. Name it: `\"{question}_{year}\"`",
                        span,
                    ));
                }
                literals.push(std::mem::take(&mut literal));
                parts.push(if inner == "value" {
                    PatternPart::Value
                } else {
                    PatternPart::Named(inner)
                });
            }
            '}' => {
                return Err(Diagnostic::illegal(
                    "this `}` closes nothing. Each piece of the name goes in a pair of braces: `\"{question}_{year}\"`",
                    span,
                ))
            }
            _ => literal.push(c),
        }
    }
    literals.push(literal);

    if parts.is_empty() {
        return Err(Diagnostic::illegal(
            format!(
                "`\"{text}\"` has no pieces in braces, so it names nothing and every column would get the same name. Say what varies: `\"{{question}}_{{year}}\"`"
            ),
            span,
        ));
    }

    // **Two pieces with nothing between them cannot be told apart**, and the
    // failure is silent rather than loud: `"{a}{b}"` would match anywhere and
    // split in a place nobody chose. Refusing it here is the only place that
    // can, because by the time a name is being read there is nothing to say.
    for (i, _) in parts.iter().enumerate().skip(1) {
        if literals[i].is_empty() {
            return Err(Diagnostic::illegal(
                "two pieces sit against each other with nothing between them, so there is no way to tell where one ends. Put the text that separates them in between: `\"{question}_{year}\"`",
                span,
            ));
        }
    }

    let named = |p: &PatternPart| match p {
        PatternPart::Named(n) => Some(n.clone()),
        PatternPart::Value => None,
    };
    let names: Vec<String> = parts.iter().filter_map(named).collect();
    for (i, name) in names.iter().enumerate() {
        if names[..i].contains(name) {
            return Err(Diagnostic::illegal(
                format!("`{{{name}}}` is here twice, and two columns cannot share one name. Give the second piece its own"),
                span,
            ));
        }
    }
    if parts.iter().filter(|p| **p == PatternPart::Value).count() > 1 {
        return Err(Diagnostic::illegal(
            "`{value}` says which value column a piece belongs to, so there is only ever one of it",
            span,
        ));
    }

    Ok(Pattern { literals, parts, span, quoted: true })
}
