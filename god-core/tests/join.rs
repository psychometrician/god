//! `join`, end to end, over two real tables.
//!
//! **Every assertion here reads rows.** A join is the verb with the most places
//! to be subtly wrong, and every one of them produces a query that runs: a
//! dropped key, a lost row, a column that arrives twice. Asserting on the SQL
//! would agree with whatever the backend wrote, including on the day it started
//! writing something else.
//!
//! The fixture is deliberately lopsided. `sales` has an id that `products` does
//! not, and `products` has one that `sales` does not, so left, inner and full
//! all give different answers and a test cannot pass by accident.

use duckdb::Connection;
use god_core::check::Tables;
use god_core::{compile_tables, Schema, Type};

fn tables() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE sales (id BIGINT, revenue DOUBLE);
         INSERT INTO sales VALUES (1, 100), (2, 200), (3, 300), (9, 50);
         CREATE TABLE products (id BIGINT, name VARCHAR);
         INSERT INTO products VALUES (1, 'Widget'), (2, 'Gadget'), (3, 'Doohickey'), (4, 'Gizmo');
         CREATE TABLE more_sales (id BIGINT, revenue DOUBLE);
         INSERT INTO more_sales VALUES (11, 10), (12, 20);
         CREATE TABLE catalog (product_id BIGINT, label VARCHAR);
         INSERT INTO catalog VALUES (1, 'One'), (2, 'Two'), (3, 'Three'), (4, 'Four');",
    )
    .expect("the fixture tables");
    conn
}

fn schema_of(conn: &Connection, table: &str) -> Schema {
    let mut stmt = conn.prepare(&format!("DESCRIBE {table}")).expect("describe");
    let columns = stmt
        .query_map([], |row| {
            let name: String = row.get(0)?;
            let kind: String = row.get(1)?;
            Ok((name, kind))
        })
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
        .into_iter()
        .map(|(name, kind)| {
            let kind = match kind.split('(').next().unwrap_or("").trim() {
                "BIGINT" | "DOUBLE" | "INTEGER" | "DECIMAL" => Type::Number,
                "VARCHAR" => Type::Text,
                "BOOLEAN" => Type::Truth,
                "DATE" | "TIMESTAMP" => Type::Date,
                _ => Type::Unknown,
            };
            (name, kind)
        })
        .collect();
    Schema { columns }
}

/// Compile against both tables and read back what the engine returns.
fn run(conn: &Connection, pipeline: &str) -> (Vec<String>, Vec<Vec<String>>) {
    let left = schema_of(conn, "sales");
    let others = Tables::new([
        ("products", schema_of(conn, "products")),
        ("more_sales", schema_of(conn, "more_sales")),
        ("catalog", schema_of(conn, "catalog")),
    ]);
    let compiled = match compile_tables(pipeline, &left, &others, "sql") {
        Ok(c) => c,
        Err(d) => panic!("\n{}\n", d.render(pipeline)),
    };

    let mut stmt = conn
        .prepare(&compiled.text)
        .unwrap_or_else(|e| panic!("the query would not prepare: {e}\n\n{}\n", compiled.text));
    let width = compiled.schema.columns.len();
    let rows = stmt
        .query_map([], |row| {
            (0..width)
                .map(|i| {
                    Ok(row
                        .get::<_, Option<String>>(i)
                        .or_else(|_| row.get::<_, Option<i64>>(i).map(|v| v.map(|n| n.to_string())))
                        .or_else(|_| row.get::<_, Option<f64>>(i).map(|v| v.map(|n| format!("{n}"))))
                        .unwrap_or(None)
                        .unwrap_or_else(|| "missing".into()))
                })
                .collect::<Result<Vec<String>, duckdb::Error>>()
        })
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");

    (compiled.schema.names(), rows)
}

fn refusal(pipeline: &str) -> String {
    let conn = tables();
    let left = schema_of(&conn, "sales");
    let others = Tables::new([
        ("products", schema_of(&conn, "products")),
        ("more_sales", schema_of(&conn, "more_sales")),
        ("catalog", schema_of(&conn, "catalog")),
    ]);
    match compile_tables(pipeline, &left, &others, "sql") {
        Ok(_) => panic!("this was accepted, and should not have been:\n{pipeline}"),
        Err(d) => d.message,
    }
}

// -- the three kinds -------------------------------------------------------

#[test]
fn the_default_keeps_every_row_of_this_table() {
    let conn = tables();
    let (names, rows) = run(&conn, "sales then join products by [id] then sort [id]");
    assert_eq!(names, ["id", "revenue", "name"]);
    // id 9 is in sales and not in products, and it survives with no name.
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[3], ["9", "50", "missing"]);
}

#[test]
fn none_keeps_only_the_rows_that_matched() {
    let conn = tables();
    let (_, rows) = run(
        &conn,
        r#"sales then join products by [id] unmatched "none" then sort [id]"#,
    );
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|r| r[2] != "missing"));
}

#[test]
fn both_keeps_the_other_tables_unmatched_rows_too() {
    let conn = tables();
    let (_, rows) = run(
        &conn,
        r#"sales then join products by [id] unmatched "both" then sort [id]"#,
    );
    assert_eq!(rows.len(), 5);
}

#[test]
fn a_full_join_never_loses_the_key_it_matched_on() {
    // **The defect this test exists for.** Taking the key from this table's side
    // is right for every row this table has, and a full join has rows it does
    // not: `Gizmo` is only in `products`, so `sales.id` is empty for it while
    // the id is sitting in `products` untouched. The first version of the
    // backend wrote exactly that query, ran, and returned a row whose key was
    // missing. The column that says which rows correspond is the one that must
    // never be empty in the answer.
    let conn = tables();
    let (_, rows) = run(
        &conn,
        r#"sales then join products by [id] unmatched "both" then sort [id]"#,
    );
    let gizmo = rows.iter().find(|r| r[2] == "Gizmo").expect("the row only products has");
    assert_eq!(gizmo[0], "4", "the key came back empty on a full join");
}

// -- what it works out, and what it refuses --------------------------------

#[test]
fn the_key_is_worked_out_from_the_shared_names_and_said_out_loud() {
    let conn = tables();
    let left = schema_of(&conn, "sales");
    let others = Tables::new([("products", schema_of(&conn, "products"))]);
    let compiled = compile_tables("sales then join products", &left, &others, "sql")
        .expect("a join with no `by` is legal");

    assert_eq!(compiled.assumptions.len(), 1, "the choice has to be reported");
    assert!(
        compiled.assumptions[0].message.contains("id"),
        "the assumption names the column it matched on: {}",
        compiled.assumptions[0].message
    );
}

#[test]
fn the_worked_out_key_is_written_back_into_the_pipeline() {
    // A backend is handed a plan and never sees the other table, so it could not
    // work the key out a second time. The checker settles it and puts it back,
    // which is also what lets someone read what was assumed.
    let conn = tables();
    let left = schema_of(&conn, "sales");
    let others = Tables::new([("products", schema_of(&conn, "products"))]);
    let printed = compile_tables("sales then join products", &left, &others, "god")
        .expect("legal")
        .text;
    assert!(
        printed.contains("join products by [id]"),
        "the printed pipeline should show the key that was chosen: {printed}"
    );
}

#[test]
fn tables_that_share_no_column_name_are_refused_with_the_fix() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE sales (a BIGINT); INSERT INTO sales VALUES (1);
         CREATE TABLE products (b BIGINT); INSERT INTO products VALUES (1);",
    )
    .unwrap();
    let left = schema_of(&conn, "sales");
    let others = Tables::new([("products", schema_of(&conn, "products"))]);
    let message = match compile_tables("sales then join products", &left, &others, "sql") {
        Ok(_) => panic!("no shared name, so this should not have been accepted"),
        Err(d) => d.message,
    };
    assert!(message.contains("join products by [id]"), "{message}");
}

#[test]
fn a_key_the_other_table_does_not_have_names_that_table() {
    let message = refusal("sales then join products by [revenue]");
    assert_eq!(
        message,
        "`products` has no column called `revenue`. If `products` calls it something else, say both: `by [revenue] is [their_name]`. It has: id, name"
    );
}

/// **The nudge above is for the caller who wrote one name for both sides**, and
/// it would be noise for the caller who already wrote two: they plainly know
/// the two halves can differ, so telling them costs a line and says nothing.
#[test]
fn a_key_written_as_a_pair_is_not_told_about_pairs() {
    let message = refusal("sales then join products by [revenue] is [price]");
    assert_eq!(
        message,
        "`products` has no column called `price`. It has: id, name"
    );
}

#[test]
fn a_key_of_a_different_kind_in_each_table_is_refused() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE sales (id VARCHAR); INSERT INTO sales VALUES ('1');
         CREATE TABLE products (id BIGINT); INSERT INTO products VALUES (1);",
    )
    .unwrap();
    let left = schema_of(&conn, "sales");
    let others = Tables::new([("products", schema_of(&conn, "products"))]);
    let message = match compile_tables("sales then join products by [id]", &left, &others, "sql") {
        Ok(_) => panic!("text against a number can never match"),
        Err(d) => d.message,
    };
    assert_eq!(
        message,
        "`[id]` is text here and a number in `products`, so the two can never match. Convert one of them first"
    );
}

#[test]
fn a_column_on_both_tables_that_is_not_a_key_is_refused() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE sales (id BIGINT, name VARCHAR); INSERT INTO sales VALUES (1, 'a');
         CREATE TABLE products (id BIGINT, name VARCHAR); INSERT INTO products VALUES (1, 'b');",
    )
    .unwrap();
    let left = schema_of(&conn, "sales");
    let others = Tables::new([("products", schema_of(&conn, "products"))]);
    let message = match compile_tables("sales then join products by [id]", &left, &others, "sql") {
        Ok(_) => panic!("`name` would arrive twice"),
        Err(d) => d.message,
    };
    assert!(message.contains("both tables have `name`"), "{message}");
    assert!(message.contains("pick all_but"), "the message says what to do: {message}");
}

#[test]
fn a_table_that_was_never_described_says_so() {
    let conn = tables();
    let left = schema_of(&conn, "sales");
    let message = match compile_tables(
        "sales then join suppliers by [id]",
        &left,
        &Tables::empty(),
        "sql",
    ) {
        Ok(_) => panic!("there is nothing to join to"),
        Err(d) => d.message,
    };
    assert!(message.contains("suppliers"), "{message}");
}

#[test]
fn the_join_assertions_can_fail() {
    // The guard on the guards: if `run` could not surface a wrong answer, none
    // of the above is evidence.
    let conn = tables();
    let (_, rows) = run(&conn, "sales then join products by [id]");
    assert_ne!(rows.len(), 99, "sanity");
    assert!(std::panic::catch_unwind(|| {
        let conn = tables();
        let (_, rows) = run(&conn, "sales then join products by [id]");
        assert_eq!(rows.len(), 99);
    })
    .is_err());
}

// -- the rest of M7's verbs, which all reach for a second table or a schema --

#[test]
fn add_rows_needs_both_tables_to_have_the_same_columns() {
    // dplyr's `bind_rows` fills the difference with NA, which is convenient
    // exactly until the day the two tables differ because one of them is wrong,
    // and then it hands back a half-empty column and says nothing.
    let message = refusal("sales then add_rows products");
    assert!(message.contains("the same columns"), "{message}");
    assert!(message.contains("`pick`"), "the message says what to do: {message}");
}

#[test]
fn add_rows_stacks_the_rows_and_keeps_the_repeats() {
    let conn = tables();
    let (_, before) = run(&conn, "sales then take 100");
    let (_, after) = run(&conn, "sales then add_rows more_sales");
    assert_eq!(after.len(), before.len() + 2, "every row of both tables");
}

#[test]
fn add_rows_can_name_the_table_at_the_head() {
    // Doubling a table is a legitimate thing to say, and the head table is a
    // described table: refusing `sales then add_rows sales` with "no other
    // table was described" told the caller to do the thing they had just done.
    let conn = tables();
    let (_, before) = run(&conn, "sales then take 100");
    let (_, after) = run(&conn, "sales then add_rows sales");
    assert_eq!(after.len(), before.len() * 2, "every row, twice");
}

#[test]
fn renaming_onto_a_name_the_table_already_has_is_refused() {
    let message = refusal("sales then rename [revenue] as [id]");
    assert!(message.contains("already has a column called `revenue`"), "{message}");
    // The message has to say which side the new name goes on, because both
    // spellings parse and only one is meant.
    assert!(message.contains("rename [new] as [id]"), "{message}");
}

#[test]
fn rename_keeps_the_column_where_it_was() {
    // `SELECT * EXCLUDE` plus the new name would move every renamed column to
    // the end, which is a change nobody asked for.
    let conn = tables();
    let (names, _) = run(&conn, "sales then rename [amount] as [revenue]");
    assert_eq!(names, ["id", "amount"]);
}

#[test]
fn rename_will_not_take_a_value_and_says_which_verb_does() {
    let message = refusal("sales then rename [doubled] as [revenue] * 2");
    assert!(message.contains("use `add`"), "{message}");
}

#[test]
fn filling_a_number_column_with_text_is_refused() {
    let message = refusal(r#"sales then fill_missing [revenue] as "none""#);
    assert!(message.contains("would change what the column holds"), "{message}");
}

#[test]
fn dropping_duplicates_hands_rows_back_in_a_settled_order() {
    // The same irregularity `summarize` has: DISTINCT promises nothing about
    // order, so two runs of one engine can disagree. Ordering by the columns
    // that define the groups is the same answer, not a second one.
    let conn = tables();
    let (_, rows) = run(&conn, "sales then drop_duplicates");
    let ids: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_by_key(|s| s.parse::<i64>().unwrap_or(0));
    assert_eq!(ids, sorted, "drop_duplicates should settle the order");
}

#[test]
fn a_grouped_take_needs_a_sort_and_says_so() {
    // "The first row of each group" means nothing until something says first by
    // what, and the answer would look entirely reasonable while being arbitrary.
    let message = refusal("sales then take 1 by [id]");
    assert!(message.contains("there is no first"), "{message}");
    assert!(message.contains("sort"), "the message says what to do: {message}");
}

#[test]
fn a_grouped_take_keeps_one_row_of_each_group() {
    let conn = tables();
    let (names, rows) = run(&conn, "sales then sort [id] then take 1 by [id]");
    assert_eq!(names, ["id", "revenue"], "the row number never survives the step");
    assert_eq!(rows.len(), 4, "sales has four distinct ids");
}

#[test]
fn a_grouped_take_takes_the_row_the_sort_put_first() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE sales (id BIGINT, revenue DOUBLE);
         INSERT INTO sales VALUES (1, 10), (1, 99), (2, 20), (2, 77);
         CREATE TABLE products (id BIGINT, name VARCHAR);
         CREATE TABLE more_sales (id BIGINT, revenue DOUBLE);
         CREATE TABLE catalog (product_id BIGINT, label VARCHAR);",
    )
    .unwrap();
    let (_, rows) = run(&conn, "sales then sort [id], [revenue] descending then take 1 by [id]");
    let revenues: Vec<&str> = rows.iter().map(|r| r[1].as_str()).collect();
    assert_eq!(revenues, ["99", "77"], "the sort decides which row survives");
}

#[test]
fn a_summarize_between_the_sort_and_the_take_unsettles_the_order() {
    // summarize imposes its own order, so whatever the sort established is gone
    // and the take has nothing to mean again.
    let message = refusal(
        "sales then sort [revenue] then summarize [n] as row_count() by [id] then take 1 by [id]",
    );
    assert!(message.contains("there is no first"), "{message}");
}

#[test]
fn naming_columns_on_drop_duplicates_teaches_both_alternatives() {
    let message = refusal("sales then drop_duplicates [id]");
    assert!(message.contains("pick [id] then drop_duplicates"), "{message}");
    assert!(message.contains("take 1 by [id]"), "{message}");
}

// -- matching, the filtering join ------------------------------------------
//
// A semi join and an anti join add no columns, so they are conditions rather
// than joins and are spelled `keep where matching(...)`. These read rows for the
// same reason the tests above do: the query looks reasonable either way.

#[test]
fn matching_keeps_the_rows_that_have_a_partner() {
    let conn = tables();
    let (names, rows) = run(
        &conn,
        "sales then keep where matching(products, by [id]) then sort [id]",
    );
    // A filtering join adds no columns, which is the whole reason it is not
    // spelled `join`.
    assert_eq!(names, ["id", "revenue"]);
    let ids: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
    assert_eq!(ids, ["1", "2", "3"], "id 9 has no partner in products");
}

#[test]
fn not_matching_keeps_exactly_the_rows_matching_drops() {
    let conn = tables();
    let (names, rows) = run(
        &conn,
        "sales then keep where not matching(products, by [id]) then sort [id]",
    );
    assert_eq!(names, ["id", "revenue"]);
    let ids: Vec<&str> = rows.iter().map(|r| r[0].as_str()).collect();
    assert_eq!(ids, ["9"]);
}

#[test]
fn a_duplicate_key_on_the_other_side_cannot_multiply_rows() {
    // **The guarantee `join` could not make.** A row either has a partner or it
    // does not, so how many it has never reaches the answer. The same two tables
    // through `join` return four rows; through `matching` they return two, and
    // that difference is the reason to reach for one over the other.
    let conn = tables();
    conn.execute_batch(
        "CREATE TABLE restocks (id BIGINT, quantity BIGINT);
         INSERT INTO restocks VALUES (1, 10), (1, 20), (1, 30), (2, 40);",
    )
    .expect("a table with a repeated key");

    let left = schema_of(&conn, "sales");
    let others = Tables::new([
        ("products", schema_of(&conn, "products")),
        ("more_sales", schema_of(&conn, "more_sales")),
        ("restocks", schema_of(&conn, "restocks")),
    ]);

    let count = |pipeline: &str| {
        let compiled = match compile_tables(pipeline, &left, &others, "sql") {
            Ok(c) => c,
            Err(d) => panic!("\n{}\n", d.render(pipeline)),
        };
        let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
        stmt.query_map([], |_| Ok(()))
            .expect("rows")
            .count()
    };

    assert_eq!(count("sales then keep where matching(restocks, by [id])"), 2);
    assert_eq!(
        count("sales then join restocks by [id] unmatched \"none\""),
        4,
        "the join multiplies where the filter cannot"
    );
}

#[test]
fn matching_works_out_the_key_from_the_shared_names() {
    let conn = tables();
    let (_, rows) = run(&conn, "sales then keep where matching(products)");
    assert_eq!(rows.len(), 3, "id is the only name both tables carry");
}

#[test]
fn matching_cannot_be_one_half_of_a_bigger_question() {
    // A filtering join is a whole verb in every host underneath, so combining it
    // with `and` would mean one backend rendering a structure the others cannot.
    let message = refusal("sales then keep where matching(products, by [id]) and [revenue] > 100");
    assert!(message.contains("its own step"), "{message}");
    assert!(message.contains("matching(products, by [id])"), "{message}");
}

#[test]
fn matching_a_table_nobody_described_names_the_ones_that_were() {
    let message = refusal("sales then keep where matching(produce, by [id])");
    assert!(message.contains("products"), "{message}");
    assert!(message.contains("Did you mean"), "{message}");
}

#[test]
fn a_filtering_join_reports_the_table_it_reads() {
    // `--needs` is how a launcher knows what to hand over, and a table named
    // inside a condition is named by no step at all.
    let plan = god_core::parse::parse("sales then keep where matching(products, by [id])")
        .expect("this parses");
    assert_eq!(plan.tables(), ["sales", "products"]);
}

// -- the rank family -------------------------------------------------------
//
// **Read as rows, because the tie is the whole question.** A query using `RANK`
// and a query using `DENSE_RANK` look equally reasonable and differ only in what
// comes back, which is exactly the shape this file exists to catch.

#[test]
fn rank_lets_ties_share_a_place_and_skips_the_next() {
    let conn = tables();
    conn.execute_batch(
        "CREATE TABLE races (name VARCHAR, score BIGINT);
         INSERT INTO races VALUES ('a', 10), ('b', 20), ('c', 20), ('d', 30);",
    )
    .expect("a table with a tie in it");

    let left = schema_of(&conn, "races");
    let others = Tables::empty();
    let pipeline =
        "races then add [place] as rank([score]) then sort [name]";
    let compiled = match compile_tables(pipeline, &left, &others, "sql") {
        Ok(c) => c,
        Err(d) => panic!("\n{}\n", d.render(pipeline)),
    };
    let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
    let places: Vec<i64> = stmt
        .query_map([], |row| row.get::<_, i64>(2))
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");

    // 10 is first, the two 20s share second, and 30 is fourth. Not fourth is
    // what `dense_rank` would give, and it is the reason `rank` is the word.
    assert_eq!(places, [1, 2, 2, 4]);
}

#[test]
fn rank_reads_descending_the_way_sort_does() {
    let conn = tables();
    conn.execute_batch(
        "CREATE TABLE races (name VARCHAR, score BIGINT);
         INSERT INTO races VALUES ('a', 10), ('b', 20), ('c', 30);",
    )
    .expect("the fixture");

    let left = schema_of(&conn, "races");
    let pipeline =
        "races then add [place] as rank([score] descending) then sort [name]";
    let compiled = compile_tables(pipeline, &left, &Tables::empty(), "sql")
        .unwrap_or_else(|d| panic!("\n{}\n", d.render(pipeline)));
    let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
    let places: Vec<i64> = stmt
        .query_map([], |row| row.get::<_, i64>(2))
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    assert_eq!(places, [3, 2, 1], "the largest score is in first place");
}

#[test]
fn rank_by_a_group_restarts_at_one_for_each() {
    let conn = tables();
    conn.execute_batch(
        "CREATE TABLE races (heat VARCHAR, name VARCHAR, score BIGINT);
         INSERT INTO races VALUES
           ('x', 'a', 10), ('x', 'b', 20), ('y', 'c', 5), ('y', 'd', 50);",
    )
    .expect("the fixture");

    let left = schema_of(&conn, "races");
    let pipeline = "races then add [place] as rank([score] descending) by [heat] then sort [name]";
    let compiled = compile_tables(pipeline, &left, &Tables::empty(), "sql")
        .unwrap_or_else(|d| panic!("\n{}\n", d.render(pipeline)));
    let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
    let places: Vec<i64> = stmt
        .query_map([], |row| row.get::<_, i64>(3))
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    // a and b are heat x; c and d are heat y. Each heat has a first place.
    assert_eq!(places, [2, 1, 2, 1]);
}

#[test]
fn row_number_never_ties_where_rank_does() {
    let conn = tables();
    conn.execute_batch(
        "CREATE TABLE races (name VARCHAR, score BIGINT);
         INSERT INTO races VALUES ('a', 10), ('b', 20), ('c', 20), ('d', 30);",
    )
    .expect("a table with a tie in it");

    let left = schema_of(&conn, "races");
    let pipeline =
        "races then sort [score] then add [n] as row_number() then sort [n]";
    let compiled = compile_tables(pipeline, &left, &Tables::empty(), "sql")
        .unwrap_or_else(|d| panic!("\n{}\n", d.render(pipeline)));
    let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
    let numbers: Vec<i64> = stmt
        .query_map([], |row| row.get::<_, i64>(2))
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    // The same four rows `rank` numbered 1, 2, 2, 4.
    assert_eq!(numbers, [1, 2, 3, 4]);
}

#[test]
fn row_number_without_a_sort_says_what_to_write() {
    let message = refusal("sales then add [n] as row_number()");
    assert!(message.contains("nothing has said what that order is"), "{message}");
    assert!(message.contains("rank("), "it offers the alternative: {message}");
}

#[test]
fn a_window_cannot_choose_the_rows_it_is_computed_over() {
    let message = refusal("sales then keep where rank([revenue]) <= 3");
    assert!(message.contains("cannot be what chooses them"), "{message}");
    assert!(message.contains("take 3 by"), "it names the one-step form: {message}");
}

#[test]
fn a_window_in_a_summarize_is_refused_in_its_own_words() {
    let message = refusal("sales then summarize [p] as rank([revenue]) by [id]");
    assert!(message.contains("nowhere to go"), "{message}");
}

// -- first_present ---------------------------------------------------------

#[test]
fn first_present_reads_left_to_right_and_stops_at_the_first_value() {
    let conn = tables();
    conn.execute_batch(
        "CREATE TABLE reach (id BIGINT, mobile VARCHAR, landline VARCHAR, email VARCHAR);
         INSERT INTO reach VALUES
           (1, '555-01', '555-99', 'a@x'),
           (2, NULL,     '555-02', 'b@x'),
           (3, NULL,     NULL,     'c@x'),
           (4, NULL,     NULL,     NULL);",
    )
    .expect("the fixture");

    let left = schema_of(&conn, "reach");
    let read = |pipeline: &str| {
        let compiled = compile_tables(pipeline, &left, &Tables::empty(), "sql")
            .unwrap_or_else(|d| panic!("\n{}\n", d.render(pipeline)));
        let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
        stmt.query_map([], |row| Ok(row.get::<_, Option<String>>(4)?))
            .expect("rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows")
    };

    assert_eq!(
        read("reach then add [got] as first_present([mobile], [landline], [email]) then sort [id]"),
        [
            Some("555-01".to_string()),
            Some("555-02".to_string()),
            Some("c@x".to_string()),
            // Every column missing, so the answer is missing too.
            None,
        ]
    );

    // **The arguments are a priority order, not a set.** The same three columns
    // in another order give another answer, which is the whole reason they are
    // written in an order.
    assert_eq!(
        read("reach then add [got] as first_present([email], [mobile], [landline]) then sort [id]"),
        [
            Some("a@x".to_string()),
            Some("b@x".to_string()),
            Some("c@x".to_string()),
            None,
        ]
    );
}

#[test]
fn only_a_missing_value_is_skipped() {
    // A zero is a reading. This is the case people expect to fall through and it
    // does not, which is why the word is `present` rather than `valid`.
    let conn = tables();
    conn.execute_batch(
        "CREATE TABLE readings (id BIGINT, sensor BIGINT, backup BIGINT);
         INSERT INTO readings VALUES (1, 0, 99), (2, NULL, 99);",
    )
    .expect("the fixture");

    let left = schema_of(&conn, "readings");
    let pipeline = "readings then add [used] as first_present([sensor], [backup]) then sort [id]";
    let compiled = compile_tables(pipeline, &left, &Tables::empty(), "sql")
        .unwrap_or_else(|d| panic!("\n{}\n", d.render(pipeline)));
    let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
    let used: Vec<i64> = stmt
        .query_map([], |row| row.get::<_, i64>(3))
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    assert_eq!(used, [0, 99]);
}

#[test]
fn first_present_needs_somewhere_to_choose_between() {
    let message = refusal("sales then add [x] as first_present([revenue])");
    assert!(message.contains("at least 2 columns"), "{message}");
}

#[test]
fn first_present_will_not_mix_what_a_column_holds() {
    // One of the columns is going to be the answer, and a column holds one kind
    // of thing, so they all have to agree. The fixture's own `sales` is numbers
    // throughout, so this needs a table with both.
    let conn = tables();
    let left = schema_of(&conn, "products");
    let pipeline = "products then add [x] as first_present([id], [name])";
    let message = match compile_tables(pipeline, &left, &Tables::empty(), "sql") {
        Ok(_) => panic!("this was accepted, and should not have been:\n{pipeline}"),
        Err(d) => d.message,
    };
    assert!(message.contains("same kind of thing"), "{message}");
}

// -- keys the two tables name differently ----------------------------------
//
// **The everyday case, and god could not say it until 2026-08-16.** A schema
// names its primary key `id` and its foreign key `<thing>_id`, so the mismatch
// is the ordinary shape of a join rather than an edge of one. `catalog` is in
// the fixture for exactly this: it holds `product_id` where `sales` holds `id`.

#[test]
fn a_pair_joins_columns_the_two_tables_name_differently() {
    let conn = tables();
    let (names, rows) = run(
        &conn,
        "sales then join catalog by [id] is [product_id] then sort [id]",
    );
    // **This table's name survives and the other's is dropped**, which is what
    // makes the rest of the sentence readable: the pipeline is still about
    // `sales`, so it goes on saying `id`.
    assert_eq!(names, ["id", "revenue", "label"]);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0], ["1", "100", "One"]);
    assert_eq!(rows[3], ["9", "50", "missing"]);
}

#[test]
fn a_pair_matches_rows_rather_than_merely_running() {
    // The query would run and answer nothing if the two halves were read in the
    // wrong order, so this asserts the values that came across rather than the
    // shape of the result.
    let conn = tables();
    let (_, rows) = run(
        &conn,
        r#"sales then join catalog by [id] is [product_id] unmatched "none" then sort [id]"#,
    );
    let labels: Vec<&str> = rows.iter().map(|r| r[2].as_str()).collect();
    assert_eq!(labels, ["One", "Two", "Three"]);
}

#[test]
fn a_full_join_on_a_pair_never_loses_the_key() {
    // The same defect `a_full_join_never_loses_the_key_it_matched_on` guards,
    // one step harder: the value has to be carried across from a column with a
    // *different name*. Catalog's row 4 is in neither `sales` nor `products`.
    let conn = tables();
    let (names, rows) = run(
        &conn,
        r#"sales then join catalog by [id] is [product_id] unmatched "both" then sort [id]"#,
    );
    assert_eq!(names, ["id", "revenue", "label"]);
    let four = rows.iter().find(|r| r[2] == "Four").expect("the row only catalog has");
    assert_eq!(four[0], "4", "the key came back empty on a full join over a pair");
}

#[test]
fn same_named_and_differently_named_keys_mix_in_one_join() {
    // Two tables that agree on `region` and disagree on what the customer key
    // is called, which is the ordinary shape of a warehouse schema and the one
    // case that exercises both halves of the parser in one clause.
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE orders (region VARCHAR, customer_id BIGINT, amount DOUBLE);
         INSERT INTO orders VALUES ('W', 1, 10), ('E', 2, 20), ('W', 9, 30);
         CREATE TABLE customers (region VARCHAR, id BIGINT, name VARCHAR);
         INSERT INTO customers VALUES ('W', 1, 'a'), ('E', 2, 'b'), ('N', 7, 'c');",
    )
    .unwrap();
    let left = schema_of(&conn, "orders");
    let others = Tables::new([("customers", schema_of(&conn, "customers"))]);
    let pipeline = "orders then join customers by [region], [customer_id] is [id]";

    let printed = compile_tables(pipeline, &left, &others, "god").expect("legal").text;
    assert!(printed.contains("by [region], [customer_id] is [id]"), "{printed}");

    // And it answers, rather than merely compiling. `region` matching by name
    // and `customer_id` matching by pair have to happen in the same `ON`.
    let compiled = compile_tables(pipeline, &left, &others, "sql").expect("legal");
    assert_eq!(compiled.schema.names(), ["region", "customer_id", "amount", "name"]);
    let mut stmt = conn.prepare(&compiled.text).expect("the query prepares");
    let names: Vec<Option<String>> = stmt
        .query_map([], |row| row.get::<_, Option<String>>(3))
        .expect("rows")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows");
    assert_eq!(names.iter().filter(|n| n.is_some()).count(), 2);
}

#[test]
fn a_run_of_shared_names_round_trips_as_one_bracket_group() {
    // `by [a, b]` and `by [a], [b]` mean the same thing and only one of them is
    // what the caller typed. The god backend hands back the sentence, so it has
    // to hand back that one.
    let conn = tables();
    let left = schema_of(&conn, "sales");
    let others = Tables::new([("more_sales", schema_of(&conn, "more_sales"))]);
    let printed = compile_tables(
        "sales then join more_sales by [id, revenue]",
        &left,
        &others,
        "god",
    )
    .expect("legal")
    .text;
    assert!(printed.contains("by [id, revenue]"), "{printed}");
}

#[test]
fn a_pair_still_refuses_a_key_of_a_different_kind() {
    // Naming the two sides separately does not buy an exemption from the rule
    // that a text id and a number id can never match.
    let message = refusal("sales then join catalog by [id] is [label]");
    assert!(message.contains("can never match"), "{message}");
    // and it names both columns, because one name would send the reader looking
    // for a column that is not on the table they are reading.
    assert!(message.contains("`[id]` against `[label]`"), "{message}");
}

#[test]
fn the_other_tables_key_is_not_counted_as_a_clash() {
    // `catalog.product_id` is not on `sales`, so nothing here is a clash; the
    // guard is against the opposite mistake, where the exemption is written too
    // wide and a genuine collision gets through. `label` is the non-key column
    // and must still be free.
    let conn = tables();
    let (names, _) = run(&conn, "sales then join catalog by [id] is [product_id]");
    assert!(names.contains(&"label".to_string()), "{names:?}");
    assert!(!names.contains(&"product_id".to_string()), "{names:?}");
}

#[test]
fn a_filtering_join_takes_a_pair_too() {
    // The spec's own argument for `matching` is that it works out its key by the
    // same code `join` does, so a pipeline cannot get one answer from a join and
    // another from a filter over the same two tables. That has to hold for the
    // pair form as well, or the grammar has an exception in it.
    let conn = tables();
    let (_, rows) = run(
        &conn,
        "sales then keep where matching(catalog, by [id] is [product_id]) then sort [id]",
    );
    assert_eq!(rows.len(), 3, "id 9 has no partner in catalog");
    let (_, missing) = run(
        &conn,
        "sales then keep where not matching(catalog, by [id] is [product_id])",
    );
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0][0], "9");
}

#[test]
fn one_is_takes_one_column_on_each_side() {
    let message = refusal("sales then join catalog by [id, revenue] is [product_id]");
    assert!(message.contains("one column against one column"), "{message}");
}
