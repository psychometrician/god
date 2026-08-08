//! A pipeline, end to end, over a real table.
//!
//! **These tests read the rows that came back.** Asserting on the generated SQL
//! would be checking a proxy: the query can be exactly what was expected and the
//! answer still wrong, and a suite built on proxies passes for years while the
//! thing it covers is broken. So every assertion here is about the table — its
//! rows, the order of its columns, and the values in them.
//!
//! The schema comes from the engine rather than from a literal in the test, by
//! asking DuckDB to describe the table. That is the path a real host takes, and
//! writing it out by hand here would test a fiction.

use duckdb::Connection;
use god_core::{compile, Schema, Type};

/// The table every test in this file runs against.
///
/// Small enough to work out by hand, and shaped so the arithmetic is checkable:
/// four West rows across two products, and one East row that the first step has
/// to remove.
fn sales() -> Connection {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE TABLE sales (
             region  VARCHAR,
             product VARCHAR,
             revenue DOUBLE,
             cost    DOUBLE
         );
         INSERT INTO sales VALUES
             ('West', 'Widget', 100, 40),
             ('West', 'Widget', 200, 50),
             ('West', 'Gadget', 300, 100),
             ('West', 'Gadget', 150, 50),
             ('East', 'Widget', 500, 100);",
    )
    .expect("the fixture table");
    conn
}

/// What a query returns, asked of the engine without running it.
///
/// This is the path a host takes to hand the grammar a schema, so the tests use
/// it rather than writing the columns out as a literal. A literal would agree
/// with the checker forever, including on the day the table changed.
fn describe(conn: &Connection, query: &str) -> Vec<(String, Type)> {
    let mut stmt = conn
        .prepare(&format!("DESCRIBE {query}"))
        .unwrap_or_else(|e| panic!("the query would not describe: {e}\n\n{query}\n"));
    stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let kind: String = row.get(1)?;
        Ok((name, kind))
    })
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
            "DATE" | "TIMESTAMP" | "TIMESTAMP_NS" => Type::Date,
            _ => Type::Unknown,
        };
        (name, kind)
    })
    .collect()
}

fn schema_of(conn: &Connection, table: &str) -> Schema {
    Schema::new(describe(conn, &format!("SELECT * FROM {table}")))
}

/// Run a pipeline and hand back the column names and the rows as text, so a test
/// can compare the whole table in one assertion.
fn run(conn: &Connection, pipeline: &str) -> (Vec<String>, Vec<Vec<String>>) {
    run_on(conn, pipeline, "sales")
}

/// The same, for a pipeline whose table is not the fixture. A qualified name is
/// the case that needs it: the schema has to be asked for under the name the
/// sentence used.
fn run_on(
    conn: &Connection,
    pipeline: &str,
    table: &str,
) -> (Vec<String>, Vec<Vec<String>>) {
    let schema = schema_of(conn, table);
    let compiled = match compile(pipeline, &schema, "sql") {
        Ok(c) => c,
        Err(d) => panic!("\n{}\n", d.render(pipeline)),
    };

    // The engine's own account of what the query returns, name and type, without
    // running it. Both are compared against the checker's account below: if the
    // grammar and the engine disagree about the shape of the answer, that is a
    // defect in the grammar and this is where it surfaces.
    let described = describe(conn, &compiled.text);
    let names: Vec<String> = described.iter().map(|(n, _)| n.clone()).collect();
    let count = described.len();

    assert_eq!(
        names,
        compiled.schema.names(),
        "the checker and the engine disagree about the columns"
    );
    let engine_types: Vec<Type> = described.iter().map(|(_, t)| *t).collect();
    let checked_types: Vec<Type> = compiled.schema.columns.iter().map(|(_, t)| *t).collect();
    for ((name, engine), checked) in names.iter().zip(&engine_types).zip(&checked_types) {
        if *checked != Type::Unknown && *engine != Type::Unknown {
            assert_eq!(
                checked, engine,
                "the checker says `{name}` is {} and the engine says it is {}",
                checked.name(),
                engine.name()
            );
        }
    }

    let mut stmt = conn.prepare(&compiled.text).unwrap_or_else(|e| {
        panic!("the query would not prepare: {e}\n\n{}\n", compiled.text)
    });

    let rows = stmt
        .query_map([], |row| {
            (0..count)
                .map(|i| {
                    // Read every column as whatever it is and render it the same
                    // way, so a test compares tables rather than types.
                    Ok(match row.get_ref(i)? {
                        duckdb::types::ValueRef::Null => "missing".to_string(),
                        duckdb::types::ValueRef::Text(t) => {
                            String::from_utf8_lossy(t).to_string()
                        }
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

/// The sentence the whole grammar is built around, run for real.
#[test]
fn the_pipeline_returns_the_table_worked_out_by_hand() {
    let conn = sales();
    let (columns, rows) = run(
        &conn,
        r#"sales
             then keep where [region] is "West"
             then add [margin] as [revenue] - [cost]
             then summarize [margin] as total([margin]), [orders] as row_count() by [product]
             then sort [margin] descending
             then take 10"#,
    );

    // West only: Gadget is (300-100) + (150-50) = 300 over two orders; Widget is
    // (100-40) + (200-50) = 210 over two. The East row is gone, and Gadget sorts
    // first because 300 is larger.
    assert_eq!(columns, vec!["product", "margin", "orders"]);
    assert_eq!(
        rows,
        vec![
            vec!["Gadget".to_string(), "300.0".to_string(), "2".to_string()],
            vec!["Widget".to_string(), "210.0".to_string(), "2".to_string()],
        ]
    );
}

#[test]
fn columns_come_back_in_the_order_they_were_asked_for() {
    let conn = sales();
    let (columns, rows) = run(
        &conn,
        r#"sales then keep where [region] is "East" then pick [cost, product]"#,
    );
    assert_eq!(columns, vec!["cost", "product"]);
    assert_eq!(rows, vec![vec!["100.0".to_string(), "Widget".to_string()]]);
}

#[test]
fn all_but_drops_the_columns_it_names_and_keeps_the_rest() {
    let conn = sales();
    let (columns, _) = run(
        &conn,
        "sales then pick all_but [cost, revenue]",
    );
    assert_eq!(columns, vec!["region", "product"]);
}

#[test]
fn add_replaces_a_column_rather_than_producing_it_twice() {
    let conn = sales();
    let (columns, rows) = run(
        &conn,
        r#"sales
             then keep where [region] is "East"
             then add [cost] as [cost] * 2
             then pick [product, cost]"#,
    );
    assert_eq!(columns, vec!["product", "cost"]);
    assert_eq!(rows, vec![vec!["Widget".to_string(), "200.0".to_string()]]);
}

/// An aggregate written in `add` spans the group and comes back to every row in
/// it, which is what makes a share one idea rather than a second verb.
#[test]
fn an_aggregate_in_add_broadcasts_over_the_group() {
    let conn = sales();
    let (columns, rows) = run(
        &conn,
        r#"sales
             then keep where [region] is "West"
             then add [product_total] as total([revenue]) by [product]
             then pick [product, revenue, product_total]
             then sort [revenue]"#,
    );
    assert_eq!(columns, vec!["product", "revenue", "product_total"]);
    assert_eq!(
        rows,
        vec![
            vec!["Widget".to_string(), "100.0".to_string(), "300.0".to_string()],
            vec!["Gadget".to_string(), "150.0".to_string(), "450.0".to_string()],
            vec!["Widget".to_string(), "200.0".to_string(), "300.0".to_string()],
            vec!["Gadget".to_string(), "300.0".to_string(), "450.0".to_string()],
        ]
    );
}

#[test]
fn a_set_of_choices_matches_any_of_them() {
    let conn = sales();
    let (_, rows) = run(
        &conn,
        r#"sales
             then keep where [product] in {"Gadget"}
             then summarize [n] as row_count() by [region]"#,
    );
    assert_eq!(rows, vec![vec!["West".to_string(), "2".to_string()]]);
}

#[test]
fn summarizing_with_no_groups_returns_one_row() {
    let conn = sales();
    let (columns, rows) = run(
        &conn,
        "sales then summarize [orders] as row_count(), [biggest] as largest([revenue])",
    );
    assert_eq!(columns, vec!["orders", "biggest"]);
    assert_eq!(rows, vec![vec!["5".to_string(), "500.0".to_string()]]);
}

/// Groups come back in a settled order, every time.
///
/// `GROUP BY` promises nothing about row order, so a hash aggregation yields
/// groups in whatever order its table happens to hold — which differs between
/// two runs of the same pipeline, never mind between two hosts. This was found
/// by running one sentence in R and in Python and getting the same rows in a
/// different order, which is the kind of defect that hides for a long time
/// because every individual run looks fine.
#[test]
fn summarizing_returns_its_groups_in_order() {
    let conn = sales();
    let (_, rows) = run(&conn, "sales then summarize [n] as row_count() by [product]");
    assert_eq!(
        rows,
        vec![
            vec!["Gadget".to_string(), "2".to_string()],
            vec!["Widget".to_string(), "3".to_string()],
        ]
    );

    // Several grouping columns order by all of them, left to right.
    let (_, rows) = run(&conn, "sales then summarize [n] as row_count() by [region, product]");
    assert_eq!(
        rows.iter().map(|r| r[..2].join("/")).collect::<Vec<_>>(),
        vec!["East/Widget", "West/Gadget", "West/Widget"]
    );
}

/// The guard has to be able to fail. A pipeline whose expected answer is wrong
/// on purpose proves the assertions above are measuring the rows and not merely
/// running without complaint.
#[test]
#[should_panic(expected = "assertion")]
fn the_row_assertions_can_fail() {
    let conn = sales();
    let (_, rows) = run(&conn, "sales then take 1");
    assert_eq!(rows, vec![vec!["this is not what the table holds".to_string()]]);
}

/// A table named in parts is found, which is what a catalog needs.
///
/// **The mistake this covers reads perfectly and cannot work.** Quoting the
/// whole of `sch.orders` names one table whose name contains a dot, and no
/// catalog has one, so the query parses and then fails looking for something
/// that was never there. Each part is an identifier of its own.
///
/// It runs rather than reading the SQL, for the reason the header gives: the
/// query can look exactly right and the answer still be wrong. Here the engine
/// settles it by finding the table or not.
#[test]
fn a_table_named_in_parts_is_found() {
    let conn = Connection::open_in_memory().expect("an in-memory database");
    conn.execute_batch(
        "CREATE SCHEMA sch;
         CREATE TABLE sch.orders (product VARCHAR, revenue DOUBLE);
         INSERT INTO sch.orders VALUES ('Widget', 100), ('Gadget', 300);",
    )
    .expect("the fixture schema and table");

    let (_, rows) = run_on(&conn, "sch.orders then sort [revenue] descending then take 1", "sch.orders");
    assert_eq!(rows, vec![vec!["Gadget".to_string(), "300.0".to_string()]]);

    // And three parts, which is the shape a warehouse catalog uses. `memory` is
    // the name DuckDB gives the database an in-memory connection opens.
    let (_, rows) = run_on(&conn, "memory.sch.orders then sort [revenue] then take 1", "memory.sch.orders");
    assert_eq!(rows, vec![vec!["Widget".to_string(), "100.0".to_string()]]);
}

/// A dot joins a name only where a table is named.
///
/// The four positions that read a table accept one. Everywhere else the dot
/// means what it always meant, which is nothing, and the grammar says so.
#[test]
fn a_dot_outside_a_table_name_is_still_refused() {
    let columns = Schema::new([("a", Type::Number)]);

    // A value is not a name, so this is the message it always gave.
    let refused = compile("t then keep where [a] > x.y", &columns, "sql")
        .err()
        .expect("a dot in an expression is not a table name");
    assert!(
        refused.message.contains("bare word"),
        "the message changed: {}",
        refused.message
    );

    // **The word after the dot is looked at before the dot is taken.** `then` is
    // an ordinary word to the lexer, so without that lookahead this reads as a
    // table called `t.then` and the error lands on the next step instead.
    let refused = compile("t. then take 3", &columns, "sql")
        .err()
        .expect("a dot with no part after it is not a name");
    assert!(
        refused.message.contains("names a table in parts"),
        "the message did not name the real problem: {}",
        refused.message
    );
}
