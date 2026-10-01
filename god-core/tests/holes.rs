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

/// **A missing key matches nothing, itself included.** That is how every
/// engine the sentence runs on joins, and two of the printing libraries do the
/// opposite by default: pandas' `merge` and `isin` and dplyr's joins pair a
/// missing key with a missing key. So pandas leaves the other table's
/// missing-key rows out of the match, and hands them back unmatched in a full
/// join; dplyr says `na_matches = "never"`.
#[test]
fn a_missing_key_matches_nothing() {
    for (sentence, kind) in [
        ("sales then join products by [product]", "left_join"),
        ("sales then join products by [product] unmatched \"none\"", "inner_join"),
        ("sales then join products by [product] unmatched \"both\"", "full_join"),
        ("sales then join regions by [region] is [area]", "left_join"),
        ("sales then keep where matching(products, by [product])", "semi_join"),
        ("sales then keep where not matching(regions, by [region] is [area])", "anti_join"),
    ] {
        let dplyr = printed(sentence, "dplyr");
        assert!(dplyr.contains(kind), "{sentence}\n{dplyr}");
        assert!(dplyr.contains("na_matches = \"never\""), "{sentence}\n{dplyr}");
    }

    let left = printed("sales then join products by [product]", "pandas");
    assert!(left.contains("merge(products.dropna(subset=[\"product\"])"), "{left}");
    let full = printed("sales then join products by [product] unmatched \"both\"", "pandas");
    assert!(full.contains("products[products[\"product\"].isna()]"), "{full}");
    let renamed = printed("sales then join regions by [region] is [area] unmatched \"both\"", "pandas");
    assert!(renamed.contains("regions.dropna(subset=[\"area\"])"), "{renamed}");
    assert!(renamed.contains("regions[regions[\"area\"].isna()]"), "{renamed}");
    let semi = printed("sales then keep where matching(products, by [product])", "pandas");
    assert!(semi.contains(".isin(products[\"product\"].dropna())"), "{semi}");
}

/// **A full join hands back one key column in polars, whatever the names.**
/// polars does not coalesce a full join's key by default, so a row only the
/// other table had came back with its key missing and its value in
/// `product_right`. Not a hole in the data at all: found by the same probe.
#[test]
fn a_polars_full_join_keeps_one_key() {
    let full = printed("sales then join products by [product] unmatched \"both\"", "polars");
    assert!(full.contains("how=\"full\", coalesce=True"), "{full}");
    let left = printed("sales then join products by [product]", "polars");
    assert!(!left.contains("coalesce"), "a left join keeps its own key and needs no word: {left}");
}
