//! What the grammar refuses, and exactly what it says.
//!
//! **Every message here is asserted word for word.** A refusal that is only
//! checked for "an error happened" can be reworded into nonsense, or stop being
//! reachable at all, and nothing notices — the sibling project carried a refusal
//! that nothing had triggered for the life of the project. Asserting the words
//! means the test fails when the message changes, which is the point: changing
//! what the grammar says to a person should take a deliberate edit.
//!
//! Each case is written as the pipeline someone would actually type, so the
//! suite doubles as the list of mistakes the grammar knows how to talk about.

use god_core::{compile, Schema, Type};

fn sales() -> Schema {
    Schema::new([
        ("region", Type::Text),
        ("product", Type::Text),
        ("revenue", Type::Number),
        ("cost", Type::Number),
        ("ordered_on", Type::Date),
    ])
}

/// Compile, expect a refusal, and hand back what it said.
fn refusal(pipeline: &str) -> String {
    match compile(pipeline, &sales(), "sql") {
        Ok(_) => panic!("this was accepted, and should not have been:\n{pipeline}"),
        Err(d) => d.message,
    }
}

fn assert_refused(pipeline: &str, expected: &str) {
    let actual = refusal(pipeline);
    assert_eq!(actual, expected, "\n  pipeline: {pipeline}");
}

/// The vocabulary as a message lists it, walked rather than written out.
///
/// **Three assertions in this file used to hold these lists as literal text**,
/// which is the failure `vocabulary.rs` warns about in its own header: a test
/// that restates the vocabulary stops covering it the moment the vocabulary
/// moves. Renaming `rows()` to `row_count()` broke two of them, and the break
/// carried no information, because nothing was wrong.
///
/// Building the list here keeps the rest of each message asserted word for word,
/// which is this suite's whole point. What must not drift silently is the
/// **wording**; what must not go stale is the **list**. The pair of tests below
/// is what stops this from becoming a tautology: they check the refusal really
/// does name every word there is.
fn verbs() -> String {
    god_core::vocabulary::VERBS.join(", ")
}

fn functions() -> String {
    god_core::vocabulary::FUNCTIONS
        .iter()
        .map(|f| f.name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn an_unknown_verb_names_every_verb_there_is() {
    // Enumerates the table rather than restating it, so a verb added later is
    // covered on the day it is added rather than the day someone remembers to
    // widen a list here.
    let message = refusal(r#"sales then frobnicate [x]"#);
    for verb in god_core::vocabulary::VERBS {
        assert!(message.contains(verb), "the refusal never names `{verb}`:\n  {message}");
    }
}

#[test]
fn an_unknown_function_names_every_function_there_is() {
    let message = refusal(r#"sales then summarize [n] as frobnicate([revenue])"#);
    for function in god_core::vocabulary::FUNCTIONS {
        assert!(
            message.contains(function.name),
            "the refusal never names `{}`:\n  {message}",
            function.name
        );
    }
}

// -- columns ---------------------------------------------------------------

#[test]
fn an_unknown_column_names_the_nearest_one_and_lists_the_rest() {
    assert_refused(
        r#"sales then keep where [reveune] > 100"#,
        "there is no column called `reveune`. Did you mean `revenue`? The table has: region, product, revenue, cost, ordered_on",
    );
}

#[test]
fn an_unknown_column_that_is_nothing_like_a_real_one_offers_no_guess() {
    // A wrong suggestion is worse than none: it sends someone to check a column
    // that was never the one they wanted.
    assert_refused(
        r#"sales then keep where [quarterly_forecast] > 100"#,
        "there is no column called `quarterly_forecast`. The table has: region, product, revenue, cost, ordered_on",
    );
}

#[test]
fn a_column_is_checked_against_the_step_that_reaches_it_not_the_original_table() {
    // `margin` exists by step three, and `cost` is gone by then. Checking against
    // the original table would accept this, run it, and fail in the engine.
    assert_refused(
        r#"sales
             then add [margin] as [revenue] - [cost]
             then pick [product, margin]
             then keep where [cost] > 10"#,
        "there is no column called `cost`. The table has: product, margin",
    );
}

// -- verbs and functions ---------------------------------------------------

#[test]
fn an_unknown_verb_names_the_nearest_one_and_lists_them_all() {
    assert_refused(
        r#"sales then filter where [revenue] > 100"#,
        &format!("`filter` is not one of the verbs. The verbs are: {}", verbs()),
    );
}

#[test]
fn a_misspelled_verb_is_guessed() {
    assert_refused(
        r#"sales then summarise [n] as row_count()"#,
        &format!(
            "`summarise` is not one of the verbs. Did you mean `summarize`? The verbs are: {}",
            verbs()
        ),
    );
}

#[test]
fn an_unknown_function_names_the_nearest_one() {
    assert_refused(
        r#"sales then summarize [n] as sum([revenue])"#,
        &format!(
            "there is no function called `sum`. The grammar has: {}",
            functions()
        ),
    );
}

#[test]
fn a_function_given_the_wrong_number_of_columns_says_how_many_it_takes() {
    assert_refused(
        r#"sales then summarize [n] as row_count([revenue])"#,
        "`row_count` takes no columns, and 1 was written",
    );
}

// -- the shapes people actually reach for ----------------------------------

#[test]
fn filtering_on_a_group_is_answered_with_the_sentence_that_works() {
    // The HAVING case. The grammar does not have a second verb for it; it has an
    // order of steps, and the message is where that gets taught.
    assert_refused(
        r#"sales then keep where total([revenue]) > 100"#,
        "`keep` decides one row at a time, so it cannot ask a question about a whole group. Summarize first, then keep: `then summarize [n] as row_count() by [g] then keep where [n] > 5`",
    );
}

#[test]
fn a_summarized_value_that_does_not_span_the_group_is_refused_with_the_fix() {
    assert_refused(
        r#"sales then summarize [r] as [revenue] by [product]"#,
        "`summarize` returns one row for each group, so `[r]` has to be a value that spans the group. Wrap it: `total(...)`, `average(...)`, `first(...)`, or count the rows with `row_count()`",
    );
}

#[test]
fn keeping_on_something_that_is_not_a_question_says_what_to_do() {
    assert_refused(
        r#"sales then keep where [revenue]"#,
        "`keep where` needs a question that is either yes or no, and this is a number. Compare it to something: `is`, `>`, `<`, or `in {...}`",
    );
}

#[test]
fn totalling_text_is_refused() {
    assert_refused(
        r#"sales then summarize [r] as total([region])"#,
        "`total` works on numbers, and this column is text. Count the rows instead with `row_count()`, or convert the column first",
    );
}

#[test]
fn comparing_two_different_kinds_of_thing_is_refused() {
    assert_refused(
        r#"sales then keep where [region] is 3"#,
        "this compares text with a number, which can never match. Convert one of them first",
    );
}

#[test]
fn an_aggregate_inside_an_aggregate_is_refused() {
    assert_refused(
        r#"sales then summarize [r] as total(average([revenue]))"#,
        "`total` is already asking about a whole group, so it cannot hold another value that does. Use the column itself: `total([column])`",
    );
}

#[test]
fn a_column_made_in_a_step_is_not_visible_to_the_rest_of_that_step() {
    // dplyr's `mutate` and pandas' `assign` both allow this, so it arrives as an
    // expectation rather than as a mistake. The general "no such column" message
    // would send someone hunting for a typo in a name they can see themselves
    // writing one clause to the left.
    assert_refused(
        r#"sales then add [margin] as [revenue] - [cost], [doubled] as [margin] * 2"#,
        "`[margin]` is made by this same `add`, so it is not on the table yet. Every value in one step is worked out from the table as it arrives. Make it in a step of its own: `then add [margin] as ... then add ...`",
    );
}

#[test]
fn summarize_says_the_same_thing_in_its_own_words() {
    assert_refused(
        r#"sales then summarize [t] as total([revenue]), [half] as [t] / 2 by [product]"#,
        "`[t]` is made by this same `summarize`, so it is not on the table yet. Every value in one step is worked out from the table as it arrives. Make it in a step of its own: `then summarize [t] as ... then summarize ...`",
    );
}

#[test]
fn replacing_a_column_reads_its_old_value_and_is_not_that_case() {
    // `add [revenue] as [revenue] * 2` names a column the table already has, so
    // the name resolves to the arriving value. Guarding against the rule above
    // catching this, which would break the ordinary case to fix the rare one.
    let schema = sales();
    assert!(compile(r#"sales then add [revenue] as [revenue] * 2"#, &schema, "sql").is_ok());
}

#[test]
fn making_the_same_column_twice_in_one_step_is_refused() {
    assert_refused(
        r#"sales then add [x] as [revenue], [x] as [cost]"#,
        "`[x]` is made twice in one step, so one of the two would be thrown away. Give them different names, or make the second one in a step of its own",
    );
}

// -- host habits the grammar does not share --------------------------------

#[test]
fn equality_is_a_word_not_a_symbol() {
    assert_refused(
        r#"sales then keep where [region] = "West""#,
        "the grammar writes equality as the word `is`, so that one spelling works in every language. Write `is` instead of `=`",
    );
}

#[test]
fn negation_is_a_word_not_a_symbol() {
    assert_refused(
        r#"sales then keep where [region] != "West""#,
        "the grammar writes negation as the word `not`. Write `is not` instead of `!=`, and `not` instead of `!`",
    );
}

#[test]
fn and_is_a_word_not_an_ampersand() {
    assert_refused(
        r#"sales then keep where [revenue] > 1 & [cost] > 1"#,
        "the grammar writes this as the word `and`, so that one spelling works in every language",
    );
}

#[test]
fn sort_does_not_take_the_word_by() {
    // `by` names the columns that say which rows go together. A sort key says
    // nothing about which rows correspond, so it does not get the word.
    assert_refused(
        r#"sales then sort by [revenue]"#,
        "`sort` does not take the word `by`. Write `sort [column]`, and `descending` after it to run the other way",
    );
}

#[test]
fn there_is_no_word_for_the_default_direction() {
    assert_refused(
        r#"sales then sort [revenue] ascending"#,
        "there is no word `ascending`, because ascending is what `sort` does when nothing is asked of it. Write `sort [column]`",
    );
}

// -- one spelling, and the second one names the first ----------------------
//
// Every case here is a habit someone arrives with from SQL, R or Python. None of
// them is *accepted* — two ways to write one thing is what the grammar refuses —
// but a refusal that only says "no" leaves a person guessing. Each of these names
// the word the grammar takes.

#[test]
fn a_text_value_is_double_quoted_and_only_double_quoted() {
    assert_refused(
        r#"sales then keep where [region] is 'West'"#,
        "the grammar writes a text value with double quotes, and only double quotes, so there is one spelling rather than two. Write `\"West\"`",
    );
}

#[test]
fn sqls_not_equal_names_the_words_that_replace_it() {
    assert_refused(
        r#"sales then keep where [revenue] <> 1"#,
        "the grammar writes this as the words `is not`, so that one spelling works in every language. Write `is not` instead of `<>`",
    );
}

#[test]
fn a_shouted_grammar_word_is_told_it_is_lowercase() {
    // SQL is written in capitals by habit, so `IS` and `AND` arrive often.
    assert_refused(
        r#"sales then keep where [revenue] IS 1"#,
        "the grammar's words are lowercase. Write `is` instead of `IS`",
    );
}

#[test]
fn each_hosts_spelling_of_truth_names_the_grammars() {
    for (theirs, ours) in [("TRUE", "yes"), ("True", "yes"), ("FALSE", "no"), ("F", "no")] {
        assert_refused(
            &format!("sales then keep where [revenue] is {theirs}"),
            &format!("the grammar writes this as `{ours}`, so there is one spelling rather than one per language. Write `{ours}` instead of `{theirs}`"),
        );
    }
}

#[test]
fn each_hosts_spelling_of_the_absent_value_names_the_grammars() {
    // R writes NA, Python writes None, SQL writes NULL. The grammar writes one.
    for theirs in ["NULL", "NA", "None", "nan"] {
        assert_refused(
            &format!("sales then keep where [revenue] is {theirs}"),
            &format!("the grammar writes this as `missing`, so there is one spelling rather than one per language. Write `missing` instead of `{theirs}`"),
        );
    }
}

#[test]
fn sort_keys_take_their_own_brackets_and_the_message_shows_the_form() {
    // Each key can have its own direction, so they cannot share one bracket —
    // and saying only that they cannot leaves the reader to guess the shape.
    assert_refused(
        r#"sales then sort [revenue, cost]"#,
        "each key gets its own brackets here, so that each can have its own direction. Write `[revenue], [cost]`",
    );
}

#[test]
fn pick_takes_one_bracket_and_the_message_shows_the_form() {
    // This used to blame a missing `then`, which sent the reader to the wrong
    // end of the line entirely.
    assert_refused(
        r#"sales then pick [revenue], [cost]"#,
        "`pick` takes one list of columns in one bracket. Write `pick [revenue, …]`",
    );
}

// -- the parser ------------------------------------------------------------

#[test]
fn the_flow_word_cannot_appear_inside_a_value() {
    // The rule that cost the most to learn: a word doing structural work does
    // that work and nothing else. A conditional phrased with `then` tore itself
    // in half.
    assert_refused(
        r#"sales then add [grade] as then"#,
        "`then` separates steps and cannot appear inside a value. Something is missing before it",
    );
}

#[test]
fn a_bare_word_where_a_value_belongs_says_how_to_write_both_kinds() {
    assert_refused(
        r#"sales then keep where region is "West""#,
        "`region` is a bare word where a value belongs. A column is written in brackets, as `[region]`, and a function is written with parentheses.",
    );
}

#[test]
fn an_unclosed_column_bracket_is_named() {
    assert_refused(
        r#"sales then keep where [region is "West""#,
        "this column bracket is never closed. Add a `]` after the column name",
    );
}

#[test]
fn an_unclosed_text_value_is_named() {
    assert_refused(
        r#"sales then keep where [region] is "West"#,
        "this text value is never closed. Add a `\"` at the end of it",
    );
}

#[test]
fn an_empty_column_bracket_is_refused() {
    assert_refused(
        r#"sales then pick []"#,
        "a column bracket is empty. Write the column's name between the brackets",
    );
}

#[test]
fn a_pipeline_has_to_start_with_a_table() {
    assert_refused(
        r#"keep where [region] is "West""#,
        "steps are joined by the word `then`, and one is missing here",
    );
}

#[test]
fn an_empty_set_can_never_match() {
    assert_refused(
        r#"sales then keep where [region] in {}"#,
        "this set has no values in it, so nothing could ever match it. Write the values between the braces: `in {\"West\", \"East\"}`",
    );
}

#[test]
fn a_set_holding_the_wrong_kind_of_value_is_refused() {
    assert_refused(
        r#"sales then keep where [region] in {1, 2}"#,
        "this set holds a number while the column is text, so nothing in it could ever match. Write the values as text instead",
    );
}

#[test]
fn choosing_no_columns_at_all_is_refused() {
    assert_refused(
        r#"sales then pick all_but [region, product, revenue, cost, ordered_on]"#,
        "this would leave the table with no columns at all. Keep at least one: `pick [a]`, or name fewer columns to drop",
    );
}

// -- the message a person actually sees ------------------------------------

#[test]
fn a_refusal_puts_a_caret_under_the_word_that_caused_it() {
    let pipeline = "sales\n  then keep where [reveune] > 100";
    let Err(d) = compile(pipeline, &sales(), "sql") else {
        panic!("this was accepted");
    };
    // The caret sits under `reveune` — column 19 of the second line, which is
    // where `[` ends — and is as wide as the name. Written as a repeat count
    // rather than as typed spaces, because a hand-counted column is a test that
    // fails for the wrong reason.
    let expected = format!(
        "illegal: there is no column called `reveune`. Did you mean `revenue`? \
         The table has: region, product, revenue, cost, ordered_on\n\
         \x20 |\n\
         2 |   then keep where [reveune] > 100\n\
         \x20 | {}{}",
        " ".repeat(19),
        "^".repeat("reveune".len())
    );
    assert_eq!(d.render(pipeline), expected);
}

// -- the guard has to be able to fail --------------------------------------

/// Proof that the assertions above measure the words rather than merely running.
#[test]
#[should_panic(expected = "assertion")]
fn asserting_the_wrong_message_fails() {
    assert_refused(
        r#"sales then keep where [reveune] > 100"#,
        "some message this grammar has never produced",
    );
}

/// Proof that a pipeline the grammar accepts is not reported as refused.
#[test]
#[should_panic(expected = "this was accepted")]
fn a_legal_pipeline_is_not_counted_as_a_refusal() {
    refusal(r#"sales then keep where [revenue] > 100"#);
}

#[test]
fn the_old_word_for_all_but_names_the_new_one() {
    // `except` was the grammar's own spelling until 2026-08-07, and it is also
    // what SQL and dplyr habits reach for. Anyone who learned it, or who guesses
    // from another tool, gets told the one word rather than a generic complaint
    // about brackets.
    for habit in ["except", "exclude", "excluding", "drop", "omit", "without"] {
        let message = refusal(&format!("sales then pick {habit} [cost]"));
        assert!(
            message.contains("`all_but`"),
            "`{habit}` should name the word that works: {message}"
        );
    }
}

// -- choosing a column by the shape of its name ----------------------------

#[test]
fn name_is_only_a_word_where_a_name_is_being_asked_about() {
    // Everywhere else a column is `[name]`, brackets and all, which is what lets
    // a table have a column actually called `name`.
    let message = refusal(r#"sales then keep where name starts "q""#);
    assert!(message.contains("`pick where` is the one place"), "{message}");
    assert!(message.contains("[name]"), "it names the bracket form: {message}");
}

#[test]
fn a_pattern_that_matches_nothing_is_refused_rather_than_emptying_the_table() {
    let message = refusal(r#"sales then pick where name starts "zzz""#);
    assert!(message.contains("no column's name matches"), "{message}");
    // And it lists what there was, so the reader can see the near miss.
    assert!(message.contains("region"), "{message}");
}

#[test]
fn pick_where_asks_about_a_name_and_says_so_when_given_a_value() {
    let message = refusal(r#"sales then pick where [region] starts "W""#);
    assert!(message.contains("the thing being tested is `name`"), "{message}");
}

#[test]
fn a_text_test_needs_text_on_both_sides() {
    let message = refusal(r#"sales then keep where [revenue] starts "1""#);
    assert!(message.contains("compares text with text"), "{message}");
    assert!(message.contains("to_text"), "it names the conversion: {message}");
}

// -- one value applied to many columns -------------------------------------

#[test]
fn value_is_only_a_word_where_a_column_is_being_worked_on() {
    let message = refusal(r#"sales then add [x] as value * 2"#);
    assert!(message.contains("`add where` and `summarize where`"), "{message}");
}

#[test]
fn an_across_that_matches_nothing_is_refused() {
    let message = refusal(r#"sales then add where name starts "zzz" as value * 2"#);
    assert!(message.contains("no column's name matches"), "{message}");
}

#[test]
fn the_expansion_is_checked_by_the_rules_that_check_a_written_out_value() {
    // **The point of expanding before checking**: nothing here knows about
    // patterns. `summarize` refuses a value that does not collapse a group, and
    // it names the column it refused, which it could only do after expansion.
    // `revenue` is the only number whose name ends in `nue`, so the pattern
    // picks out one column and the refusal below is the summarize rule rather
    // than a type rule tripping first.
    let message = refusal(r#"sales then summarize where name ends "nue" as value * 2"#);
    assert!(message.contains("[revenue]"), "it names the column: {message}");
    assert!(message.contains("spans the group"), "{message}");

    // And the type rule applies the same way.
    let typed = refusal(r#"sales then add where name is "region" as value * 2"#);
    assert!(typed.contains("works on numbers"), "{typed}");
}

#[test]
fn across_says_what_to_make_of_each_column() {
    let message = refusal(r#"sales then add where name starts "re""#);
    assert!(message.contains("write `as`"), "{message}");
    assert!(message.contains("`value` standing for the column"), "{message}");
}

// -- choosing a column by what it holds ------------------------------------

#[test]
fn a_kind_the_grammar_does_not_have_lists_the_ones_it_does() {
    let message = refusal(r#"sales then pick where kind is "numeric""#);
    assert!(message.contains("`text`"), "{message}");
    assert!(message.contains("`number`"), "{message}");
}

#[test]
fn kind_is_only_a_word_where_columns_are_being_chosen() {
    let message = refusal(r#"sales then keep where kind is "number""#);
    assert!(message.contains("what a column holds"), "{message}");
}

// -- making the absent combinations appear ---------------------------------

/// **One column has no combinations to make**, whatever else the sentence says,
/// so this is a refusal rather than a step that quietly does nothing.
#[test]
fn one_column_cannot_be_crossed_with_itself() {
    let message = refusal("sales then add_combinations [region]");
    assert!(message.contains("two columns or more"), "{message}");
    assert!(
        message.contains("the table would come back unchanged"),
        "the message should say what the sentence would actually do: {message}"
    );
    assert!(
        message.contains("[region, ...]"),
        "the repair should be written with the column they already named: {message}"
    );
}

/// A column cannot be crossed and held fixed at once, and the message says which
/// of the two to take it out of rather than only that it cannot be both.
#[test]
fn a_column_is_crossed_or_held_fixed_and_not_both() {
    let message = refusal("sales then add_combinations [region, product] by [region]");
    assert!(message.contains("crossed and held fixed"), "{message}");
    assert!(message.contains("Take it out of `by`"), "{message}");
}

/// **The neighbour is the mistake worth catching.** `add_rows` takes a table and
/// this takes columns; the two words start alike, and left to the general
/// message a reader would be told a word was unexpected rather than that they
/// had reached for the other verb.
#[test]
fn a_table_here_points_at_the_verb_that_takes_one() {
    let message = refusal("sales then add_combinations products");
    assert!(message.contains("takes columns rather than a table"), "{message}");
    assert!(message.contains("add_rows products"), "{message}");
}

#[test]
fn a_column_named_twice_is_refused_in_either_list() {
    assert!(refusal("sales then add_combinations [region, region]")
        .contains("named twice in the columns being crossed"));
    assert!(refusal("sales then add_combinations [region, product] by [cost, cost]")
        .contains("named twice in `by`"));
}
