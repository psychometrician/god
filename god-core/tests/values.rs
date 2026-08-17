//! The expressions, end to end over real tables.
//!
//! **What is checked here is the type rules and the answers**, because those are
//! the two things a walk over the vocabulary cannot check. `grammar.rs` already
//! proves every function is spelled by every backend; that it is spelled
//! *correctly* is only knowable by reading rows back.
//!
//! The harness is smaller than `reshape.rs`'s on purpose. That one compares the
//! checker's account of the columns against the engine's, because `widen` has to
//! predict a schema; an expression makes one ordinary column and has nothing to
//! predict.

use duckdb::Connection;
use god_core::{compile, Schema, Type};


fn pupils() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE pupils (name VARCHAR, score BIGINT);
         INSERT INTO pupils VALUES ('ann', 95), ('bob', 75), ('cat', 50);",
    )
    .expect("the fixture table");
    conn
}

/// **An aggregate in `add` is a window even with no `by`.** The group is the
/// whole table, and the SQL still has to say `OVER ()`: the bare aggregate
/// this used to render made the engine demand a `GROUP BY` nobody wrote.
/// Found by a cookbook recipe computing a share of the whole.
#[test]
fn a_share_of_the_whole_needs_no_by() {
    let conn = pupils();
    let (names, rows) = run(
        &conn,
        "pupils",
        "pupils then add [share] as [score] / total([score]) then keep where [share] > 0.4 then pick [name]",
    );
    assert_eq!(names, ["name"]);
    // Worked by hand: the total is 220, and 95 over it is the one share past
    // 0.4. The float itself stays out of the assertion on purpose; its text
    // form belongs to the driver.
    assert_eq!(rows, vec![vec!["ann"]]);
}

/// The same aggregate under a `by` spans its group instead, and each row keeps
/// its group's answer.
#[test]
fn a_share_of_the_group_takes_the_by() {
    let conn = pupils();
    let (_, rows) = run(
        &conn,
        "pupils",
        r#"pupils then add [band] as when([score] >= 90, "A", otherwise "B") then add [share] as [score] / total([score]) by [band] then keep where [share] is 1 then pick [name, band]"#,
    );
    // ann is alone in band A, so her share of it is exactly one; bob and cat
    // split band B and neither reaches it.
    assert_eq!(rows, vec![vec!["ann", "A"]]);
}

#[test]
fn the_first_question_that_is_true_wins() {
    let conn = pupils();
    let (names, rows) = run(
        &conn,
        "pupils",
        r#"pupils then add [band] as when([score] >= 90, "A", [score] >= 70, "B", otherwise "C") then sort [name]"#,
    );
    assert_eq!(names, ["name", "score", "band"]);
    assert_eq!(
        rows,
        vec![
            vec!["ann", "95", "A"],
            vec!["bob", "75", "B"],
            vec!["cat", "50", "C"],
        ]
    );
}

/// **Order is the meaning**, so the same questions in the other order answer
/// differently, and that is the property rather than a trap. A test for it
/// because it is the one thing about a conditional people get wrong.
#[test]
fn the_order_of_the_questions_is_the_meaning() {
    let conn = pupils();
    let (_, rows) = run(
        &conn,
        "pupils",
        r#"pupils then add [band] as when([score] >= 70, "B", [score] >= 90, "A", otherwise "C") then sort [name]"#,
    );
    // `ann` scores 95, matches the 70 test first, and gets B. Nothing is wrong
    // with that: it is what the sentence says.
    assert_eq!(rows[0], vec!["ann", "95", "B"]);
}

#[test]
fn a_row_matching_nothing_is_missing_unless_otherwise_says() {
    let conn = pupils();
    let (_, rows) = run(
        &conn,
        "pupils",
        r#"pupils then add [top] as when([score] >= 90, "yes") then sort [name]"#,
    );
    assert_eq!(rows[0][2], "yes");
    assert_eq!(rows[1][2], "missing");
    assert_eq!(rows[2][2], "missing");
}

#[test]
fn every_answer_has_to_be_the_same_kind_of_thing() {
    let conn = pupils();
    let message = refusal(
        &conn,
        "pupils",
        r#"pupils then add [band] as when([score] >= 90, "A", otherwise 0)"#,
    );
    assert!(
        message.contains("same kind of thing"),
        "the message does not say what is wrong: {message}"
    );
}

#[test]
fn a_question_that_is_not_a_question_is_refused() {
    let conn = pupils();
    let message = refusal(
        &conn,
        "pupils",
        r#"pupils then add [band] as when([score], "A", otherwise "C")"#,
    );
    assert!(
        message.contains("yes or no"),
        "the message does not say what a question is: {message}"
    );
}

#[test]
fn a_question_with_no_answer_beside_it_is_refused() {
    let conn = pupils();
    let message = refusal(
        &conn,
        "pupils",
        r#"pupils then add [band] as when([score] >= 90, "A", [score] >= 70)"#,
    );
    assert!(
        message.contains("needs the answer that goes with it"),
        "the message does not name the shape: {message}"
    );
}

/// An answer written after the catch-all could never be reached, and quietly
/// ignoring it is the kind of thing the grammar refuses elsewhere.
#[test]
fn nothing_may_follow_otherwise() {
    let conn = pupils();
    let message = refusal(
        &conn,
        "pupils",
        r#"pupils then add [band] as when([score] >= 90, "A", otherwise "C", [score] >= 70, "B")"#,
    );
    assert!(
        message.contains("could never be reached"),
        "the message does not say why: {message}"
    );
}

/// `summarize` refuses a value that does not collapse a group, and it has to see
/// *inside* a `when` to know. That works only because `Expr::walk` recurses into
/// the variant; without it every rule that asks `aggregates()` would quietly
/// stop applying to conditionals.
#[test]
fn the_rules_that_look_inside_a_value_look_inside_a_when() {
    let conn = pupils();
    let plain = refusal(
        &conn,
        "pupils",
        r#"pupils then summarize [band] as when([score] >= 90, "A", otherwise "C")"#,
    );
    assert!(
        plain.contains("spans the group"),
        "summarize did not see inside the conditional: {plain}"
    );

    // And an aggregate written inside one is seen, so this is allowed.
    let (_, rows) = run(
        &conn,
        "pupils",
        r#"pupils then summarize [how] as when(average([score]) > 70, "high", otherwise "low")"#,
    );
    assert_eq!(rows, vec![vec!["high"]]);
}

// -- the harness ------------------------------------------------------------

fn schema_of(conn: &Connection, table: &str) -> Schema {
    let mut stmt = conn.prepare(&format!("DESCRIBE SELECT * FROM {table}")).expect("describe");
    let columns = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .expect("describe rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("describe rows");
    Schema::new(columns.into_iter().map(|(name, kind)| {
        let kind = match kind.split('(').next().unwrap_or("").trim() {
            "VARCHAR" | "TEXT" | "STRING" => Type::Text,
            "BIGINT" | "INTEGER" | "DOUBLE" | "DECIMAL" | "HUGEINT" => Type::Number,
            "BOOLEAN" => Type::Truth,
            _ => Type::Unknown,
        };
        (name, kind)
    }))
}

/// Run a pipeline and hand back the columns and the rows as text.
fn run(conn: &Connection, table: &str, pipeline: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let compiled = match compile(pipeline, &schema_of(conn, table), "sql") {
        Ok(c) => c,
        Err(d) => panic!("\n{}\n", d.render(pipeline)),
    };
    let names = compiled.schema.names();
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
                        // **Unwrap whatever the driver named the type**, rather
                        // than listing the names. `sum()` hands back a HUGEINT
                        // on DuckDB, and a list of prefixes is a list that goes
                        // stale the first time a function returns a width nobody
                        // thought of.
                        other => {
                            let shown = format!("{:?}", duckdb::types::Value::from(other));
                            match (shown.find('('), shown.rfind(')')) {
                                (Some(a), Some(b)) if b > a => shown[a + 1..b].to_string(),
                                _ => shown,
                            }
                        }
                    })
                })
                .collect::<Result<Vec<String>, duckdb::Error>>()
        })
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    (names, rows)
}

/// The message a pipeline was refused with, from the gate or from the engine.
fn refusal(conn: &Connection, table: &str, pipeline: &str) -> String {
    let compiled = match compile(pipeline, &schema_of(conn, table), "sql") {
        Ok(c) => c,
        Err(d) => return d.message,
    };
    match conn
        .prepare(&compiled.text)
        .and_then(|mut s| s.query_map([], |_| Ok(()))?.collect::<Result<Vec<_>, _>>())
    {
        Ok(_) => panic!("this was supposed to be refused and it ran:\n\n{pipeline}\n"),
        Err(e) => e.to_string(),
    }
}

// -- converting, text, and between ------------------------------------------

fn messy() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE people (raw VARCHAR, n BIGINT);
         INSERT INTO people VALUES ('  ann marie  ', 7), ('  bob  ', 99);",
    )
    .expect("the fixture table");
    conn
}

#[test]
fn the_text_functions_give_the_answers_worked_out_by_hand() {
    let conn = messy();
    let (names, rows) = run(
        &conn,
        "people",
        r#"people then add [name] as trim([raw]) then add [first] as split_text([name], " ", 1), [size] as characters([name]), [fixed] as replace_text([name], "a", "A") then pick [first, size, fixed] then sort [first]"#,
    );
    assert_eq!(names, ["first", "size", "fixed"]);
    assert_eq!(
        rows,
        vec![
            vec!["ann", "9", "Ann mArie"],
            vec!["bob", "3", "bob"],
        ]
    );
}

/// **`split_text` counts pieces from 1 and says which one**, because every value
/// in the grammar is one value and there is no list to hand back. That is the
/// one place this diverges from what §4.7 first wrote down.
#[test]
fn split_text_says_which_piece_it_wants() {
    let conn = messy();
    let (_, rows) = run(
        &conn,
        "people",
        r#"people then add [second] as split_text(trim([raw]), " ", 2) then pick [second] then sort [second]"#,
    );
    // **`bob` has no second word, and the answer is empty text rather than
    // missing.** Both engines agree on that, checked rather than assumed, so it
    // is asserted here: a reader deciding whether to test the result with
    // `is missing` needs to know it never will be.
    assert_eq!(rows, vec![vec![""], vec!["marie"]]);
}

#[test]
fn between_counts_both_ends() {
    let conn = messy();
    let (_, rows) = run(&conn, "people", "people then keep where between([n], 7, 99) then pick [n] then sort [n]");
    assert_eq!(rows, vec![vec!["7"], vec!["99"]]);

    let (_, narrower) = run(&conn, "people", "people then keep where between([n], 8, 98) then pick [n]");
    assert!(narrower.is_empty(), "both ends are counted, so nothing is between 8 and 98 here");
}

#[test]
fn a_conversion_says_what_it_gives_whatever_it_was_handed() {
    let conn = messy();
    let (_, rows) = run(
        &conn,
        "people",
        "people then add [word] as to_text([n]) then add [longer] as characters([word]) then pick [longer] then sort [longer]",
    );
    // `characters` needs text, and it only reaches it because `to_text` said so.
    assert_eq!(rows, vec![vec!["1"], vec!["2"]]);
}

#[test]
fn text_functions_refuse_a_number_and_name_the_conversion() {
    let conn = messy();
    for (word, sentence) in [
        ("trim", "people then add [x] as trim([n])"),
        ("characters", "people then add [x] as characters([n])"),
    ] {
        let message = refusal(&conn, "people", sentence);
        assert!(
            message.contains("to_text(...)"),
            "`{word}` does not name the conversion: {message}"
        );
    }
}

#[test]
fn between_needs_all_three_to_be_the_same_kind_of_thing() {
    let conn = messy();
    let message = refusal(&conn, "people", r#"people then keep where between([n], 1, "ten")"#);
    assert!(
        message.contains("same kind of thing"),
        "the message does not say what is wrong: {message}"
    );
}

/// Three named roles, so it may not be variadic. The arity comes from the
/// vocabulary rather than from anything written here.
#[test]
fn between_takes_exactly_three() {
    let conn = messy();
    let message = refusal(&conn, "people", "people then keep where between([n], 1)");
    assert!(message.contains("3 columns"), "the message does not say what it wants: {message}");
}

// -- dates, and looking along the rows --------------------------------------

fn diary() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE diary (g VARCHAR, on_ VARCHAR, x BIGINT);
         INSERT INTO diary VALUES
             ('a', '2026-01-02', 10), ('a', '2026-01-05', 20), ('b', '2026-01-06', 30);",
    )
    .expect("the fixture table");
    conn
}

/// **Monday is 1, and it is the grammar's numbering rather than an engine's.**
/// Asked plainly, DuckDB calls 2026-01-02 a 5 and Spark calls it a 4, with
/// nothing raised either way. The dates below span a whole week so a wrong
/// numbering cannot hide behind a lucky day.
#[test]
fn weekday_counts_monday_as_one() {
    let conn = Connection::open_in_memory().expect("a database");
    conn.execute_batch(
        "CREATE TABLE week (d VARCHAR);
         INSERT INTO week VALUES ('2026-01-05'),('2026-01-06'),('2026-01-07'),
                                 ('2026-01-08'),('2026-01-09'),('2026-01-10'),('2026-01-11');",
    )
    .expect("a week");
    let (_, rows) = run(
        &conn,
        "week",
        "week then add [wd] as weekday(to_date([d])) then sort [d] then pick [wd]",
    );
    // Monday the 5th through Sunday the 11th.
    assert_eq!(
        rows,
        vec![vec!["1"], vec!["2"], vec!["3"], vec!["4"], vec!["5"], vec!["6"], vec!["7"]]
    );
}

#[test]
fn the_other_date_parts_read_what_they_say() {
    let conn = diary();
    let (names, rows) = run(
        &conn,
        "diary",
        "diary then add [d] as to_date([on_]) then add [y] as year([d]), [m] as month([d]), [dd] as day([d]) then pick [y, m, dd] then sort [dd]",
    );
    assert_eq!(names, ["y", "m", "dd"]);
    assert_eq!(rows[0], vec!["2026", "1", "2"]);
}

#[test]
fn a_date_part_refuses_a_number_and_names_the_conversion() {
    let conn = diary();
    let message = refusal(&conn, "diary", "diary then add [y] as year([x])");
    assert!(
        message.contains("to_date(...)"),
        "the message does not name the conversion: {message}"
    );
}

#[test]
fn the_running_total_adds_up_as_it_goes() {
    let conn = diary();
    let (names, rows) = run(
        &conn,
        "diary",
        "diary then sort [on_] then add [so_far] as running_total([x]) then pick [on_, so_far]",
    );
    assert_eq!(names, ["on_", "so_far"]);
    assert_eq!(
        rows,
        vec![
            vec!["2026-01-02", "10"],
            vec!["2026-01-05", "30"],
            vec!["2026-01-06", "60"],
        ]
    );
}

/// `by` restarts it, and the rows come back in the order the `sort` asked for.
///
/// **That second half is the part worth asserting.** Computing a window groups
/// the rows to do it and nothing puts them back, so without the sort being said
/// again the same sentence returns the groups in different orders on different
/// engines, with every value identical.
#[test]
fn a_window_keeps_the_order_that_was_asked_for() {
    let conn = diary();
    let (_, rows) = run(
        &conn,
        "diary",
        "diary then sort [on_] then add [so_far] as running_total([x]) by [g] then pick [on_, so_far]",
    );
    assert_eq!(
        rows,
        vec![
            vec!["2026-01-02", "10"],
            vec!["2026-01-05", "30"],
            vec!["2026-01-06", "30"],
        ]
    );
}

#[test]
fn previous_and_following_look_one_row_each_way() {
    let conn = diary();
    let (_, rows) = run(
        &conn,
        "diary",
        "diary then sort [on_] then add [before] as previous([x]), [after] as following([x]) then pick [before, after]",
    );
    assert_eq!(
        rows,
        vec![
            vec!["missing", "20"],
            vec!["10", "30"],
            vec!["20", "missing"],
        ]
    );
}

/// **How far is an optional second argument**, and one row back stays the
/// default. A year-over-year comparison on monthly rows is `previous([x], 12)`,
/// which every neighbour can say — `shift(x, 12)`, `lag(x, n = 12)`, `LAG(x,
/// 12)` — and which god could not until 2026-08-16.
#[test]
fn previous_and_following_take_how_far_to_look() {
    let conn = diary();
    let (_, rows) = run(
        &conn,
        "diary",
        "diary then sort [on_] then add [back] as previous([x], 2), [on] as following([x], 2) then pick [back, on]",
    );
    assert_eq!(
        rows,
        vec![
            vec!["missing", "30"],
            vec!["missing", "missing"],
            vec!["10", "missing"],
        ]
    );
}

/// The offset is a plain whole number and cannot be worked out per row, which
/// is not a limitation of the checker: no engine underneath takes a column in
/// that position, so accepting one would mean writing a query that runs and
/// answers something else.
#[test]
fn how_far_has_to_be_a_written_number() {
    let conn = diary();
    let message = refusal(&conn, "diary", "diary then sort [on_] then add [v] as previous([x], [x])");
    assert!(message.contains("cannot be worked out per row"), "{message}");
}

/// Three mistakes, three sentences. **A negative is the interesting one**,
/// because by the time the checker sees it the lexer has turned `-3` into
/// `0 - 3`, so the shape has to be recognized or the reader gets the message
/// about per-row offsets instead of the one about the other word.
#[test]
fn how_far_refuses_zero_and_negatives_by_name() {
    let conn = diary();
    let zero = refusal(&conn, "diary", "diary then sort [on_] then add [v] as previous([x], 0)");
    assert!(zero.contains("is the column itself"), "{zero}");

    let back = refusal(&conn, "diary", "diary then sort [on_] then add [v] as previous([x], -3)");
    assert!(back.contains("use `following`"), "{back}");

    let forward = refusal(&conn, "diary", "diary then sort [on_] then add [v] as following([x], -3)");
    assert!(forward.contains("use `previous`"), "{forward}");
}

/// The same rule `row_number` has, and for the same reason: a total *so far*
/// means nothing until something has said so far in what order. `rank` is the
/// one window exempt, because it carries its own key.
#[test]
fn the_windows_that_are_not_told_an_order_require_a_sort() {
    let conn = diary();
    for word in ["running_total([x])", "previous([x])", "following([x])"] {
        let message = refusal(&conn, "diary", &format!("diary then add [v] as {word}"));
        assert!(
            message.contains("nothing has said what that order is"),
            "`{word}` was allowed without a sort: {message}"
        );
    }
    // And `rank` is not caught by it, because its argument is the order.
    assert!(run(&conn, "diary", "diary then add [place] as rank([x] descending)").1.len() == 3);
}

// -- what a task list found that four API sweeps could not ------------------

/// **`latest` exists because the workaround this project shipped was wrong.**
/// §14 refused a window in `fill_missing`'s seat and told people to write
/// `first_present([x], previous([x]))` instead, calling it the same thing. It
/// is not: `previous` looks back exactly one row, so a run of two holes left
/// the second one open and nothing said so. This test is the difference.
#[test]
fn latest_fills_a_run_of_holes_where_previous_fills_one() {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE gaps (d BIGINT, x BIGINT);
         INSERT INTO gaps VALUES (1, 10), (2, NULL), (3, NULL), (4, 40), (5, NULL);",
    )
    .expect("the fixture table");

    let (_, carried) = run(&conn, "gaps", "gaps then sort [d] then add [v] as latest([x]) then pick [v]");
    assert_eq!(
        carried,
        vec![vec!["10"], vec!["10"], vec!["10"], vec!["40"], vec!["40"]]
    );

    // The old advice, on the same rows, so the gap it left is on the record.
    let (_, one_back) = run(
        &conn,
        "gaps",
        "gaps then sort [d] then add [v] as first_present([x], previous([x])) then pick [v]",
    );
    assert_eq!(one_back[2], ["missing"], "this is what `latest` was built to fix");
}

/// A row with nothing above it and nothing of its own stays missing, because
/// there is no earlier value to carry. `fill_missing` is what puts one there.
#[test]
fn latest_leaves_a_leading_hole_alone() {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE gaps (d BIGINT, x BIGINT);
         INSERT INTO gaps VALUES (1, NULL), (2, 30);",
    )
    .expect("the fixture table");
    let (_, rows) = run(&conn, "gaps", "gaps then sort [d] then add [v] as latest([x]) then pick [v]");
    assert_eq!(rows, vec![vec!["missing"], vec!["30"]]);
}

#[test]
fn latest_needs_a_sort_like_every_other_window() {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch("CREATE TABLE gaps (d BIGINT, x BIGINT);").expect("the fixture");
    let message = refusal(&conn, "gaps", "gaps then add [v] as latest([x])");
    assert!(message.contains("nothing has said what that order is"), "{message}");
}

/// **The sign is the grammar's rather than the engine's**, which is the second
/// time a function has needed that ruling: R, Python, pandas and polars all
/// answer 1 for `-7 % 2`, and DuckDB and Spark both answer -1 with nothing
/// raised. `weekday` was the first.
#[test]
fn remainder_takes_the_divisors_sign_whatever_the_engine_would_do() {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE nums (n BIGINT);
         INSERT INTO nums VALUES (7), (-7), (6), (0);",
    )
    .expect("the fixture table");
    let (_, rows) = run(&conn, "nums", "nums then add [r] as remainder([n], 2) then pick [r]");
    assert_eq!(rows, vec![vec!["1"], vec!["1"], vec!["0"], vec!["0"]]);
}

/// Integer division is why `remainder` is the only arithmetic word the grammar
/// gained: once it exists there is a composition for the other one, and Law 5
/// refuses a second way to say something already sayable.
#[test]
fn whole_division_composes_out_of_remainder() {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE nums (n BIGINT);
         INSERT INTO nums VALUES (7), (-7);",
    )
    .expect("the fixture table");
    let (_, rows) = run(
        &conn,
        "nums",
        "nums then add [d] as ([n] - remainder([n], 2)) / 2 then pick [d]",
    );
    // R's own answers: 7 %/% 2 is 3 and -7 %/% 2 is -4. They come back as
    // doubles because `/` divides rather than divides-and-floors, which is the
    // whole reason the composition needs `remainder` in front of it.
    assert_eq!(rows, vec![vec!["3.0"], vec!["-4.0"]]);
}

#[test]
fn remainder_wants_numbers_on_both_sides() {
    let conn = pupils();
    let message = refusal(&conn, "pupils", "pupils then add [r] as remainder([name], 2)");
    assert!(message.contains("`remainder` divides one number"), "{message}");
}
