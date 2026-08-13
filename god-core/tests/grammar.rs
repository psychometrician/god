//! Properties of the whole grammar, checked by walking it.
//!
//! **Nothing in this file writes down its own copy of the vocabulary.** A test
//! that names the verbs it expects stops testing anything the moment a verb is
//! added: the grammar grows, no assertion moves, and the suite goes on claiming
//! a coverage it lost. So every test here enumerates
//! [`god_core::vocabulary`] and fails when something in it is unfinished.

use god_core::backend;
use god_core::vocabulary::{self, Kind};
use god_core::{compile, parse, Schema, Type};

fn schema() -> Schema {
    Schema::new([
        ("region", Type::Text),
        ("product", Type::Text),
        ("revenue", Type::Number),
        ("cost", Type::Number),
        // **A column the grammar has no opinion about**, which agrees with every
        // type and so suits every function's argument. It is what
        // `pipeline_using` hands over, so the generator never has to know which
        // kind a function wants: this test is about whether a backend can write
        // the word, and a type check firing first would answer a different
        // question. `lower` arriving in the table is what found this.
        ("anything", Type::Unknown),
    ])
}

/// A pipeline that exercises one function, built from its own entry in the
/// vocabulary so neither the arity nor the step is guessed.
///
/// **The step depends on the kind**, and getting that from the table rather than
/// from a list here is the whole point. A window answers once per row, so it
/// belongs in `add` and is refused in `summarize`; `row_number` additionally
/// needs an order, so the sentence carries a `sort`. Anything added to the
/// vocabulary with a kind this does not handle fails loudly rather than being
/// quietly skipped.
fn pipeline_using(f: &vocabulary::Function) -> String {
    // A variadic function is given its floor plus one, so the sentence exercises
    // the list rather than the smallest legal case.
    let count = match f.arity {
        vocabulary::Arity::Exactly(n) => n,
        vocabulary::Arity::AtLeast(n) => n + 1,
    };
    // The column of no particular kind, repeated. Using a number here meant the
    // sentence stopped compiling the day a text function was added, which is a
    // type check answering a coverage question.
    let args = vec!["[anything]"; count].join(", ");
    match f.kind {
        // An aggregate collapses a group, which is what `summarize` is for, and
        // `summarize` refuses anything that does not.
        vocabulary::Kind::Aggregate => {
            format!("sales then summarize [answer] as {}({args})", f.name)
        }
        // A scalar answers row by row, so it belongs in `add`. This arm shared
        // the one above until `first_present` arrived, and the sharing was
        // wrong the whole time: it was simply never exercised, because every
        // function in the table was an aggregate.
        vocabulary::Kind::Scalar => format!("sales then add [answer] as {}({args})", f.name),
        // A window also belongs in `add`, and `row_number` additionally needs an
        // order, so the sentence carries a `sort`.
        vocabulary::Kind::Window => format!(
            "sales then sort [revenue] descending then add [answer] as {}({args})",
            f.name
        ),
    }
}

/// Every function the grammar has must be spelled by every backend.
///
/// This does not compare two lists; it runs each function through each backend
/// for real. A backend that has never been told how to write a function does not
/// return a wrong answer here — it fails, loudly, which is the only way this
/// check is worth having.
#[test]
fn every_function_is_spelled_by_every_backend() {
    for f in vocabulary::FUNCTIONS {
        for name in backend::names() {
            let pipeline = pipeline_using(f);
            let result = compile(&pipeline, &schema(), name);
            assert!(
                result.is_ok(),
                "the `{name}` backend cannot write `{}`: {}",
                f.name,
                result.err().map(|d| d.message).unwrap_or_default()
            );
            let text = result.unwrap().text;
            assert!(
                !text.is_empty(),
                "the `{name}` backend produced nothing for `{}`",
                f.name
            );
        }
    }
}

/// One well-formed sentence per verb.
///
/// Written as data rather than as a second list of verbs: the assertion in
/// `every_verb_parses` is what forces this table to stay complete, and more than
/// one test walks it, so a verb added to the vocabulary is covered everywhere at
/// once rather than wherever somebody remembered.
fn verb_sentences() -> &'static [(&'static str, &'static str)] {
    &[
        ("keep", r#"sales then keep where [revenue] > 1"#),
        ("pick", r#"sales then pick [region]"#),
        ("add", r#"sales then add [x] as [revenue] - [cost]"#),
        ("summarize", r#"sales then summarize [n] as row_count()"#),
        ("sort", r#"sales then sort [revenue] descending"#),
        ("take", r#"sales then take 3"#),
        // Always after a sort, which is the one rule that sets it apart from
        // `take`: a table has no far end until something says which way it runs.
        ("take_last", r#"sales then sort [revenue] then take_last 3"#),
        ("join", r#"sales then join products by [region]"#),
        ("add_rows", r#"sales then pick [region] then add_rows products"#),
        // Two columns, always: one on its own has no combinations to make, so
        // the shortest legal sentence here is the two-column one.
        ("add_combinations", r#"sales then add_combinations [region, product]"#),
        ("drop_duplicates", r#"sales then drop_duplicates"#),
        ("rename", r#"sales then rename [area] as [region]"#),
        ("drop_missing", r#"sales then drop_missing [region]"#),
        ("fill_missing", r#"sales then fill_missing [region] as "none""#),
        ("lengthen", r#"sales then lengthen [revenue, cost]"#),
        // `widen` reads the names out of one column and the values out of
        // another, so it needs a table shaped like one something has lengthened.
        // `region` holds the names and `revenue` the values, which leaves
        // `product` to say which rows go together.
        //
        // **It declares what it makes, and it has to here.** A bare `widen` is a
        // sentence Spark cannot write, which is a decision rather than a defect
        // (§4.5.5), so the one sentence shared by every walker of this table is
        // the one every backend can render. The bare form is covered in
        // `reshape.rs`, and the refusal itself in
        // `spark_refuses_the_one_sentence_it_cannot_write`.
        ("widen", r#"sales then pick [product, region, revenue] then widen name [region], value [revenue] giving [West, East]"#),
    ]
}

/// Every verb the grammar declares must actually parse.
///
/// The list and the parser are two statements of the same fact, and a verb in
/// one and not the other is the shape where a chapter documents something that
/// was never built.
#[test]
fn every_verb_parses() {
    let sentences = verb_sentences();

    // `join` is the only verb that names a second table, so it is the only one
    // that needs anything beyond the head schema. Describing it for every verb
    // costs nothing and keeps the loop one loop.
    let others = god_core::check::Tables::new([(
        "products",
        // Just the one column, so `add_rows` has a table it can legally stack
        // against after a `pick`, and `join` still has a key to match on.
        Schema::new([("region", Type::Text)]),
    )]);

    for verb in vocabulary::VERBS {
        let found = sentences.iter().find(|(v, _)| v == verb).unwrap_or_else(|| {
            panic!("`{verb}` is in the vocabulary and has no sentence in this test")
        });
        let result = god_core::compile_tables(found.1, &schema(), &others, "sql");
        assert!(
            result.is_ok(),
            "`{verb}` is in the vocabulary and will not parse: {}",
            result.err().map(|d| d.message).unwrap_or_default()
        );
    }

    assert_eq!(
        sentences.len(),
        vocabulary::VERBS.len(),
        "this test has a sentence for a verb the vocabulary does not have"
    );
}

/// Every verb the grammar declares must draw a band.
///
/// **This lives here rather than beside the drawing's own tests because the
/// table of sentences lives here.** A second copy of it would go stale the day a
/// verb is added, which is the failure this whole file exists to prevent, and a
/// drawing that quietly omits a step is the worst kind of wrong: it looks
/// finished.
#[test]
fn every_verb_draws_a_band() {
    let others = god_core::check::Tables::new([(
        "products",
        Schema::new([("region", Type::Text)]),
    )]);

    for (verb, sentence) in verb_sentences() {
        let plan = parse::parse(sentence).expect("the sentence for this verb parses");
        let drawn = god_core::draw::ladder(&plan, sentence, &schema(), &others);
        let bands = drawn.lines().filter(|l| l.starts_with('├') || l.starts_with('└')).count();
        assert_eq!(
            bands,
            plan.steps.len(),
            "`{verb}` has {} steps and the drawing shows {bands} of them:\n{drawn}",
            plan.steps.len()
        );
    }
}

/// No word may hold two roles.
///
/// A name that is a verb in one place and a function in another is a rule a
/// reader has to learn twice, and it is the defect that an audit catches and
/// reading never does.
#[test]
fn no_name_holds_two_roles() {
    let names = vocabulary::all_names();
    for (i, name) in names.iter().enumerate() {
        assert!(
            !names[..i].contains(name),
            "`{name}` appears twice in the vocabulary, in two different roles"
        );
    }
}

/// The plan has to agree with the vocabulary about what each function *is*.
///
/// Three kinds and two questions the plan can answer, so this checks both:
/// whether a value collapses a group, and whether it answers once per row. A
/// disagreement here is what lets a window slip into a `summarize`, or an
/// aggregate be refused from one.
#[test]
fn the_plan_agrees_with_the_vocabulary_about_every_kind() {
    for f in vocabulary::FUNCTIONS {
        let plan = parse::parse(&pipeline_using(f)).expect("a legal sentence");
        // The step holding the value, whichever step that is: a window sentence
        // carries a `sort` in front of its `add`.
        let value = plan
            .steps
            .iter()
            .find_map(|step| match step {
                god_core::plan::Step::Summarize { values, .. }
                | god_core::plan::Step::Add { values, .. } => Some(&values[0].value),
                _ => None,
            })
            .expect("the sentence makes a value");

        assert_eq!(
            value.aggregates(),
            f.kind == Kind::Aggregate,
            "`{}` is {:?} in the vocabulary and the plan disagrees about collapsing",
            f.name,
            f.kind
        );
        assert_eq!(
            value.windows(),
            f.kind == Kind::Window,
            "`{}` is {:?} in the vocabulary and the plan disagrees about windowing",
            f.name,
            f.kind
        );
    }
}

// -- the round trip --------------------------------------------------------

/// Text becomes a plan and a plan becomes text, and doing both changes nothing.
///
/// **The comparison is between the two plans, not between the two strings.**
/// Comparing the printed pipeline to itself printed twice only proves the
/// printer is consistent: one that drops the same clause on every pass drops it
/// on both, the two strings agree perfectly, and the sentence quietly means less
/// than it did. That hole was found by deliberately breaking the printer and
/// watching this test stay green, which is the reason it is written this way.
#[test]
fn a_pipeline_survives_being_written_back_out() {
    let pipelines = [
        r#"sales then keep where [region] is "West""#,
        r#"sales then keep where [region] is not "West""#,
        r#"sales then keep where [cost] is missing"#,
        r#"sales then keep where [cost] is not missing"#,
        r#"sales then keep where [region] in {"West", "East"}"#,
        r#"sales then keep where [region] not in {"West"}"#,
        r#"sales then keep where not ([revenue] > 1)"#,
        r#"sales then keep where [revenue] > 1 and [cost] < 2 or [region] is "West""#,
        r#"sales then add [x] as [revenue] - [cost] * 2"#,
        r#"sales then add [x] as ([revenue] - [cost]) * 2"#,
        r#"sales then add [share] as total([revenue]) by [product]"#,
        r#"sales then summarize [a] as total([revenue]), [b] as row_count() by [region, product]"#,
        r#"sales then sort [revenue] descending, [cost]"#,
        r#"sales then pick all_but [cost]"#,
        r#"sales then take 10"#,
        r#"sales then keep where [revenue] > 1.5"#,
        r#"sales then keep where [region] is "it's quoted""#,
        r#"sales then lengthen [revenue, cost]"#,
        r#"sales then lengthen [revenue, cost] as name [thing], value [amount]"#,
        r#"sales then lengthen all_but [region, product, anything]"#,
        // **`widen` writes its `by` out here**, because the checker fills it in
        // where the caller left it out and what comes back is the settled
        // sentence. A pipeline that left it out would print with it and so would
        // not survive this comparison — which is a property of every clause the
        // checker settles, and the reason `join` is not in this list either.
        r#"sales then pick [product, region, revenue] then widen name [region], value [revenue] by [product]"#,
        r#"sales then pick [product, region, revenue] then widen name [region], value average([revenue]) by [product] missing 0 giving [West, East]"#,
    ];

    for pipeline in pipelines {
        let printed = show_as_god(pipeline);

        let before = parse::parse(pipeline).expect("a legal sentence");
        let after = parse::parse(&printed).expect("what was printed is legal");
        assert_eq!(
            after.without_spans(),
            before.without_spans(),
            "\n  writing this out changed what it means\n  from: {pipeline}\n  to:   {printed}"
        );

        // And printing is stable, so a saved pipeline does not drift each time
        // it passes through.
        assert_eq!(show_as_god(&printed), printed, "\n  from: {pipeline}");
    }
}

fn show_as_god(pipeline: &str) -> String {
    match compile(pipeline, &schema(), "god") {
        Ok(c) => c.text,
        Err(d) => panic!("\n{}\n", d.render(pipeline)),
    }
}

// -- what a reader is handed -----------------------------------------------

/// The whole pitch, in one assertion: a sentence in the grammar, and the same
/// sentence in a language the reader already uses.
#[test]
fn the_pipeline_reads_as_dplyr() {
    let pipeline = r#"sales
         then keep where [region] is "West"
         then add [margin] as [revenue] - [cost]
         then summarize [margin] as total([margin]), [orders] as row_count() by [product]
         then sort [margin] descending
         then take 10"#;

    assert_eq!(
        compile(pipeline, &schema(), "dplyr").unwrap().text,
        "sales |>\n\
         \x20 filter((region == \"West\")) |>\n\
         \x20 mutate(margin = (revenue - cost)) |>\n\
         \x20 summarise(margin = sum(margin, na.rm = TRUE), orders = n(), .by = product) |>\n\
         \x20 arrange(desc(margin)) |>\n\
         \x20 head(10)"
    );
}

#[test]
fn a_column_name_r_cannot_write_bare_gets_backticks() {
    let schema = Schema::new([("order id", Type::Number)]);
    assert_eq!(
        compile("orders then sort [order id]", &schema, "dplyr").unwrap().text,
        "orders |>\n  arrange(`order id`)"
    );
}

/// A column may be called anything at all, including a word the grammar uses.
/// Inside brackets it is a column, always — which is the rule that lets the
/// grammar keep plain words without forbidding anyone from using them.
#[test]
fn a_column_may_be_named_after_a_verb() {
    let schema = Schema::new([("sort", Type::Number), ("then", Type::Text)]);
    let compiled = compile(
        r#"t then keep where [then] is "x" then sort [sort] descending"#,
        &schema,
        "sql",
    )
    .expect("a column called `sort` is a column");
    assert!(compiled.text.contains(r#""sort" DESC"#), "{}", compiled.text);
    assert!(compiled.text.contains(r#""then" = 'x'"#), "{}", compiled.text);
}

/// A `%` in the value is a percent sign, not "anything at all".
///
/// **`LIKE` gives the value's own characters a second meaning**, so a pattern
/// built by pasting it together is wrong the moment someone searches for a
/// percentage or an underscore. The failure is silent: the query runs and
/// matches too much.
#[test]
fn a_wildcard_in_the_value_is_escaped() {
    let sql = match compile(r#"sales then keep where [region] contains "100%""#, &schema(), "sql") {
        Ok(c) => c.text,
        Err(d) => panic!("\n{}\n", d.render("")),
    };
    assert!(sql.contains(r"'%100\%%'"), "the percent is escaped: {sql}");
    assert!(sql.contains(r"ESCAPE '\'"), "and the escape is declared: {sql}");

    let underscore =
        compile(r#"sales then keep where [region] starts "a_b""#, &schema(), "sql")
            .expect("a legal sentence")
            .text;
    assert!(underscore.contains(r"'a\_b%'"), "the underscore too: {underscore}");
}

/// The two dialects, and the difference that decides whether the answer is right
/// rather than whether the query runs.
///
/// **`SELECT "region"` is a column on DuckDB and the text `'region'` on Spark.**
/// So the wrong quote does not fail: it returns the column's own name once per
/// row, for every row, and the query reads perfectly. That is why this is
/// asserted here and why `parity/spark.py` compares tables on two engines rather
/// than checking that a query parsed.
#[test]
fn the_two_sql_dialects_differ_where_they_have_to() {
    let sentence = r#"sales then pick all_but [cost] then keep where [region] starts "W""#;
    let duck = compile(sentence, &schema(), "sql").expect("legal").text;
    let spark = compile(sentence, &schema(), "spark").expect("legal").text;

    assert!(duck.contains("\"region\""), "DuckDB quotes an identifier with double quotes:\n{duck}");
    assert!(spark.contains("`region`"), "Spark quotes an identifier with backticks:\n{spark}");
    assert!(!spark.contains('"'), "a double quote in Spark's output is a text value, not a column:\n{spark}");

    assert!(duck.contains("EXCLUDE"), "{duck}");
    assert!(spark.contains("EXCEPT") && !spark.contains("EXCLUDE"), "{spark}");

    // A backslash is a backslash on one engine and an escape on the other, so
    // the escape character `starts` emits has to be written twice for Spark.
    assert!(duck.contains(r"ESCAPE '\'"), "{duck}");
    assert!(spark.contains(r"ESCAPE '\\'"), "{spark}");
}

/// Where an engine cannot say what a sentence means, it says so.
///
/// This is §3.1's rule reaching the backends: a quiet difference between engines
/// is the one outcome the design is against, because it is the one nobody sees.
#[test]
fn spark_refuses_the_one_sentence_it_cannot_write() {
    let schema = Schema::new([
        ("student", Type::Text),
        ("question", Type::Text),
        ("mark", Type::Number),
    ]);
    let bare = "marks then widen name [question], value [mark] by [student]";
    let declared =
        "marks then widen name [question], value [mark] by [student] giving [q1, q2]";

    // DuckDB works the values out for itself, so the bare form is fine there.
    assert!(compile(bare, &schema, "sql").is_ok());

    let Err(refusal) = compile(bare, &schema, "spark") else {
        panic!("Spark cannot write this sentence and did not say so");
    };
    assert!(
        refusal.message.contains("giving [q1, q2, q3]"),
        "the refusal has to name the fix: {}",
        refusal.message
    );

    // And the fix is a clause the grammar already had, rather than anything new.
    let written = compile(declared, &schema, "spark").expect("declared is writable").text;
    let duck = compile(declared, &schema, "sql").expect("declared is writable").text;
    // `raise_error(` contains `error(`, so the two are told apart by the word
    // each engine actually uses rather than by a substring of one of them.
    assert!(written.contains("raise_error("), "Spark spells this `raise_error`: {written}");
    assert!(duck.contains("error(") && !duck.contains("raise_error("), "DuckDB spells this `error`: {duck}");
}

/// No verb may write a double-quoted identifier into Spark's dialect.
///
/// **This is the cheap half of the guard against the defect that does not
/// fail.** Spark reads `"region"` as the text `'region'` rather than as a
/// column, so a query with one in it parses, runs, returns the right number of
/// rows, and puts the column's own name in every cell of that column. Nothing
/// throws. `parity/spark.py` catches it by comparing tables across two engines,
/// and that needs pyspark and a JVM; this catches the same class in the default
/// suite, needing neither.
///
/// It walks the same sentence-per-verb table `every_verb_parses` does, so a verb
/// added tomorrow is covered here without anyone remembering to come back.
#[test]
fn spark_never_writes_a_double_quoted_identifier() {
    let others = god_core::check::Tables::new([("products", Schema::new([("region", Type::Text)]))]);

    for (verb, sentence) in verb_sentences() {
        let compiled = god_core::compile_tables(sentence, &schema(), &others, "spark")
            .unwrap_or_else(|d| panic!("`{verb}` will not render as Spark: {}", d.message));
        assert!(
            !compiled.text.contains('"'),
            "`{verb}` wrote a double quote into Spark's dialect, where it is a text value \
             rather than a column, so the query would run and answer wrongly:\n{}",
            compiled.text
        );
    }

    // And the same for every function, since a function is where a quote is most
    // likely to be hand-written into a format string.
    for f in vocabulary::FUNCTIONS {
        let sentence = pipeline_using(f);
        let compiled = compile(&sentence, &schema(), "spark")
            .unwrap_or_else(|d| panic!("`{}` will not render as Spark: {}", f.name, d.message));
        assert!(
            !compiled.text.contains('"'),
            "`{}` wrote a double quote into Spark's dialect:\n{}",
            f.name,
            compiled.text
        );
    }
}
