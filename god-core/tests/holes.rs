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

/// **A row is kept where its condition is true, and an unknown answer is not
/// true.** pandas answers False for a comparison over a hole, which keeps
/// nothing and agrees, except that `!=` answers True and `~` turns an unknown
/// False into True. So `not` is pushed in to the comparisons and the two that
/// would keep a hole say they do not. dplyr's logic already reasons about the
/// unknown; its `%in%` and `grepl` answer FALSE for a hole, so they are the two
/// it says out loud.
#[test]
fn an_unknown_condition_keeps_no_row() {
    let cases = [
        (
            "sales then keep where [region] is not \"West\"",
            "(d[\"region\"] != \"West\") & d[\"region\"].notna()",
        ),
        (
            "sales then keep where not ([region] is \"West\")",
            "(d[\"region\"] != \"West\") & d[\"region\"].notna()",
        ),
        (
            "sales then keep where not ([revenue] > 100 and [cost] > 50)",
            "((d[\"revenue\"] <= 100) | (d[\"cost\"] <= 50))",
        ),
        (
            "sales then keep where [product] not in {\"Widget\"}",
            "(~d[\"product\"].isin([\"Widget\"]) & d[\"product\"].notna())",
        ),
        (
            "sales then keep where not ([product] contains \"W\")",
            "(~d[\"product\"].str.contains(\"W\", regex=False, na=False) & d[\"product\"].notna())",
        ),
        (
            "sales then keep where not between([revenue], 100, 300)",
            "((d[\"revenue\"] < 100) | (d[\"revenue\"] > 300))",
        ),
        (
            "sales then add [flag] as when(not ([region] is \"West\"), \"away\", otherwise \"home\")",
            "np.select([((d[\"region\"] != \"West\") & d[\"region\"].notna())]",
        ),
    ];
    for (sentence, wanted) in cases {
        let text = printed(sentence, "pandas");
        assert!(text.contains(wanted), "{sentence}\nwanted {wanted}\n{text}");
        assert!(!text.contains("~("), "no negation of a whole condition is left: {sentence}\n{text}");
    }

    let not_in = printed("sales then keep where [product] not in {\"Widget\"}", "dplyr");
    assert!(not_in.contains("!if_else(is.na(product), NA, product %in% c(\"Widget\"))"), "{not_in}");
    let contains = printed("sales then keep where not ([product] contains \"W\")", "dplyr");
    assert!(contains.contains("!stringr::str_detect(product, stringr::fixed(\"W\"))"), "{contains}");
}

/// **A comparison kept as a value is missing where the query's is.** numpy's
/// booleans cannot hold an unknown, so pandas writes a nullable boolean and
/// masks the holes; its `&`, `|` and `~` then reason about the unknown the way
/// SQL does, and a compound needs nothing more.
#[test]
fn a_comparison_kept_as_a_value_can_be_missing() {
    let one = printed("sales then add [big] as [revenue] > 200", "pandas");
    assert!(
        one.contains("(d[\"revenue\"] > 200).astype(\"boolean\").mask(d[\"revenue\"].isna())"),
        "{one}"
    );
    let two = printed("sales then add [both] as [revenue] > 100 and [cost] > 50", "pandas");
    assert!(
        two.contains("((d[\"revenue\"] > 100).astype(\"boolean\").mask(d[\"revenue\"].isna()) & (d[\"cost\"] > 50).astype(\"boolean\").mask(d[\"cost\"].isna()))"),
        "{two}"
    );
    let literal = printed("sales then keep where [revenue] > 200", "pandas");
    assert!(!literal.contains("astype"), "a condition that only keeps rows needs none of it: {literal}");
}

/// **An aggregate of nothing answers the way the query does.** A total of no
/// values is missing, which `rows-that-are-not-there` shows and teaches;
/// pandas, polars and R all total nothing to 0, and R's `min` and `max` of
/// nothing are infinities. A join of nothing is missing rather than an empty
/// text, in every printer. A distinct count skips a missing value, which
/// polars and dplyr count as one more. And `first` and `last` read the row,
/// whatever it holds, where pandas' own words skip a hole.
#[test]
fn an_aggregate_of_nothing_answers_as_the_query_does() {
    let summary = "sales then summarize [t] as total([revenue]), [lo] as smallest([revenue]), [j] as join_rows([product], \", \"), [u] as unique_count([product]), [f] as first([product]) by [region]";

    let pandas = printed(summary, "pandas");
    for wanted in [
        "t=(\"revenue\", lambda s: s.sum(min_count=1))",
        "j=(\"product\", lambda s: \", \".join(s.dropna()) if s.notna().any() else None)",
        "f=(\"product\", lambda s: s.iloc[0])",
    ] {
        assert!(pandas.contains(wanted), "wanted {wanted}\n{pandas}");
    }
    let windowed = printed(
        "sales then sort [ordered_on] then add [t] as total([revenue]), [l] as last([cost]) by [region]",
        "pandas",
    );
    assert!(windowed.contains(".transform(\"sum\", min_count=1)"), "{windowed}");
    assert!(windowed.contains(".transform(lambda s: s.iloc[-1])"), "{windowed}");

    let polars = printed(summary, "polars");
    for wanted in [
        "pl.when(pl.col(\"revenue\").count() > 0).then(pl.col(\"revenue\").sum())",
        "pl.when(pl.col(\"product\").count() > 0).then(pl.col(\"product\").drop_nulls().str.join(\", \"))",
        "pl.col(\"product\").drop_nulls().n_unique()",
    ] {
        assert!(polars.contains(wanted), "wanted {wanted}\n{polars}");
    }

    let pyspark = printed(summary, "pyspark");
    assert!(
        pyspark.contains("F.when(F.count(F.col(\"product\")) > 0, F.array_join(F.collect_list(F.col(\"product\")), \", \"))"),
        "{pyspark}"
    );

    let dplyr = printed(summary, "dplyr");
    for wanted in [
        "t = if (all(is.na(revenue))) NA else sum(revenue, na.rm = TRUE)",
        "lo = if (all(is.na(revenue))) NA else min(revenue, na.rm = TRUE)",
        "j = if (all(is.na(product))) NA else paste(product[!is.na(product)], collapse = \", \")",
        "u = n_distinct(product, na.rm = TRUE)",
    ] {
        assert!(dplyr.contains(wanted), "wanted {wanted}\n{dplyr}");
    }
}

/// **A cell of a `widen` is an aggregate too**, and gets the same answers: a
/// cell holding only holes totals to missing, a hole is skipped by every
/// aggregate, and a combination the table never had counts 0 distinct values,
/// because a count of nothing is 0. polars' `"len"` counted a cell's rows,
/// so a value seen twice counted twice.
#[test]
fn a_widened_cell_is_an_aggregate_like_any_other() {
    let total = "sales then widen name [product], value total([revenue]) by [region] giving [Widget, Gadget]";
    let count = "sales then widen name [product], value unique_count([ordered_on]) by [region] giving [Widget, Gadget]";
    let average = "sales then widen name [product], value average([revenue]) by [region] giving [Widget, Gadget]";

    assert!(printed(total, "pandas").contains("[\"revenue\"].sum(min_count=1)"));
    assert!(printed(count, "pandas").contains("unstack(\"product\", fill_value=0)"));
    assert!(printed(total, "polars")
        .contains("aggregate_function=pl.when(pl.element().count() > 0).then(pl.element().sum())"));
    let polars_count = printed(count, "polars");
    assert!(polars_count.contains("aggregate_function=pl.element().drop_nulls().n_unique()"), "{polars_count}");
    assert!(!polars_count.contains("\"len\""), "{polars_count}");
    assert!(printed(count, "pyspark").contains("na.fill(0, subset=[\"Widget\", \"Gadget\"])"));
    let dplyr_average = printed(average, "dplyr");
    assert!(dplyr_average.contains("values_fn = \\(x) mean(x, na.rm = TRUE)"), "{dplyr_average}");
    let dplyr_count = printed(count, "dplyr");
    assert!(dplyr_count.contains("values_fn = \\(x) n_distinct(x, na.rm = TRUE), values_fill = 0"), "{dplyr_count}");
}

/// **A missing value is not a category, so it makes no combinations.** The
/// book says so of `add_combinations`, and tidyr's `complete` crosses `NA` as
/// a value like any other, so printed dplyr names each column's present
/// values. pandas and polars drop the holes from their grid already.
#[test]
fn a_hole_makes_no_combinations() {
    let dplyr = printed("sales then add_combinations [region, product] by [ordered_on]", "dplyr");
    assert!(
        dplyr.contains("complete(region = unique(region[!is.na(region)]), product = unique(product[!is.na(product)]))"),
        "{dplyr}"
    );
    assert!(printed("sales then add_combinations [region, product]", "pandas").contains(".dropna()"));
    assert!(printed("sales then add_combinations [region, product]", "polars").contains(".drop_nulls()"));
}

/// **A whole number is written whole, however it is stored**, which the query
/// has done since `to_text` was ruled in spec §4.7. A hole is how a number
/// column becomes a float column in pandas, so this is where the printers
/// were caught writing `100.0`; R's `as.character` writes 100000 as `1e+05`
/// with no hole at all. Text that is not a number is cast as it always was.
#[test]
fn a_whole_number_is_written_whole_in_every_printer() {
    let number = "sales then add [t] as to_text([revenue])";
    let text = "sales then add [t] as to_text([ordered_on])";
    for (backend, whole) in [
        ("pandas", "d[\"revenue\"].map(lambda v: str(int(v)) if float(v).is_integer() and abs(v) < 1e15 else str(v), na_action=\"ignore\")"),
        ("polars", "pl.when((pl.col(\"revenue\") % 1 == 0) & (pl.col(\"revenue\").abs() < 1e15)).then(pl.col(\"revenue\").cast(pl.Int64, strict=False).cast(pl.String)).otherwise(pl.col(\"revenue\").cast(pl.String))"),
        ("pyspark", "F.when((F.col(\"revenue\") == F.floor(F.col(\"revenue\"))) & (F.abs(F.col(\"revenue\")) < 1e15), F.col(\"revenue\").cast(\"bigint\").cast(\"string\")).otherwise(F.col(\"revenue\").cast(\"string\"))"),
        ("dplyr", "if_else(revenue == floor(revenue) & abs(revenue) < 1e15, sprintf(\"%.0f\", revenue), as.character(revenue))"),
    ] {
        let printed_number = printed(number, backend);
        assert!(printed_number.contains(whole), "{backend}\nwanted {whole}\n{printed_number}");
        let printed_text = printed(text, backend);
        assert!(!printed_text.contains("1e15"), "text is cast as it was: {backend}\n{printed_text}");
    }
}

/// **A row whose own value is missing gets missing from `rank` and
/// `running_total`, and no other row changes** (spec §4.7). pandas and polars
/// answer so by default and print what they always did; the query and PySpark
/// wrap the window, and dplyr's `cumsum`, which carries the hole to every row
/// after it, is given the hole as 0 and then has it put back.
#[test]
fn a_hole_has_no_place_and_no_total_in_any_writer() {
    let ranked = "sales then add [r] as rank([revenue] descending)";
    let summed = "sales then sort [ordered_on] then add [s] as running_total([revenue]) by [region]";

    let sql = printed(ranked, "sql");
    assert!(sql.contains("CASE WHEN \"revenue\" IS NULL THEN NULL ELSE RANK() OVER"), "{sql}");
    let sql = printed(summed, "sql");
    assert!(sql.contains("CASE WHEN \"revenue\" IS NULL THEN NULL ELSE sum(\"revenue\") OVER"), "{sql}");

    assert!(printed(ranked, "pyspark").contains("F.when(F.col(\"revenue\").isNotNull(), F.rank().over("));
    assert!(printed(summed, "pyspark").contains("F.when(F.col(\"revenue\").isNotNull(), F.sum(F.col(\"revenue\")).over("));

    let dplyr = printed(summed, "dplyr");
    assert!(dplyr.contains("if_else(is.na(revenue), NA, cumsum(coalesce(revenue, 0)))"), "{dplyr}");
    assert!(printed(ranked, "dplyr").contains("min_rank(desc(revenue))"));
    assert!(printed(summed, "pandas").contains("[\"revenue\"].cumsum()"));
    assert!(printed(summed, "polars").contains("pl.col(\"revenue\").cum_sum()"));
}

/// **`take ... with ties` reads the rows where the sort left them, holes
/// included**, so a missing key ranks at the end the sort put it, which the
/// query's own `RANK` over the sort's order already does. pandas and polars
/// left it unranked, and dplyr's slices leave a hole for last, which no cut
/// reached when the holes sat at the end being taken.
#[test]
fn a_tie_counts_the_holes_where_the_sort_put_them() {
    let first = "sales then sort [revenue] then take 3 with ties";
    let holes_first = "sales then sort [revenue] missing first then take 3 with ties";
    let far_end = "sales then sort [revenue] then take_last 3 with ties";

    assert!(printed(first, "pandas").contains("na_option=\"bottom\""));
    assert!(printed(holes_first, "pandas").contains("na_option=\"top\""));
    assert!(printed(far_end, "pandas").contains("na_option=\"top\""));

    assert!(printed(first, "polars").contains(".fill_null(pl.col(\"revenue\").count() + 1)"));
    assert!(printed(holes_first, "polars").contains(" + pl.col(\"revenue\").null_count()).fill_null(1)"));

    assert!(printed(first, "dplyr").contains("slice_min(order_by = revenue,"));
    assert!(printed(holes_first, "dplyr").contains("slice_min(order_by = tibble(!is.na(revenue), revenue),"));
    assert!(printed(far_end, "dplyr").contains("slice_max(order_by = tibble(is.na(revenue), revenue),"));
}

/// **The text a hole is filled with is a value, not a column**, and polars'
/// `coalesce` reads a bare piece of text in its list as a column name, so
/// `first_present([product], "none")` failed looking for a column called
/// `none`. `fill_null` reads it as a value already. Found by the same probes.
#[test]
fn a_text_fallback_is_a_value_in_polars() {
    let text = printed("sales then add [label] as first_present([product], [region], \"none\")", "polars");
    assert!(text.contains("pl.coalesce([pl.col(\"product\"), pl.col(\"region\"), pl.lit(\"none\")])"), "{text}");
}
