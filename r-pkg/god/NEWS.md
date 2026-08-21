# god 0.2.2

## One word is gone, and two took its place

`to_whole` is refused by name. It was never one operation: rounding down and
rounding up are two questions, and a word that did whichever the sign made it
do answered one of them by accident.

```r
sales |> add(whole = round_below(revenue / 3))
```

`round_above` is the other way. A pipeline that used `to_whole` stops, and the
refusal names both replacements, so it stops loudly rather than quietly
changing its answer.

## The same sentence, the same answer, wherever the holes are

Sorting a column with missing values in it used to leave the answer to
whichever tool was underneath, and they do not agree: some put the absent rows
last, some first, and Spark put them first going up and last coming down.

**Missing values sort last now, in both directions**, everywhere. Say
`missing first` when you want the other way.

```r
sales |> sort(revenue, missing = "first")
```

The same answer reaches whatever reads that order. `rank`, `row_number`,
`previous`, `following`, `running_total`, `latest`, `rolling` and a grouped
`take` all frame the way the sort above them framed, and so do the orderings
god adds for you after a `summarize`, a `drop_duplicates`, a `lengthen` or a
`widen`. If you grouped by a column with holes in it, or ranked one, the answer
from one engine has changed to agree with the others.

## Six new words

`look_up` is the lookup table, for when `when` would be a stack of questions
about one column.

```r
sales |> add(region = look_up(code, "W", "West", "E", "East", otherwise = code))
```

`standard_deviation` is the tenth aggregation. `rolling` is an aggregate over
the last few rows rather than all of them. `latest` is the last value that was
actually there, which is how a column with gaps in it is carried forward.
`remainder` is what is left after a division. And `join_rows` reaches down the
rows of a group and hands back one piece of text, where `join_text` reaches
across the columns of one row.

```r
sales |> sort(product) |> summarize(sold = join_rows(product, ", "), by = region)
```

## One question, asked of many columns

`where` picks columns by the shape of their name or by what they hold, and one
question is then asked of every column that matches.

```r
survey |> summarize(where(endsWith(name, "_score"), average(value)), by = region)
```

A column added next month is included without the line changing.

## Smaller

* `join` can match a key the two tables name differently:
  `join(customers, by = customer_id == id)`, and `matching` takes the same.
* `take` can keep the rows level with the cut, rather than breaking a tie
  arbitrarily: `take(2, ties = TRUE)`.
* `previous` and `following` take how far to look, so `previous(revenue, 2)`
  reaches two rows back.
* `hour` reads the time a column carries, and refuses a plain date rather than
  answering zero for every row.
* In R, attaching dplyr after god no longer takes god's verbs away. `collect`,
  `rename` and `summarize` are generics and god gives each a method, so the
  sentence runs whichever package you attached last. `pick` is the exception,
  because dplyr's is not a generic: write `god::pick`.
* In Python, every refusal now arrives as words. One kind of mistake used to
  surface as the driver's own error under three frames of plumbing.
* `show_as` writes pandas and polars that agree with the engine about
  `to_date`.
* `god_table` reads a local copy before it reaches the network.

## The manual was read against the engine, all fifty-seven pages

Every page was checked against what the engine actually does, and the
corrections applied. None of it was visible to any check: the pages rendered,
the examples ran, and the guards were green throughout. What was wrong was what
the prose claimed. Counts had frozen while the thing counted grew. Sentences
stated universals the engine does not honor. One page sent you to a cluster
without the argument that makes a cluster query correct, and a recipe
recommended a workaround another chapter warns against.

<https://psychometrician.github.io/god-book/>

# god 0.2.1

## A word for the rows at the far end

`take_last` gives the rows at the end of a sort, in the order the `sort`
asked for.

```r
sales |> sort(revenue) |> take_last(3)
```

Sorting the other way and taking the first three reaches the same rows
backwards, which is a different table. `by` works here as it does on
`take`: the last of each group. It always needs a `sort` in front of it,
where a bare `take` does not.

## The refusal the manual teaches now catches everything

Every chapter tells you to write `except GodError` in Python. One kind of
mistake escaped it: passing a whole table where a column belongs was
raising a different class, so the line the manual teaches let it through
and the program stopped. It is a `GodError` now. One idea, one exception
to catch.

The message that arrives is also new. Where a column belongs, passing a
table says so and shows how to name a column of it; passing a list says
to write the columns one at a time; passing a computed value says to make
it a column with `add` first. It used to print the whole table into the
error.

## The manual answers "can it do what I already do?"

A new appendix takes twenty-five everyday tasks and writes each one six
ways: in god, and in dplyr or tidyr, pandas, polars and data.table. Every
one of them runs when the book is built and the tables are compared, so
the page cannot claim an agreement it does not have.

<https://psychometrician.github.io/god-book/appendices/i-the-same-task.html>

## Smaller

* The package now says it needs R 4.1, which is what it has always needed:
  a pipeline is written with `|>`, and that arrived in 4.1. It said 4.0
  before, a version it could never have installed on.
* In R, a column position that names the verb no longer names it twice.

# god 0.2.0

## Three new words

`add_combinations` makes every combination of the values two columns
already hold into a row, which is how the months nobody sold in become
rows you can chart. `join_text` puts values together, where `split_text`
took them apart. `show_steps` draws what a pipeline does to a table, step
by step, before it runs, and draws a sentence that will not check as far
as it checked.

## A worked example on every word

`?keep` in R shows a small table going in and the answer coming out.
Every verb, every function and every grammar word has one, and both
languages show the same example: the same table, the same numbers, in
each language's spelling.

## Smaller

* Three help pages that had never been generated now answer `?`.
* Asking a pipeline a table's question refuses in R rather than answering
  about the pipeline object.
