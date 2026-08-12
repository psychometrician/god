# r-pkg/god

The R package. It installs as a binary from R-universe, with no toolchain:

```r
install.packages("god",
  repos = c("https://psychometrician.r-universe.dev", "https://cloud.r-project.org"))
```

Installing from the source tarball compiles the engine during installation,
which takes a few seconds and needs [Rust](https://rustup.rs/); an install that
can find no engine and no way to build one refuses, naming every place it
looked, rather than succeeding into a package that cannot answer a pipeline.

Both spellings below are the same sentence, and the package proves it:

```r
sales |>
  keep(region == "West") |>
  summarize(margin = total(margin), by = product)

run(r"(
sales
  then keep where [region] is "West"
  then summarize [margin] as total([margin]) by [product]
)")
```

## What is here

| File | Owns |
|---|---|
| `R/verbs.R` | The fifteen verbs. Each builds a sentence and decides nothing |
| `R/translate.R` | R's expressions into the grammar's |
| `R/run.R` | The text form, finding the engine, and running the query |
| `R/zzz.R` | How a pipeline prints inside a rendered document |
| `NAMESPACE` | Hand maintained, not roxygen generated |
| `configure` | Builds or bundles the engine into `inst/bin/`, and refuses rather than installing a package that cannot run |

## What does not go here

**Any decision at all.** Validation, defaults, coercion and every error message
live in the grammar. This package finds a table in your scope, hands over some
text, runs the query it gets back, and returns a data frame. That is the whole
job.

## Raw strings, so the text is the text

R 4.0 added raw strings, so a pipeline needs no escaping and is byte-for-byte
the same characters as the one you would paste into Python or into a database:

```r
run(r"(sales then keep where [region] is "West")")
```

## Names this package masks

One base name: `sort`. The grammar's `sort` takes the word's everyday meaning,
`base::sort` stays one `base::` away, and loading the package says so once.
The other exports are the grammar's own words and shadow nothing in base R.
