//! The two reshaping verbs, end to end, over real tables.
//!
//! **The assertions are about rows, not about the query.** A pivot is exactly
//! the kind of thing where the generated SQL can look entirely reasonable and
//! return the wrong shape, so every test here reads what came back.
//!
//! Two of them run a query that is *supposed* to stop. `widen` is the first
//! place the grammar refuses from inside the query rather than from the gate,
//! because whether two rows want the same cell is a property of the data and the
//! checker never sees a row. A refusal that only exists in a comment is not one,
//! so those two execute for real and read the message.

use duckdb::Connection;
use god_core::{compile, Schema, Type};

/// A survey in the shape people actually receive one: a row per person, a column
/// per question. Small enough to work out by hand.
fn answers() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE answers (student VARCHAR, q1 BIGINT, q2 BIGINT, q3 BIGINT);
         INSERT INTO answers VALUES ('ann', 1, 2, 3), ('bob', 4, 5, 6);",
    )
    .expect("the fixture table");
    conn
}

/// The same survey already stacked, which is what `widen` reads.
fn stacked() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE marks (student VARCHAR, question VARCHAR, mark BIGINT);
         INSERT INTO marks VALUES
             ('ann', 'q1', 1), ('ann', 'q2', 2), ('bob', 'q1', 4);",
    )
    .expect("the fixture table");
    conn
}

fn describe(conn: &Connection, query: &str) -> Vec<(String, Type)> {
    let mut stmt = conn
        .prepare(&format!("DESCRIBE {query}"))
        .unwrap_or_else(|e| panic!("the query would not describe: {e}\n\n{query}\n"));
    stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .expect("describe rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("describe rows")
        .into_iter()
        .map(|(name, kind)| {
            let kind = match kind.split('(').next().unwrap_or("").trim() {
                "VARCHAR" | "TEXT" | "STRING" => Type::Text,
                "DOUBLE" | "FLOAT" | "REAL" | "DECIMAL" | "INTEGER" | "BIGINT" | "SMALLINT"
                | "TINYINT" | "HUGEINT" | "UBIGINT" | "UINTEGER" => Type::Number,
                "BOOLEAN" => Type::Truth,
                _ => Type::Unknown,
            };
            (name, kind)
        })
        .collect()
}

fn schema_of(conn: &Connection, table: &str) -> Schema {
    Schema::new(describe(conn, &format!("SELECT * FROM {table}")))
}

/// Run a pipeline and hand back the columns and the rows as text.
///
/// The engine's own account of the columns is compared against the checker's,
/// which is the assertion that matters most here: `widen` is the one verb whose
/// output schema the grammar has to *predict* rather than derive, so a
/// disagreement is exactly the defect this file exists to catch.
fn run(conn: &Connection, table: &str, pipeline: &str) -> (Vec<String>, Vec<Vec<String>>) {
    run_where(conn, table, pipeline, true)
}

/// The same, for a pipeline ending in a `widen` that declares nothing.
///
/// **That is the one case where the grammar does not claim to know the answer's
/// columns**, because they are in the data and the query has not run yet. The
/// checker hands back the columns it does know — the ones saying which rows go
/// together — and no step may follow, so a partial answer misleads nobody. This
/// helper exists so that the assertion in `run` can stay strict everywhere else
/// rather than being loosened to accommodate the one honest exception.
fn run_open(conn: &Connection, table: &str, pipeline: &str) -> (Vec<String>, Vec<Vec<String>>) {
    run_where(conn, table, pipeline, false)
}

fn run_where(
    conn: &Connection,
    table: &str,
    pipeline: &str,
    schema_is_known: bool,
) -> (Vec<String>, Vec<Vec<String>>) {
    let schema = schema_of(conn, table);
    let compiled = match compile(pipeline, &schema, "sql") {
        Ok(c) => c,
        Err(d) => panic!("\n{}\n", d.render(pipeline)),
    };

    let described = describe(conn, &compiled.text);
    let names: Vec<String> = described.iter().map(|(n, _)| n.clone()).collect();
    if schema_is_known {
        assert_eq!(
            names,
            compiled.schema.names(),
            "the checker and the engine disagree about the columns\n\n{}\n",
            compiled.text
        );
    } else {
        assert!(
            names.starts_with(&compiled.schema.names()),
            "the checker named columns the engine did not produce\n\n{}\n",
            compiled.text
        );
    }

    let count = names.len();
    let mut stmt = conn
        .prepare(&compiled.text)
        .unwrap_or_else(|e| panic!("the query would not prepare: {e}\n\n{}\n", compiled.text));
    let rows = stmt
        .query_map([], |row| {
            (0..count)
                .map(|i| {
                    Ok(match row.get_ref(i)? {
                        duckdb::types::ValueRef::Null => "missing".to_string(),
                        duckdb::types::ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                        other => format!("{:?}", duckdb::types::Value::from(other))
                            .replace("Double(", "")
                            .replace("BigInt(", "")
                            .replace("Int(", "")
                            .replace(')', ""),
                    })
                })
                .collect::<Result<Vec<String>, duckdb::Error>>()
        })
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    (names, rows)
}

/// The message a pipeline was refused with, whether the gate or the engine said
/// it. Both are refusals of the same sentence and a test should not have to care
/// which one spoke.
fn refusal(conn: &Connection, table: &str, pipeline: &str) -> String {
    let schema = schema_of(conn, table);
    let compiled = match compile(pipeline, &schema, "sql") {
        Ok(c) => c,
        Err(d) => return d.message,
    };
    match conn.prepare(&compiled.text).and_then(|mut s| {
        s.query_map([], |_| Ok(()))?.collect::<Result<Vec<_>, _>>()
    }) {
        Ok(_) => panic!("this was supposed to be refused and it ran:\n\n{pipeline}\n"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn lengthen_gives_the_rows_worked_out_by_hand() {
    let conn = answers();
    let (names, rows) = run(&conn, "answers", "answers then lengthen [q1, q2, q3]");

    // The two new columns are called `name` and `value` with nothing said,
    // because those are the grammar's own words for what a column is called and
    // what it holds.
    assert_eq!(names, ["student", "name", "value"]);

    // Six rows, and each original row's new rows together: that is what ordering
    // by every column left to right buys, and it is what `cols_vary` is for
    // elsewhere.
    assert_eq!(
        rows,
        vec![
            vec!["ann", "q1", "1"],
            vec!["ann", "q2", "2"],
            vec!["ann", "q3", "3"],
            vec!["bob", "q1", "4"],
            vec!["bob", "q2", "5"],
            vec!["bob", "q3", "6"],
        ]
    );
}

/// **The two verbs are inverses, and this is the sentence that says so.**
///
/// It is worth a test rather than only a chapter: the defaults on both sides are
/// what make it spellable with no arguments at all, and a change to either one
/// breaks the property silently.
#[test]
fn the_round_trip_gives_back_the_table_it_started_from() {
    let conn = answers();
    let (before_names, before_rows) = run(&conn, "answers", "answers then sort [student]");
    let (after_names, after_rows) =
        run_open(&conn, "answers", "answers then lengthen [q1, q2, q3] then widen");

    assert_eq!(before_names, after_names);
    assert_eq!(before_rows, after_rows);
}

#[test]
fn a_pattern_splits_one_name_into_two_columns() {
    let conn = Connection::open_in_memory().expect("a database");
    conn.execute_batch(
        "CREATE TABLE t (id BIGINT, q1_2020 BIGINT, q1_2021 BIGINT);
         INSERT INTO t VALUES (1, 10, 11);",
    )
    .expect("the fixture");

    let (names, rows) = run(
        &conn,
        "t",
        r#"t then lengthen all_but [id] as name "{question}_{year}", value [answer]"#,
    );
    assert_eq!(names, ["id", "question", "year", "answer"]);
    assert_eq!(rows, vec![vec!["1", "q1", "2020", "10"], vec!["1", "q1", "2021", "11"]]);
}

/// `{value}` is tidyr's `.value`, and it is the piece of `pivot_longer` people
/// reliably have to look up twice. Here it is the word the grammar already uses.
#[test]
fn the_value_piece_makes_one_column_for_each_thing_measured() {
    let conn = Connection::open_in_memory().expect("a database");
    conn.execute_batch(
        "CREATE TABLE r (id BIGINT, x_mean DOUBLE, x_sd DOUBLE, y_mean DOUBLE, y_sd DOUBLE);
         INSERT INTO r VALUES (1, 1.0, 0.5, 2.0, 0.25);",
    )
    .expect("the fixture");

    let (names, rows) =
        run(&conn, "r", r#"r then lengthen all_but [id] as name "{sensor}_{value}""#);
    assert_eq!(names, ["id", "sensor", "mean", "sd"]);
    assert_eq!(rows, vec![vec!["1", "x", "1.0", "0.5"], vec!["1", "y", "2.0", "0.25"]]);
}

/// The fourth place the vocabulary applies this rule, after `join`'s keys,
/// `fill_missing`'s filler and `first_present`'s arguments.
#[test]
fn stacking_two_kinds_of_column_is_refused() {
    let conn = answers();
    let message = refusal(&conn, "answers", "answers then lengthen [student, q1]");
    assert!(
        message.contains("two kinds of thing in one column"),
        "the message does not say what is wrong: {message}"
    );
    assert!(
        message.contains("[student]") && message.contains("[q1]"),
        "the message does not name both columns: {message}"
    );
}

/// **The refusal `pivot_wider` will not make.** It warns and hands back a
/// list-column, which keeps the pipeline running and gives downstream a shape
/// nothing expects.
#[test]
fn two_rows_wanting_one_cell_stops_the_query() {
    let conn = stacked();
    conn.execute_batch("INSERT INTO marks VALUES ('ann', 'q1', 99);")
        .expect("a second answer for one cell");

    let message = refusal(
        &conn,
        "marks",
        "marks then widen name [question], value [mark] by [student]",
    );
    assert!(
        message.contains("two rows want the same cell"),
        "the message does not name the collision: {message}"
    );
    assert!(
        message.contains("value average(...)"),
        "the message does not name the fix: {message}"
    );
    // The manual prints this message, so it is held to the book's rules too.
    assert!(!message.contains('—'), "a refusal the book prints has an em dash in it");
}

/// The same data with an aggregate written: the collision was a question, and
/// answering it is a value rather than a new word.
#[test]
fn an_aggregate_says_what_to_do_about_the_collision() {
    let conn = stacked();
    conn.execute_batch("INSERT INTO marks VALUES ('ann', 'q1', 99);")
        .expect("a second answer for one cell");

    let (names, rows) = run(
        &conn,
        "marks",
        "marks then widen name [question], value average([mark]) by [student] giving [q1, q2]",
    );
    assert_eq!(names, ["student", "q1", "q2"]);
    assert_eq!(rows, vec![vec!["ann", "50.0", "2.0"], vec!["bob", "4.0", "missing"]]);
}

/// Declaring a schema the data then contradicts is worse than not declaring one,
/// so the value that was left out stops the query instead of vanishing from it.
#[test]
fn a_value_that_giving_does_not_list_stops_the_query() {
    let conn = stacked();
    let message = refusal(
        &conn,
        "marks",
        "marks then widen name [question], value [mark] by [student] giving [q1]",
    );
    assert!(
        message.contains("`giving` does not list"),
        "the message does not say what happened: {message}"
    );
}

#[test]
fn empty_cells_are_missing_unless_something_says_otherwise() {
    let conn = stacked();
    let (_, plain) = run(
        &conn,
        "marks",
        "marks then widen name [question], value [mark] by [student] giving [q1, q2]",
    );
    assert_eq!(plain, vec![vec!["ann", "1", "2"], vec!["bob", "4", "missing"]]);

    let (_, filled) = run(
        &conn,
        "marks",
        "marks then widen name [question], value [mark] by [student] missing 0 giving [q1, q2]",
    );
    assert_eq!(filled, vec![vec!["ann", "1", "2"], vec!["bob", "4", "0"]]);
}

/// The whole reason `giving` exists: with the columns written down, everything
/// after a `widen` is checked by exactly the code that checks everything else.
#[test]
fn a_widen_that_declares_can_be_carried_on_from() {
    let conn = stacked();
    let (names, rows) = run(
        &conn,
        "marks",
        "marks then widen name [question], value [mark] by [student] missing 0 giving [q1, q2] then add [gain] as [q2] - [q1]",
    );
    assert_eq!(names, ["student", "q1", "q2", "gain"]);
    assert_eq!(rows, vec![vec!["ann", "1", "2", "1"], vec!["bob", "4", "0", "-4"]]);
}

#[test]
fn a_step_after_a_bare_widen_is_refused_and_names_the_fix() {
    let conn = stacked();
    let message = refusal(
        &conn,
        "marks",
        "marks then widen name [question], value [mark] by [student] then take 1",
    );
    assert!(
        message.contains("giving [q1, q2, q3]"),
        "the message does not name the fix: {message}"
    );
}

/// One clause needing another, rather than a rule of its own: saying what an
/// empty cell holds means knowing which cells there are.
#[test]
fn filling_empty_cells_needs_the_columns_named() {
    let conn = stacked();
    let message = refusal(
        &conn,
        "marks",
        "marks then widen name [question], value [mark] by [student] missing 0",
    );
    assert!(
        message.contains("which cells there are"),
        "the message does not explain the tie: {message}"
    );
}

/// The three ways `pick` chooses columns are the three ways `lengthen` chooses
/// them, which is what made the commonest reshaping of all cost nothing.
#[test]
fn lengthen_chooses_columns_the_way_pick_does() {
    let conn = answers();
    let listed = run(&conn, "answers", "answers then lengthen [q1, q2, q3]");
    let inverted = run(&conn, "answers", "answers then lengthen all_but [student]");
    let matched = run(&conn, "answers", r#"answers then lengthen where name starts "q""#);

    assert_eq!(listed, inverted);
    assert_eq!(listed, matched);
}

/// A pattern that matches nothing is a typo every time, and an empty answer is
/// not a useful one.
#[test]
fn a_column_that_does_not_fit_the_pattern_is_refused() {
    let conn = answers();
    let message = refusal(
        &conn,
        "answers",
        r#"answers then lengthen [q1, q2, q3] as name "{a}_{b}", value [v]"#,
    );
    assert!(
        message.contains("does not look like"),
        "the message does not say what failed: {message}"
    );
}

/// The defaults walk straight into this, so it has its own message: a table with
/// a column called `name` is exactly the table `lengthen [a, b]` would give two
/// of.
#[test]
fn making_a_column_the_table_already_has_is_refused() {
    let conn = Connection::open_in_memory().expect("a database");
    conn.execute_batch(
        "CREATE TABLE t (name VARCHAR, q1 BIGINT, q2 BIGINT); INSERT INTO t VALUES ('a', 1, 2);",
    )
    .expect("the fixture");

    let message = refusal(&conn, "t", "t then lengthen [q1, q2]");
    assert!(
        message.contains("already has a column called `name`"),
        "the message does not name the clash: {message}"
    );
    assert!(
        message.contains("as name [question], value [answer]"),
        "the message does not name the fix: {message}"
    );
}
