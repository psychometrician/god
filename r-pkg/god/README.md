# god — a grammar of data, for R

One small vocabulary for manipulating tables, spelled the same way in R, in
Python, and as plain text. A pipeline is checked whole before any of it runs, so
a bad column is reported at the step that names it rather than failing partway
through — or worse, not failing.

```r
library(god)

sales <- read.csv("sales.csv")

sales |>
  keep(region == "West") |>
  summarize(margin = total(margin), by = product)
```

The same sentence runs as plain text, byte-for-byte what you would paste into
Python or into a database:

```r
run(r"(
sales
  then keep where [region] is "West"
  then summarize [margin] as total([margin]) by [product]
)")
```

R 4.0's raw strings are why that needs no escaping: the text is the text.

And when you reach the edge of the vocabulary, it shows you the same pipeline in
a tool you already know — `show_as(pipeline, "dplyr")`, or `"pandas"`,
`"polars"`, `"pyspark"`, `"sql"`, `"spark"`.

## Installing

```r
install.packages("god",
  repos = c("https://psychometrician.r-universe.dev", "https://cloud.r-project.org"))
```

That is a binary, with no toolchain to set up. Installing from the source
tarball compiles the engine during installation, which takes a few seconds and
needs [Rust](https://rustup.rs/); an install that can find no engine and no way
to build one refuses, naming every place it looked, rather than succeeding into
a package that cannot answer a pipeline.

## Every word answers for itself

`?keep`, `?summarize`, `?take_last` — every verb, function and grammar
word has a page with a worked example, a small table going in and the answer
coming out.

## One name this package masks

`sort`. The grammar's `sort` takes the word's everyday meaning, `base::sort`
stays one `base::` away, and loading the package says so once. The other exports
are the grammar's own words and shadow nothing in base R.

The manual, live in both languages, is at
<https://psychometrician.github.io/god-book/>.
