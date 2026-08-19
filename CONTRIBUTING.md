# Contributing to god

The grammar parses, checks and compiles; the command line, both launchers, the
live manual and the parity harness all exist and run. This document is how the
pieces fit together, and the commands below are the proof to run.

---

## The `§N` citations

Comments in this repository cite section numbers, like `§7` or `§14`. They point
into the project's design document, which records each decision with the reason
it was made. **The comment always carries its meaning by itself** — the citation
is provenance, not a link you need to follow, and no comment depends on your
being able to read the section it names.

If you want the reasoning behind a decision and the comment does not give enough
of it, that is a defect in the comment. Open an issue and it gets rewritten.

## The shape of the thing

```
              ONE TEXT
                 │
            ONE PARSER  →  ONE PLAN  →  ONE CHECKER
                                             │
                                  ┌──────────┴──────────┐
                               EXECUTE                PRINT
                               an engine              dplyr · pandas · SQL
```

| Path | Owns | |
|---|---|---|
| `god-core/src/parse.rs` | Text to a plan | ✅ |
| `god-core/src/plan.rs` | What a pipeline means, and where each piece was written | ✅ |
| `god-core/src/check.rs` | Validation and diagnostics. The largest file | ✅ |
| `god-core/src/vocabulary.rs` | Every word, in one place. Tests enumerate it | ✅ |
| `god-core/src/backend/` | A plan to text. One module per target | ✅ |
| `god-cli/src/main.rs` | Text on stdin, backend text on stdout | ✅ |
| `r-pkg/god/R/` · `py-pkg/god/god/` | Carrying text in and a table back | ✅ |
| `book/` | The manual, where every example executes | ✅ |

Inside the R package, which is the one with files a reader has to tell apart:

| File | Owns |
|---|---|
| `R/verbs.R` | The sixteen verbs. Each builds a sentence and decides nothing |
| `R/translate.R` | R's expressions into the grammar's |
| `R/run.R` | The text form, finding the engine, and running the query |
| `R/zzz.R` | How a pipeline prints inside a rendered document |
| `NAMESPACE` | Hand maintained, not roxygen generated |
| `man/` | roxygen's output. Regenerate it; see below |
| `configure` | Builds or bundles the engine into `inst/bin/`, and refuses rather than installing a package that cannot run |

**What does not go in either binding: any decision at all.** Validation,
defaults, coercion and every error message live in the grammar. A binding finds
a table in your scope, hands over some text, runs the query it gets back, and
returns a frame. That is the whole job.

**That table used to live in `r-pkg/god/README.md`, and it was the wrong
place.** R-universe renders a package's README as its front page, so what a
stranger deciding whether to install god actually read was a note to
contributors about which file owns what — titled after a directory,
`r-pkg/god`, and never saying what god is. Both package READMEs are written for
a user now and follow one shape.

**`god-core` has no runtime dependencies, and keeping it that way is a rule.**
Turning text into a plan and a plan into text needs no library. Only the test
suite links an engine, because tests must read rows rather than inspect queries.
If a change wants a dependency, that is a design conversation, not a commit.

## The three rules most likely to be broken by accident

**1. No decision in a host.** Validation, defaults, coercion and every
diagnostic live in `god-core`. A host carries text in and a table back, and
decides nothing. There is one parser and one checker, so there is nothing for two
implementations to disagree about — which is a property to protect rather than
one to assume.

**2. One door.** The core exposes `compile(text, schema, backend)`. Nothing
reaches past it. Two paths drift, and the drift is invisible until someone
compares results on an edge case a year later.

**3. The gate runs before anything else.** Every refusal lives in `check.rs`,
upstream of every backend, and walks the whole plan before a query exists. A
check that runs after the point of no return can only warn, and a warning
followed by a wrong answer is worse than no check.

## A word is not added until every backend can write it

A function that the grammar has and a backend cannot spell is a hole, and the
test that catches it runs each function through each backend for real rather than
comparing two lists.

**Done** means all of:

- the word, in `vocabulary.rs`,
- the parser and the checker,
- a spelling in **every** backend,
- a test that reads the rows, not the query,
- a refusal test, if the word can be misused,
- the book, with a live example.

### Guards are broken on purpose

**A guard whose failure has never been observed is one nobody should trust.**
Before relying on a new check, break the thing it guards, watch it fail, put it
back, and say in the commit that you did.

This is not ceremony. Four guards were broken this way on the day the grammar was
written and one of them did not fire: the round-trip test compared a printed
pipeline against itself printed twice, so a printer that dropped a clause on
every pass dropped it on both and the strings agreed perfectly. It now compares
plans. The useful question is never *did it pass* — it is **could this have
failed?**

## Adding a verb or an expression

Answer three questions in writing before writing code:

1. **Does it derive?** Can the vocabulary already say this with another verb plus
   an argument? If yes, it is a shorthand, and a shorthand needs a stated reason
   rather than a preference.
2. **Is it closed?** A verb takes a table and returns a table. An expression
   takes columns and returns a column. Anything that is neither is not either,
   and it probably belongs to the host language.
3. **Does the name hold up?** Everyday American English, two words maximum joined
   by `_`, no acronyms. If it changes the table's shape, the direction has to be
   in the name. This is why reshaping is `lengthen` and `widen`: the direction
   is the word, where nobody could remember which of `melt` and `cast` made
   data taller.

### R's help page is generated, and nothing in a source checkout needs it

**`r-pkg/god/man/` is roxygen2's output, so writing the comment is not writing
the page.** `NAMESPACE` is hand-maintained here, which means adding a verb takes
an edit there and a *regeneration* of `man/`, and the second step is the one
that gets skipped:

```bash
Rscript -e 'roxygen2::roxygenise("r-pkg/god", roclets = "rd")'
```

`roclets = "rd"` is not optional. Without it roxygen2 also wants to write
`NAMESPACE`, which this package keeps by hand and which says so at the top.

**Nothing in a source tree can miss the omission**, which is why it happened
three times before anything caught it. `pkgload::load_all` needs no `.Rd` at
all, so the suites, the book and every parity harness run green over a verb
`?name` cannot find on an installed copy. The R suite now checks every exported
name against `man/`, in both directions.

## Diagnostics

Three kinds, and the difference matters to a caller:

| Kind | Means | Behavior |
|---|---|---|
| **Illegal** | The sentence cannot mean anything | Fatal. Nothing runs |
| **Unsupported** | Legal, and not built yet | Fatal, and says so |
| **Assumption** | god chose something you did not say | Warning naming the choice |

**Never accept an argument and silently drop it.** This is the single most common
way a data tool loses a person's trust: the pipeline ran, the number came out,
and one clause did nothing.

**A message says what to do**, not only what went wrong. Not
`unknown column 'reveune'` but `unknown column 'reveune'. Did you mean
'revenue'? The table has: revenue, cost, region, product.`

## The book

Anything that changes what a pipeline returns goes in `book/` as a **live**
chunk. A fenced block that does not execute shows syntax without proving it
works, and it is how a manual ends up documenting verbs the engine does not have.

Write the chapter while the feature is still fresh. It is the cheapest time, and
it is the point where the prose and the results check each other: the chunk
proves the code, and writing the explanation is what finds the case nobody
considered. A chapter written months later is written by someone who no longer
remembers why.

Then check what the change made *wrong*. A new verb quietly invalidates the
chapter that lists the vocabulary, the chapter that counts it, and the refusals
table. The damage lands in the siblings' chapters, not in the new verb's own
section.

Some things cannot be live chunks: the text form has no chunk engine yet, and a
shell transcript is not code the page can run. Those are checked instead by
`book/check_grammar.R`, which hands every pipeline the book shows to the engine
and every command line to `--help`. It is what the preface needed and did not
have; it had been showing `is 'West'`, which the grammar refuses, and a
`god show` subcommand that never existed.

## Building

```bash
cargo build --release                     # explicitly; never inferred from a test
cargo test --release                      # the suite. It links a real engine

Rscript r-pkg/god/tests/test_basic.R      # the R package, and it runs the book guards
python3 py-pkg/god/tests/test_basic.py    # the Python package, and it runs the harnesses below

python3 parity/check.py                   # the two languages against each other
python3 parity/vocabulary.py              # both bindings against the engine's own word list
python3 parity/spark.py                   # the corpus on DuckDB and Spark, tables compared
python3 parity/warehouse.py               # the corpus on DuckDB and a SQL warehouse, tables compared
python3 parity/printed.py                 # the printed code executed, tables compared

Rscript book/check_grammar.R              # every pipeline and transcript the book shows, run
Rscript book/check_prose.R                # the book's voice, the half a machine can hold
Rscript book/check_vocabulary.R           # every word demonstrated by a chunk, and named in the README
Rscript book/check_counts.R               # every prose count of a thing the engine can count
Rscript book/check_refusals.R             # every refusal chunk actually refuses (the R half)
python3 book/check_refusals.py            # the same promise, kept by the Python half
Rscript book/check_tabs.R                 # every tabset holds both languages
Rscript book/check_template.R             # the verb-chapter shape, and every page ends on prose
Rscript book/check_promises.R             # the five rules the preface states
Rscript book/check_render.R               # the rendered book, for the defects only output has
python3 book/readability.py               # a report, not a gate

cd book && quarto render --to html
```

The book has chapters in both languages, reached through reticulate — no
Jupyter kernel is involved, so neither `jupyter` nor `QUARTO_PYTHON` is needed.
One venv, once:

```bash
python3 -m venv book/.venv
book/.venv/bin/pip install pandas duckdb gog jinja2
```

It is gitignored and never shipped. `gog` is for the closing chapter, which
draws its plots; `jinja2` is what pandas' `.style` needs in two chunks, and
without it the render dies at chapter 1.

All of these run from the repository root, and none of them needs the engine's
location given to it: both packages find `target/release/god-cli` by walking
up, so `cargo build --release` really is the only setup step. `GOD_CLI`
outranks every other way the engine is found, in both languages.

`parity/check.py` is the one that matters. It reads one corpus written three
ways — as text, as R, as Python — and checks four things for each sentence:
the two languages produce the same query, they produce the same table, and
each native spelling builds the very sentence the text form parses. A binding
that has drifted shows up as a disagreement rather than as a small debt.

`cargo test` passing does not mean `target/release/god-cli` was rebuilt. They are
separate artifacts. Run `cargo build --release` explicitly, and re-render the
**whole** book after a change to the grammar: every table in it is computed by
that binary, so one change invalidates every page at once, while Quarto tracks
`.qmd` dependencies and will happily leave the others stale.

`quarto render` exits 0 even with broken links, and it emits `WARN:` rather than
`warning:`. Grep the output for `-inE "warn|error|unable|cannot|fail|not found"`.
A clean exit code proves nothing.

## Commits

- Say what changed and why, not which files.
- Stage explicit paths. Never `git add -A` or `git add .`, and never `git stash`.
  A sweep is how half-finished work gets committed by someone who never saw it.
- Run `git diff <file>` before staging it. One file can hold two people's work.
- No `Co-Authored-By:` trailer.
- Line endings are LF, enforced by `.gitattributes`.

## License

By contributing you agree that your contribution is licensed under Apache 2.0,
the license this project ships under.
