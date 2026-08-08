//! What the grammar says when it will not do something.
//!
//! **A refusal names what would have happened and what to write instead.** That
//! is the whole standard, and it is the difference between a tool a person
//! trusts and one they work around. `unknown column 'reveune'` tells someone
//! they made a typo they can already see; naming the column they probably meant,
//! and listing the ones that exist, tells them what to type next (§10).
//!
//! **Never accept a clause and quietly drop it.** A pipeline that runs, returns
//! a number, and silently ignored one line is the single most expensive way a
//! data tool loses someone's trust, because nothing about the result says
//! anything went wrong.
//!
//! Because the grammar is parsed rather than embedded in a host language, every
//! refusal knows the exact characters that caused it and can put a caret under
//! them. A host's own error machinery cannot do this: it points at the call, and
//! the clause is somewhere inside.

use crate::plan::Span;

/// The three kinds, and the difference matters to whoever is reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The sentence cannot mean anything. A column that is not there, a function
    /// given the wrong number of arguments, a step that needs a group and has
    /// none. Fatal, and nothing runs.
    Illegal,
    /// The sentence is legal and is not built yet. Fatal, and says so, because a
    /// legal sentence that quietly does nothing is worse than one that stops.
    Unsupported,
    /// The grammar chose something the caller did not say. Not fatal, and names
    /// both the choice and how to state it outright.
    Assumption,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Illegal => "illegal",
            Kind::Unsupported => "not built yet",
            Kind::Assumption => "assumption",
        }
    }

    pub fn is_fatal(self) -> bool {
        !matches!(self, Kind::Assumption)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: Kind,
    /// The whole message, as one line, so a test can assert it word for word.
    /// A message nothing ever compares against is a message that drifts.
    pub message: String,
    pub span: Option<Span>,
}

impl Diagnostic {
    pub fn illegal(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { kind: Kind::Illegal, message: message.into(), span: Some(span) }
    }

    pub fn unsupported(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { kind: Kind::Unsupported, message: message.into(), span: Some(span) }
    }

    pub fn assumption(message: impl Into<String>, span: Span) -> Self {
        Diagnostic { kind: Kind::Assumption, message: message.into(), span: Some(span) }
    }

    /// The message with the offending text quoted underneath it.
    ///
    /// The line is shown as the caller wrote it, with the caret under the exact
    /// characters. Tabs become single spaces first, so the caret lands where the
    /// eye expects rather than where the byte count says.
    pub fn render(&self, source: &str) -> String {
        let Some(span) = self.span else {
            return format!("{}: {}", self.kind.label(), self.message);
        };

        let (line_start, line_number) = line_of(source, span.start);
        let line_end = source[line_start..].find('\n').map(|i| line_start + i).unwrap_or(source.len());
        let line = source[line_start..line_end].replace('\t', " ");

        let column = source[line_start..span.start].chars().count();
        let width = source[span.start..(span.start + span.len).min(source.len())]
            .chars()
            .count()
            .max(1);

        let gutter = format!("{line_number}");
        let pad = " ".repeat(gutter.len());
        format!(
            "{}: {}\n{pad} |\n{gutter} | {line}\n{pad} | {}{}",
            self.kind.label(),
            self.message,
            " ".repeat(column),
            "^".repeat(width),
        )
    }
}

fn line_of(source: &str, offset: usize) -> (usize, usize) {
    let mut start = 0;
    let mut number = 1;
    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            start = i + 1;
            number += 1;
        }
    }
    (start, number)
}

/// The closest real name to what someone typed, if anything is close enough.
///
/// **A suggestion is always a name that exists**, and it is only offered when it
/// is near enough to be a typo rather than a different word. A wrong suggestion
/// is worse than none: it sends a reader to check a column that was never the
/// one they wanted. The cap is two edits, or one for very short names, where two
/// edits can reach an unrelated word.
pub fn nearest<'a>(typed: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = if typed.chars().count() <= 4 { 1 } else { 2 };
    candidates
        .into_iter()
        .map(|c| (edit_distance(typed, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, c)| (*d, c.len()))
        .map(|(_, c)| c)
}

/// Levenshtein distance, case insensitive, two rows at a time.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }

    let mut previous: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];

    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            current[j + 1] = (previous[j] + cost)
                .min(previous[j + 1] + 1)
                .min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

/// A list of names, written the way the messages write them.
pub fn list(names: &[String]) -> String {
    names.join(", ")
}
