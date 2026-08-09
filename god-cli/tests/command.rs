//! The command line, exercised as a caller would.
//!
//! **These tests run the built binary**, not a function inside it. A launcher
//! sees a process: an exit code, something on stdout, something on stderr. That
//! is the contract, and testing the library instead would leave the contract
//! itself unchecked — which is how a binding ends up never reaching the engine
//! while every check in the tree is green.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn god(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_god-cli"))
        .args(args)
        .output()
        .expect("the binary runs")
}

fn god_stdin(args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_god-cli"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write");
    child.wait_with_output().expect("output")
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}
fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

const COLUMNS: &str = "region:text,product:text,revenue:number,cost:number";

#[test]
fn a_pipeline_on_the_command_line_becomes_a_query() {
    let o = god(&[
        "--columns",
        COLUMNS,
        "sales then keep where [region] is \"West\" then take 10",
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", err(&o));
    assert_eq!(
        out(&o),
        "WITH step0 AS (SELECT * FROM \"sales\"),\n     \
         step1 AS (SELECT * FROM step0 WHERE (\"region\" = 'West')),\n     \
         step2 AS (SELECT * FROM step1 LIMIT 10)\n\
         SELECT * FROM step2\n"
    );
}

/// The way a launcher will usually call this: the text arrives on stdin, so the
/// shell's quoting rules never touch it.
#[test]
fn a_pipeline_on_stdin_works_the_same() {
    let o = god_stdin(
        &["--columns", COLUMNS],
        "sales\n  then keep where [region] is \"West\"\n  then take 10\n",
    );
    assert_eq!(o.status.code(), Some(0), "stderr: {}", err(&o));
    assert!(out(&o).contains("LIMIT 10"), "{}", out(&o));
}

#[test]
fn another_backend_is_one_flag() {
    let o = god(&[
        "--columns",
        COLUMNS,
        "--as",
        "dplyr",
        "sales then keep where [region] is \"West\" then take 10",
    ]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", err(&o));
    assert_eq!(
        out(&o),
        "sales |>\n  filter((region == \"West\")) |>\n  head(10)\n"
    );
}

/// A launcher has to know which table to describe before it can describe one,
/// and the alternative is for every launcher to pick the first word out of the
/// pipeline itself. That is parsing, in a host, once per language, and the
/// copies would differ the first time a pipeline could name two tables.
#[test]
fn the_grammar_says_which_table_a_pipeline_reads() {
    let o = god(&["--needs", "sales then keep where [a] > 1 then take 10"]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", err(&o));
    assert_eq!(out(&o), "sales\n");
}

#[test]
fn asking_what_a_pipeline_needs_does_not_require_a_schema() {
    // It runs before anything is known about the table, which is the whole point
    // of the question.
    let o = god(&["--needs", "anything then take 1"]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", err(&o));
}

#[test]
fn a_pipeline_that_will_not_parse_is_refused_even_when_only_asked_what_it_needs() {
    let o = god(&["--needs", "sales then keeep where [a] is 1"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("Did you mean `keep`?"), "{}", err(&o));
}

// -- the failure contract --------------------------------------------------

#[test]
fn a_refused_pipeline_exits_two_and_explains_itself_on_stderr() {
    let o = god(&["--columns", COLUMNS, "sales then keep where [reveune] > 1"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(out(&o), "", "a refusal writes nothing to stdout");
    let e = err(&o);
    assert!(e.contains("there is no column called `reveune`"), "{e}");
    assert!(e.contains("Did you mean `revenue`?"), "{e}");
    assert!(e.contains("^^^^^^^"), "the caret is part of the message:\n{e}");
}

#[test]
fn a_missing_schema_is_a_usage_error_not_a_refusal() {
    let o = god(&["sales then take 10"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("`--columns` is required"), "{}", err(&o));
}

#[test]
fn a_malformed_column_list_says_how_to_write_one() {
    let o = god(&["--columns", "region", "sales then take 10"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("has no type"), "{}", err(&o));
    assert!(err(&o).contains("name:type"), "{}", err(&o));
}

/// **This test named `pandas` as its unknown backend until pandas was built**,
/// which is the shape worth noticing rather than the one-word fix: a test whose
/// example is "something the grammar does not have" stops testing anything the
/// day the grammar has it. A misspelling can never become a real name, and it
/// checks the more useful half anyway, which is that the message points at what
/// the caller meant.
#[test]
fn an_unknown_backend_lists_the_real_ones() {
    let o = god(&["--columns", COLUMNS, "--as", "panda", "sales then take 10"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("there is no backend called `panda`"), "{}", err(&o));
    assert!(err(&o).contains("Did you mean `pandas`?"), "{}", err(&o));
}

#[test]
fn an_unfamiliar_column_type_is_accepted_rather_than_refused() {
    // The host knows its own type system and this list does not, so an unknown
    // word is a type the grammar has no opinion about — not an error.
    let o = god(&["--columns", "id:uuid,n:number", "t then keep where [n] > 1"]);
    assert_eq!(o.status.code(), Some(0), "stderr: {}", err(&o));
}

/// Given twice is refused rather than last-one-wins. A flag the tool quietly
/// dropped would be the same defect as a clause the grammar quietly dropped,
/// and the grammar refuses those.
#[test]
fn a_repeated_as_flag_is_refused() {
    let o = god(&["--columns", COLUMNS, "--as", "dplyr", "--as", "sql", "sales then take 1"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("`--as` was given twice"), "{}", err(&o));
}

#[test]
fn a_second_schema_for_the_head_table_is_refused() {
    let o = god(&["--columns", "a:number", "--columns", "b:number", "t then take 1"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("described the head table twice"), "{}", err(&o));
}

#[test]
fn two_descriptions_of_one_named_table_are_refused() {
    let o = god(&[
        "--columns",
        COLUMNS,
        "--columns",
        "products=id:number",
        "--columns",
        "products=id:text",
        "sales then take 1",
    ]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("described `products` twice"), "{}", err(&o));
}

#[test]
fn help_exits_zero_and_shows_an_example() {
    let o = god(&["--help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("--columns"), "{}", out(&o));
    assert!(out(&o).contains("then keep where"), "the help shows a real pipeline");
}

// -- the guard has to be able to fail --------------------------------------

/// Proof that these assertions read the process rather than merely starting it.
#[test]
#[should_panic(expected = "assertion")]
fn the_command_line_assertions_can_fail() {
    let o = god(&["--columns", COLUMNS, "sales then take 1"]);
    assert_eq!(out(&o), "this is not what the binary prints");
}
