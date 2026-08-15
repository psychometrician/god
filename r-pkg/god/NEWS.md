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
