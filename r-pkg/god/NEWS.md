# god 0.2.3

## After a join, say which rows come first

A join hands its rows back in the order the engine matched them, and that
order is not the same on every engine, or in R and in Python. `take 3` after a
join returned three different rows in the two languages. So after a `join`,
`take`, `first`, `last` and `join_rows` are refused until a `sort` says which
rows come first, and the message gives the sort to write:

```r
sales |> join(products, by = product) |> sort(date) |> take(3)
```

A step that sets an order of its own, such as `summarize`, settles it as well.

## A missing value has no rank, and a missing name has no column

`rank` and `running_total` answer missing on a row whose own value is missing,
and no other row changes. Before, the query ranked that row after every value
and repeated the total above it, as if the hole had added 0.

```r
patchy |>
  sort(product) |>
  add(so_far = running_total(revenue), place = rank(descending(revenue)))
```

`widen` refuses a row whose name is missing, where it used to drop that row
without a word. The message gives the two ways out: `drop_missing` the rows,
or `fill_missing` the hole and list what fills it in `giving`.

## One file is one table, in R and in Python

`god_table` reads a file with DuckDB in both languages, so a CSV gives the same
names, values and missing cells whichever language reads it. Before, an empty
cell was `""` in R and missing in Python, `T` was true in one and text in the
other, and R renamed a header such as `order date` to `order.date`. An empty
field and `NA` are now missing, a date stays text until `to_date` reads it, and
a row with the wrong number of fields is refused.

It also says what went wrong in god's own words, gives up after 60 seconds,
and matches a file's name exactly. In Python, `text = "code"` keeps that column
as text, as it always did in R.

## Numbers and names arrive as they were written

* A number in a pipeline keeps every digit. R wrote seven significant digits,
  so `keep(amount >= 123456.78)` filtered on `123456.8`, and Python cut `1e-12`
  to `0.0`. Both now write the number they were given, and Python takes a
  numpy number such as `sales.revenue.max()`.
* `to_text` writes a whole number without a decimal point, however a language
  stored it. `to_text` of 7 was `"7.0"` in R and `"7"` in Python.
* A table with a column called `price (USD, net)` now works. The engine used
  to cut that name in two at its comma. A table with two columns of one name,
  or a column with no name, is refused, because a sentence could not say which.
* In R, a table held in a dotted variable such as `my.data` works. The dot was
  read as a catalog name; R now writes the table as `my_data`.

## In Python

* `first_present` takes a value as its last fallback, as R's always has:
  `first_present(col.revenue, 0)`. Python read `"none"` as a column called
  `none`, and refused `0`.
* `NaN` and `pd.NA` in a sentence are `missing`, as `None` already was, so
  `keep(col.a != np.nan)` works as it does in R.
* A pandas frame is handed to the engine in whichever form it reads faster.
  For plain numbers, and for text held as Python objects, that is several
  times faster than before. The answers are the same.

## What `show_as` prints answers what the query answers, holes and all

The pandas, polars, PySpark and dplyr that `show_as` prints now agree with the
query on a table with missing values, row by row and in order. About sixty
places disagreed, and each is fixed. The printed code keeps a group whose key
is missing, never matches a missing key in a join, keeps no row whose condition
is unknown, answers missing for a total of nothing, and restates the row order
the query settles.

## Smaller

* `widen` without `giving` runs on DuckDB 2.0, released on 2026-10-21, where
  it stopped saying that a step of its own query did not exist.
* An aggregate in an `add` with `by` keeps the order a `sort` above it asked
  for, and `first`, `last` and `join_rows` there read the rows in that order.
  On Spark, `join_rows` and a distinct count in an `add` now run.
* A grouped `take`, `take(n, ties = TRUE)` and `take_last` list what they kept
  in the same order every time.
* A table named in parts, such as `shop.products`, can be the other table in a
  `join`.
* `show_as(..., "pandas")` writes `join_rows` in an `add` with `by`, where it
  stopped with an internal error.

## The manual reads every table from a file

Every example table in the book is now a CSV published beside it, read with
`god_table` and shown on the page that uses it, so `god_table("hourly")` hands
you the table a chapter reasons from. The R examples pipe into `collect` and
`show_as` rather than wrapping the pipeline in them.

<https://psychometrician.github.io/god-book/>

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
