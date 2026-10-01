//! The printed code over a table with holes in it.
//!
//! **The printing backends are for reading, and a hole is where they read
//! wrongly.** Each library has its own answer for a missing value, and its
//! default is not always the one the query gives: pandas drops a missing group,
//! R totals nothing to zero, dplyr joins a missing key to a missing key. A
//! sentence means one table, so where a library's default answers another, the
//! printed code says the difference out loud.
//!
//! These tests hold the spelling. `parity/printed.py` runs it, against a copy
//! of the fixture with a hole in every column, and compares the tables: the
//! spelling is only worth asserting because something else proves it answers.

use god_core::check::Tables;
use god_core::{compile_tables, Schema, Type};

fn sales() -> Schema {
    Schema::new([
        ("region", Type::Text),
        ("product", Type::Text),
        ("revenue", Type::Number),
        ("cost", Type::Number),
        ("ordered_on", Type::Text),
    ])
}

fn partners() -> Tables {
    Tables::new([
        ("products", Schema::new([("product", Type::Text), ("maker", Type::Text)])),
        ("regions", Schema::new([("area", Type::Text), ("manager", Type::Text)])),
    ])
}

fn printed(sentence: &str, backend: &str) -> String {
    match compile_tables(sentence, &sales(), &partners(), backend) {
        Ok(compiled) => compiled.text,
        Err(d) => panic!("\n{}\n", d.render(sentence)),
    }
}

/// **A missing key is a group, in pandas too.** `groupby` drops one by default,
/// and DuckDB, Spark, dplyr and polars all keep it, so every place pandas
/// groups says so: a summary, a grouped `take`, a window or a share in `add`,
/// and a `widen`. `widen` and `lengthen` leave `pivot_table` for it, because
/// its `dropna=False` builds every combination of two keys, rows the table
/// never had.
#[test]
fn pandas_keeps_a_missing_group() {
    for sentence in [
        "sales then summarize [n] as row_count() by [region]",
        "sales then sort [revenue] then take 1 by [region]",
        "sales then sort [revenue] then take 1 by [region] with ties",
        "sales then add [share] as [revenue] / total([revenue]) by [region]",
        "sales then sort [ordered_on] then add [so_far] as running_total([revenue]) by [region]",
        "sales then sort [ordered_on] then add [r] as rolling(total([revenue]), 2) by [region]",
        "sales then widen name [product], value [revenue] by [region] giving [Widget, Gadget]",
    ] {
        let text = printed(sentence, "pandas");
        assert!(text.contains("dropna=False"), "{sentence}\n{text}");
        assert!(
            !text.contains("groupby(") || text.matches("groupby(").count() == text.matches("dropna=False").count(),
            "every grouping keeps the missing key: {sentence}\n{text}"
        );
    }
    let widened = printed(
        "sales then widen name [product], value [revenue] by [region] giving [Widget, Gadget]",
        "pandas",
    );
    assert!(!widened.contains("pivot_table"), "{widened}");
    let lengthened = printed(
        "sales then lengthen [revenue, cost] as name [measure], value [amount]",
        "pandas",
    );
    assert!(!lengthened.contains("pivot_table"), "{lengthened}");
}
