//! god-cli — a pipeline in, a query out.
//!
//! ```text
//! $ god --columns 'region:text,revenue:number' \
//!       'sales then keep where [region] is "West" then take 10'
//! WITH step0 AS (SELECT * FROM "sales"),
//!      step1 AS (SELECT * FROM step0 WHERE ("region" = 'West')),
//!      step2 AS (SELECT * FROM step1 LIMIT 10)
//! SELECT * FROM step2
//! ```
//!
//! **The data does not come through here, and that is the whole design.** The
//! host already holds the table; what it lacks is the query. So a pipeline and a
//! list of columns go in — kilobytes — and text comes out, and the host runs that
//! text against data that never moved. An earlier design had tables crossing this
//! boundary as Arrow files, which needed a bridge; there is nothing to bridge.
//!
//! **The schema is a flag rather than JSON**, and that is deliberate. `god-core`
//! has no dependencies, which is what makes it compile in under a second and
//! vendor without argument. Reaching for a serialization library to carry six
//! column names would trade that away for nothing: `region:text,revenue:number`
//! is twenty lines of parsing and keeps the property.
//!
//! Exit codes are the contract with whatever is calling:
//!
//! | | |
//! |---|---|
//! | 0 | the query is on stdout |
//! | 1 | the command line was wrong |
//! | 2 | the pipeline was refused, and the reason is on stderr |
//!
//! Exiting 0 with no output is how a tool teaches a caller to stop checking.

use std::io::Read;
use std::process::ExitCode;

use god_core::{backend, Schema, Type};

const USAGE: &str = "\
god — a grammar of data

    god [options] <pipeline>
    god [options] < pipeline.god

Options
    --columns <list>   the table's columns, as name:type separated by commas.
                       Types: text, number, truth, date, timestamp. Anything
                       else is read as a type the grammar has no opinion about.
                       A timestamp is a date carrying a time. The two differ for
                       `hour` alone, and `kind` reports both as a date.
    --as <backend>     what to write out. Default: sql
    --draw <way>       draw what the pipeline does to the table, step by step,
                       and run nothing. `text` for a terminal, `svg` for a page.
                       A sentence that will not check is drawn as far as it
                       checked, with the refusal under the words that stopped it.
    --needs            print the table this pipeline reads, and stop. A launcher
                       asks this first, so it knows which table to describe
                       without having to read the pipeline itself.
    --vocabulary       print every word the grammar has, one per line, tagged
                       with what kind of word it is. Nothing else should keep
                       its own copy of this list.
    --seats            print where each kind of function can stand, one cell
                       per line: seat, kind, stands or refused, and the note
                       the cell carries. The book's table is generated from
                       this, so it cannot drift from the checker.
    --help             this

Examples
    god --columns 'region:text,revenue:number' \\
        'sales then keep where [region] is \"West\" then take 10'

    Wrap the pipeline in single quotes: a text value is written with double
    quotes, so single quotes around the whole thing keep the shell out of it.

    god --columns 'a:number' --as dplyr 't then sort [a] descending'
";

fn main() -> ExitCode {
    match run() {
        Ok(out) => {
            print!("{out}");
            ExitCode::SUCCESS
        }
        Err(Failure::Usage(message)) => {
            eprintln!("god: {message}\n\n{USAGE}");
            ExitCode::from(1)
        }
        Err(Failure::Refused(rendered)) => {
            eprintln!("{rendered}");
            ExitCode::from(2)
        }
        Err(Failure::Help) => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(Failure::Vocabulary) => {
            print!("{}", vocabulary());
            ExitCode::SUCCESS
        }
        Err(Failure::Seats) => {
            print!("{}", seats());
            ExitCode::SUCCESS
        }
    }
}

enum Failure {
    Usage(String),
    Refused(String),
    Help,
    /// Not a failure, and it travels this way because `run` returns one string
    /// and this returns a list. Kept beside `Help` for the same reason.
    Vocabulary,
    /// The composition table, the same way: where each kind can stand.
    Seats,
}

/// Where each kind of function can stand, cell by cell, from the table the
/// checker's own tests enforce. The book's grid is generated from this for
/// the reason `vocabulary` gives: a copy kept anywhere else goes stale the
/// day a cell moves.
fn seats() -> String {
    use god_core::seats::{note_for, rule_for, Admission, KINDS, SEATS};

    let mut lines = Vec::new();
    for &seat in SEATS {
        for &kind in KINDS {
            let answer = match rule_for(seat, kind) {
                Admission::Stands => "stands",
                Admission::Refused => "refused",
            };
            lines.push(format!(
                "{}\t{}\t{}\t{}\t{}",
                seat.word(),
                seat.shown_as(),
                format!("{kind:?}").to_lowercase(),
                answer,
                note_for(seat, kind),
            ));
        }
    }
    lines.join("\n") + "\n"
}

/// Every word the grammar has, tagged with its role.
///
/// **This exists so that nothing outside `god-core` keeps its own copy.** Two
/// bindings and a book already describe the vocabulary, and a list written down
/// in any of them goes stale the day a word is added, silently, which has
/// happened here more than once. Anything that wants to know what the grammar
/// contains asks the grammar.
fn vocabulary() -> String {
    use god_core::vocabulary::{Kind, FUNCTIONS, GRAMMAR_WORDS, VERBS};

    let mut lines = Vec::new();
    for v in VERBS {
        lines.push(format!("verb\t{v}"));
    }
    for f in FUNCTIONS {
        let kind = match f.kind {
            Kind::Aggregate => "aggregate",
            Kind::Scalar => "scalar",
            Kind::Window => "window",
        };
        lines.push(format!("{kind}\t{}", f.name));
    }
    for w in GRAMMAR_WORDS {
        lines.push(format!("word\t{w}"));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn run() -> Result<String, Failure> {
    let mut columns: Option<String> = None;
    let mut named: Vec<(String, String)> = Vec::new();
    let mut wanted: Option<String> = None;
    let mut pipeline: Option<String> = None;
    let mut needs_only = false;
    let mut drawing: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Err(Failure::Help),
            "--needs" => needs_only = true,
            "--draw" => {
                if drawing.is_some() {
                    return Err(Failure::Usage(
                        "`--draw` was given twice, and this takes one".into(),
                    ));
                }
                drawing = Some(args.next().ok_or_else(|| {
                    Failure::Usage(format!(
                        "`--draw` needs a way of drawing. There is: {}",
                        god_core::draw::ways().join(", ")
                    ))
                })?)
            }
            "--vocabulary" => return Err(Failure::Vocabulary),
            "--seats" => return Err(Failure::Seats),
            "--columns" => {
                let value = args.next().ok_or_else(|| {
                    Failure::Usage("`--columns` needs a list, like \"region:text,revenue:number\"".into())
                })?;
                // `--columns products=id:number,name:text` describes a table by
                // name, for a pipeline that joins. Without a name it is the
                // table at the head, which is the only one most pipelines have.
                //
                // Given twice is refused rather than last-one-wins: a flag this
                // tool quietly dropped would be the same defect as a clause the
                // grammar quietly dropped, and the grammar refuses those.
                match split_named(&value) {
                    Some((table, list)) => {
                        if named.iter().any(|(t, _)| *t == table) {
                            return Err(Failure::Usage(format!(
                                "`--columns` described `{table}` twice, and a table takes one description"
                            )));
                        }
                        named.push((table, list))
                    }
                    None => {
                        if columns.is_some() {
                            return Err(Failure::Usage(
                                "`--columns` described the head table twice, and it takes one description".into(),
                            ));
                        }
                        columns = Some(value)
                    }
                }
            }
            "--as" => {
                if wanted.is_some() {
                    return Err(Failure::Usage(
                        "`--as` was given twice, and this takes one".into(),
                    ));
                }
                wanted = Some(args.next().ok_or_else(|| {
                    Failure::Usage(format!(
                        "`--as` needs a backend. There is: {}",
                        backend::names().join(", ")
                    ))
                })?)
            }
            other if other.starts_with('-') => {
                return Err(Failure::Usage(format!("`{other}` is not an option")))
            }
            other => {
                if pipeline.is_some() {
                    return Err(Failure::Usage(
                        "two pipelines were given, and this takes one".into(),
                    ));
                }
                pipeline = Some(other.to_string())
            }
        }
    }

    // No pipeline on the command line means it is arriving on stdin, which is
    // how a launcher will usually call this: a heredoc or a pipe keeps the text
    // out of the shell's quoting rules entirely.
    let pipeline = match pipeline {
        Some(p) => p,
        None => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|e| Failure::Usage(format!("could not read the pipeline from stdin: {e}")))?;
            if buffer.trim().is_empty() {
                return Err(Failure::Usage("no pipeline was given".into()));
            }
            buffer
        }
    };

    // Which table does this pipeline read? A launcher has to know before it can
    // describe anything, and the alternative is for every launcher to pick the
    // first word out of the text itself — which is parsing, in a host, twice
    // over, and the two would drift the first time a pipeline could name more
    // than one table. So the grammar answers it.
    if needs_only {
        let plan = god_core::parse::parse(&pipeline)
            .map_err(|d| Failure::Refused(d.render(&pipeline)))?;
        return Ok(format!("{}\n", plan.tables().join("\n")));
    }

    let columns = columns.ok_or_else(|| {
        Failure::Usage(
            "`--columns` is required. The grammar checks a pipeline against the table it will run on, and it cannot do that without knowing the columns".into(),
        )
    })?;
    let schema = read_schema(&columns)?;

    let mut others = Vec::new();
    for (table, list) in &named {
        others.push((table.clone(), read_schema(list)?));
    }
    let others = god_core::check::Tables::new(others);

    // **The drawing answers a different question and so it takes a different
    // route.** Every other path here ends in a refusal or a query; this one ends
    // in a picture either way, because a pipeline that will not run is exactly
    // when someone wants to see how far it got. The assumptions are not repeated
    // on stderr: they are drawn, on the band that made them.
    if let Some(way) = drawing {
        if wanted.is_some() {
            return Err(Failure::Usage(
                "`--draw` and `--as` answer different questions, so this takes one or the other. `--as` writes the pipeline out in another language; `--draw` shows what it does to the table".into(),
            ));
        }
        return god_core::draw_tables(&pipeline, &schema, &others, &way)
            .map_err(|d| Failure::Refused(d.render(&pipeline)));
    }

    let compiled = god_core::compile_tables(&pipeline, &schema, &others, wanted.as_deref().unwrap_or("sql"))
        .map_err(|d| Failure::Refused(d.render(&pipeline)))?;

    // An assumption is not a failure and does not stop anything, but it is never
    // silent: the grammar chose something the caller did not say, and saying so
    // is the difference between a tool that is trusted and one that is worked
    // around.
    for note in &compiled.assumptions {
        eprintln!("{}", note.render(&pipeline));
    }

    let mut out = compiled.text;
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Split `products=id:number` into the table and its columns.
///
/// The `=` has to come before the first `:`, or `region:text` would read as a
/// table called `region`. A column list always has a `:` in it and a table name
/// never does, which makes the test exact rather than a guess.
fn split_named(value: &str) -> Option<(String, String)> {
    let equals = value.find('=')?;
    if let Some(colon) = value.find(':') {
        if colon < equals {
            return None;
        }
    }
    Some((value[..equals].to_string(), value[equals + 1..].to_string()))
}

/// `region:text,revenue:number` into a schema.
///
/// A type the grammar does not recognize is read as one it has no opinion about
/// rather than refused. The host knows its own type system and this list does
/// not, so refusing an unfamiliar word would refuse working pipelines over
/// columns the grammar has simply never met.
fn read_schema(list: &str) -> Result<Schema, Failure> {
    let mut columns = Vec::new();
    for piece in list.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        let (name, kind) = match piece.rsplit_once(':') {
            Some((name, kind)) => (name.trim(), kind.trim()),
            None => {
                return Err(Failure::Usage(format!(
                    "`{piece}` has no type. Write each column as name:type, like `revenue:number`"
                )))
            }
        };
        if name.is_empty() {
            return Err(Failure::Usage(format!("`{piece}` has no column name")));
        }
        let kind = match kind.to_ascii_lowercase().as_str() {
            "text" | "string" | "character" | "varchar" => Type::Text,
            "number" | "numeric" | "double" | "integer" | "int" | "float" => Type::Number,
            "truth" | "boolean" | "bool" | "logical" => Type::Truth,
            "date" => Type::Date,
            // **Told apart from a plain date for one word's sake, `hour`.**
            // Everything else treats the two as one kind, and `kind` reports
            // both as "date".
            "timestamp" | "datetime" | "posixct" => Type::Timestamp,
            _ => Type::Unknown,
        };
        columns.push((name.to_string(), kind));
    }
    if columns.is_empty() {
        return Err(Failure::Usage(
            "`--columns` was empty. Write the table's columns as name:type, separated by commas".into(),
        ));
    }
    Ok(Schema::new(columns))
}
