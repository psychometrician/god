//! The ladder drawing: that it says the same thing twice, and that it cannot
//! quietly leave something out.
//!
//! **A drawing is the hardest thing in this tree to hold a test to.** Prose
//! renders cleanly whatever it claims and so does a picture: a ladder missing a
//! band looks exactly as finished as one that has them all. So the checks here
//! are the two kinds that can fail — the ones that enumerate something the
//! grammar already owns, and a golden file a person reads.

use god_core::check::{Schema, Tables, Type};
use god_core::draw::scene::Ink;
use god_core::{draw, parse};

/// The corpus's tables, which are the ones `parity/sales.csv` and
/// `parity/products.csv` hold. Kept here rather than read from the CSVs because
/// a type is what the drawing shows and a CSV does not carry one.
fn sales() -> Schema {
    Schema::new([
        ("region", Type::Text),
        ("product", Type::Text),
        ("revenue", Type::Number),
        ("cost", Type::Number),
        ("ordered_on", Type::Date),
    ])
}

/// `products` is the corpus's second table and `regions` its third, the one
/// whose key `sales` calls something else — `area` here against `region` there,
/// which is what the corpus's `by [region] is [area]` sentences need.
///
/// **A table missing from here does not fail; it draws as a refusal**, and the
/// golden would record that refusal as if it were the expected picture. So this
/// list has to grow with the corpus, and the way you find out it did not is by
/// reading the blessed diff rather than by a test going red.
///
/// `archive` is this file's own, and it exists because `add_rows` needs both
/// tables to hold the same columns — which `products` deliberately does not, so
/// no corpus sentence can stack it. Named by nothing in the corpus, so it
/// changes no drawing there.
fn others() -> Tables {
    Tables::new([
        ("products", Schema::new([("product", Type::Text), ("maker", Type::Text)])),
        ("regions", Schema::new([("area", Type::Text), ("manager", Type::Text)])),
        ("archive", sales()),
    ])
}

fn ladder(sentence: &str) -> String {
    let plan = parse::parse(sentence)
        .unwrap_or_else(|d| panic!("`{sentence}` will not parse: {}", d.message));
    draw::ladder(&plan, sentence, &sales(), &others())
}

fn picture(sentence: &str) -> String {
    let plan = parse::parse(sentence).expect("parses");
    draw::picture(&plan, sentence, &sales(), &others())
}

fn scene(sentence: &str) -> draw::Scene {
    let plan = parse::parse(sentence).expect("parses");
    draw::ladder::build(&plan, sentence, &sales(), &others())
}

const CORPUS: &str = include_str!("../../parity/corpus.god");
const GOLDEN: &str = include_str!("ladder.golden");

fn corpus() -> Vec<&'static str> {
    CORPUS.split("\n---\n").map(str::trim).filter(|s| !s.is_empty()).collect()
}

/// **The same sentence draws the same picture, every time.**
///
/// The way generated output stops being deterministic is a container whose
/// iteration order is not written down, and in Rust that is `HashMap`: two of
/// them in one process are seeded differently, so drawing twice and comparing is
/// enough to catch one that got into the layout. Nothing in the drawing path
/// uses one today, and this is what says so tomorrow.
#[test]
fn the_same_sentence_draws_the_same_picture() {
    for sentence in corpus() {
        assert_eq!(
            ladder(sentence),
            ladder(sentence),
            "`{sentence}` drew two different pictures in one process"
        );
    }
}

/// **Every table a sentence reads is drawn.**
///
/// This is the check that earns its place. A second table can be named by a step
/// — `join`, `add_rows` — or from inside a condition, by `matching`, and the
/// second kind is the one a drawing forgets: nothing in the step says a table is
/// there. A ladder missing it is not obviously wrong, which is precisely why a
/// person reading the picture would never catch it.
///
/// The grammar already answers which tables a sentence reads, so this asks it
/// rather than keeping a list.
///
/// **It asks for the table's own row, not for its name.** The first version of
/// this looked for the name anywhere in the drawing and could never have failed:
/// the step's own words hold it, so `keep where not matching(products, ...)`
/// satisfied a search for `products` with the arriving table entirely missing.
/// Broken on purpose to find that out.
#[test]
fn every_table_a_sentence_reads_is_drawn() {
    let sentences = [
        r#"sales then join products by [product]"#,
        r#"sales then keep where matching(products, by [product])"#,
        r#"sales then keep where not matching(products, by [product])"#,
        r#"sales then add_rows archive"#,
    ];

    for sentence in sentences {
        let plan = parse::parse(sentence).expect("parses");
        let drawn = ladder(sentence);
        let named = plan.tables();

        let (head, arriving) = named.split_first().expect("a pipeline reads at least one table");
        assert!(
            drawn.lines().next().is_some_and(|first| first.starts_with(head.as_str())),
            "`{sentence}` starts from `{head}` and the drawing does not open on it:\n{drawn}"
        );

        for table in arriving {
            let elbow = format!("└ {table}");
            assert!(
                drawn.lines().any(|line| line.trim_start().starts_with(&elbow)),
                "`{sentence}` reads `{table}` and the drawing gives it no row of its own:\n{drawn}"
            );
        }
    }
}

/// A join marks the key on both sides, because it is on both tables and arrives
/// once. Getting this wrong is what a suffixed `region_x` is.
#[test]
fn a_join_marks_the_key_in_both_strips() {
    let drawn = ladder("sales then join products by [product]");
    assert_eq!(
        drawn.matches("=product").count(),
        2,
        "the key should be marked in this table's strip and in the arriving one:\n{drawn}"
    );
    assert!(drawn.contains("+maker"), "the column that crosses should be marked:\n{drawn}");
    assert!(
        drawn.contains("rows may multiply"),
        "a join can multiply rows and the grammar cannot know whether it will:\n{drawn}"
    );
}

/// A key the two tables name differently shows **both** names, because hiding
/// one is hiding the fact a reader opened the drawing to check.
#[test]
fn a_join_on_a_pair_draws_both_names() {
    let drawn = ladder("sales then join regions by [region] is [area]");
    assert!(drawn.contains("matched on region is area"), "{drawn}");
}

/// **The defect this test exists for**, found by reading a blessed golden rather
/// than by anything going red. The line under an arriving table lists what
/// crossed over, and it worked out "what crossed" by removing the keys — matched
/// against the string the drawing *shows*. The moment that string became
/// `region is area`, no name matched, and the drawing announced that `area`
/// crossed over. It does not: it holds the key's own value and is dropped, and
/// the column strip on the same line said so. Two lines of one picture
/// disagreeing is worse than either being wrong alone.
#[test]
fn the_other_tables_key_is_not_listed_as_crossing_over() {
    let drawn = ladder("sales then join regions by [region] is [area]");
    assert!(drawn.contains("manager crosses over"), "{drawn}");
    assert!(
        !drawn.contains("area, manager cross over"),
        "`area` is the key under another name and never arrives:\n{drawn}"
    );
}

/// **The distinction the whole drawing is for.** A filtering join reads a second
/// table, brings nothing back, and cannot multiply rows — which is exactly what
/// an inner join looks like until a key repeats.
#[test]
fn a_filtering_join_brings_nothing_and_never_multiplies() {
    let drawn = ladder("sales then keep where matching(products, by [product])");
    assert!(drawn.contains("no columns cross"), "nothing crosses a `matching`:\n{drawn}");
    assert!(drawn.contains("never more"), "a `matching` cannot add rows:\n{drawn}");
    assert!(
        !drawn.contains("may multiply"),
        "a `matching` cannot multiply rows, and saying it might would teach the mistake this drawing exists to prevent:\n{drawn}"
    );
    assert!(!drawn.contains("+maker"), "no column of the other table crosses:\n{drawn}");
}

/// A column that leaves is marked on the band that takes it away, so the answer
/// to "where did it go" is on the page rather than in a message.
#[test]
fn a_column_is_marked_where_it_leaves() {
    let drawn = ladder("sales then summarize [gross] as total([revenue]) by [region]");
    assert!(drawn.contains("+gross"), "the column it makes:\n{drawn}");
    assert!(drawn.contains("-product"), "a column it takes away:\n{drawn}");
    assert!(drawn.contains("-revenue"), "the column it summarized away:\n{drawn}");
}

/// **A sentence that will not check is still drawn.**
///
/// How far it got is the thing worth knowing, and it is the one thing a refusal
/// on its own cannot say. Here the column is real and was taken away two steps
/// earlier, which is the case where the picture answers a question the message
/// does not.
#[test]
fn a_refused_sentence_is_drawn_as_far_as_it_checked() {
    let drawn = ladder(
        r#"sales then summarize [gross] as total([revenue]) by [region] then keep where [product] is "hat""#,
    );
    assert!(drawn.contains("+gross"), "the step that did check is drawn:\n{drawn}");
    assert!(drawn.contains("-product"), "and it shows where the column went:\n{drawn}");
    assert!(drawn.contains("^^^"), "with a caret under the words that were refused:\n{drawn}");
    assert!(
        drawn.contains("there is no column called `product`"),
        "and the refusal itself:\n{drawn}"
    );
}

/// **Every column the ladder names, the picture draws.**
///
/// The first version of this counted one `<text>` per cell and was exact until
/// the picture started folding a long step's words across lines, at which point
/// the counts diverged for a reason that was not a defect. Chips are atomic and
/// never fold, so asking for the columns is the invariant that survives a
/// layout change: what must not happen is a column going missing from one
/// drawing and not the other.
#[test]
fn every_column_the_ladder_names_the_picture_draws() {
    for sentence in corpus() {
        let drawn = scene(sentence);
        let picture = picture(sentence);
        for row in &drawn.rows {
            for cell in row.cells.iter().filter(|c| {
                matches!(c.ink, Ink::Column | Ink::Added | Ink::Dropped | Ink::Key)
            }) {
                assert!(
                    picture.contains(&cell.text),
                    "`{sentence}` names `{}` in the ladder and the picture does not draw it",
                    cell.text
                );
            }
        }
    }
}

/// The picture is a whole document and the same one every time.
#[test]
fn the_picture_is_closed_and_repeatable() {
    for sentence in corpus() {
        let drawn = picture(sentence);
        assert!(drawn.starts_with("<svg "), "`{sentence}` did not open a document");
        assert!(drawn.trim_end().ends_with("</svg>"), "`{sentence}` did not close one");
        assert_eq!(
            drawn.matches("<text ").count(),
            drawn.matches("</text>").count(),
            "`{sentence}` left a run of text open"
        );
        assert_eq!(drawn, picture(sentence), "`{sentence}` drew two different pictures");
    }
}

/// The whole corpus, drawn, against a file a person has read.
///
/// **This is the one that makes a layout change reviewable.** A drawing has no
/// natural assertion — nobody can write down in advance what a good picture
/// looks like — so the check is that it has not changed without somebody
/// noticing. The diff is the review.
///
/// `GOD_BLESS=1 cargo test --release ladder_matches` rewrites the file. Read the
/// diff before committing it: that reading is the entire point of the file.
#[test]
fn the_ladder_matches_the_golden() {
    let mut drawn = String::new();
    for sentence in corpus() {
        drawn.push_str(sentence);
        drawn.push('\n');
        drawn.push_str(&ladder(sentence));
        drawn.push_str("---\n");
    }

    if std::env::var("GOD_BLESS").is_ok() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ladder.golden");
        std::fs::write(path, &drawn).expect("rewriting the golden");
        return;
    }

    assert_eq!(
        drawn, GOLDEN,
        "the ladder has changed. Read the difference; if it is the change you meant, \
         run it again with GOD_BLESS=1 to record it"
    );
}
