# Changelog

What changed, for someone deciding whether to upgrade. Every entry is something
a person using god can see.

## Unreleased

### `look_up`, the lookup table

Written values become written values, pairs side by side, and the `otherwise`
says where a value with no pair goes:

```r
sales |> add(region = look_up(code, "W", "West", "E", "East", otherwise = code))
```

```python
sales >> add(region=look_up(col.code, "W", "West", "E", "East", otherwise=col.code))
```

The `otherwise` is required. Naming the column keeps unpaired values as they
were, `missing` (`NA` in R, `None` in Python) drops them, and a written value
is a default. Those three endings are the whole difference between the two
functions dplyr gives this idea (`replace_values`, `recode_values`) and the
two polars gives it (`replace`, `replace_strict`), so god writes the ending
rather than naming the word twice — and refuses to guess it, because half the
neighbouring tools assume one way and half the other.

A pair may send a value missing, which is how "this value means absent" is
written: `look_up(x, "", missing, otherwise = x)` turns empty text into a
proper hole. The pairs are written values on both sides; a computed test or a
computed answer is `when`'s job, and the refusals say so. A value looked up
twice is refused, and so is looking up `missing`, with `fill_missing` named
instead.

### `hour` reads a time, and says so when there is none

`hour` used to answer on a column that carries no time. What it answered
depended on where the pipeline ran: **0 when god ran it, and an error when the
same sentence was printed as polars and run there.**

A plain date has no hour in it, so it is now refused, wherever the date came
from — a column your table already had, or one `to_date` made:

```
illegal: `hour` reads the time of day a column carries, and a plain date
carries none, so this would be nought on every row. It wants a column that
arrived carrying one, which `to_date` does not make
```

`hour` still reads any column that arrived carrying a time, which is how it was
always meant to be used:

```r
stamps |> add(h = hour(at))
```

```python
stamps >> add(h = hour(col.at))
```

Where the values are text, the conversion belongs above the pipeline —
`as.POSIXct` in R, `pd.to_datetime` in Python. **`year`, `month`, `day` and
`weekday` are untouched** and still read a date of either sort.

**Nothing else changes.** A date and a date-carrying-a-time compare, sort and
join against each other exactly as before, and `pick where kind is "date"` picks
columns of both, as it always did.

### Printed pandas and polars agree with the engine about `to_date`

Both kept the time where every other target dropped it, so a pipeline printed
to pandas or polars and run there could answer differently from the same
pipeline run by god. They now make a date, as DuckDB, Spark, dplyr and PySpark
always did.

Printed pandas also stopped crashing on a column holding both
`2026-01-02 14:30:00` and `2026-03-15`: `pd.to_datetime` raises on mixed
formats, where the engine parses them without complaint.

### `standard_deviation`, the tenth aggregation

How spread out a column is, in `summarize`, broadcast in `add ... by`, and in
`widen`'s cell. It is the sample deviation, which is what R's `sd`, pandas'
`.std()`, polars' `.std()` and both SQL engines' `stddev` all mean by their
bare word, so the answer does not change with the engine. A group of one row
answers missing, because one value has no spread.

```r
sales |> summarize(spread = standard_deviation(revenue), by = region)
```

```python
sales >> summarize(spread=standard_deviation(col.revenue), by=col.region)
```

It earned its word by having no composition: the variance can be written out,
and the deviation is its square root, which the grammar does not have.
Variance stays a composition, the deviation squared.

### `rolling`, an aggregate of the last few rows

A seven-row average, sliding, is one word wrapping another:

```r
sales |> sort(day) |> add(week = rolling(average(revenue), 7), by = region)
```

```python
sales >> sort(col.day) >> add(week=rolling(average(col.revenue), 7), by=col.region)
```

The aggregate goes inside, one of `total`, `average`, `median`, `smallest`,
`largest` and `standard_deviation`, and the number says how many rows the
window holds, the row itself included. It needs a `sort` before it, like every
window, and `by` restarts it for each group.

A window that is not yet full answers missing, and so does a full window with
a missing value inside it: six values are not the seven-row average. That is
how pandas, polars and data.table answer by default too, so the printed code
and the query agree without a word. `first`, `last`, `row_count` and
`unique_count` are refused inside it, each with the reason and the spelling to
use instead.

### A window inside a function's arguments is refused

`total(rank([x]))` used to check and then die in the engine, and
`round_below(running_total([x]))` quietly lost the order it walked. Both are
now refused up front, with the repair named: make it a column, then use the
column. Windows still compose through arithmetic, comparisons and `when`,
which carry the order down.

### The window-as-filler refusal points at `latest` now

`fill_missing [x] as previous([x])` is still refused, and the message now
names the spelling that fills a whole run of holes, `sort` then
`add [x] as latest([x])`, instead of the one-row workaround it used to
recommend.

### A refusal arrives as words, in Python too

An uncaught refusal used to print three frames of god's own plumbing before the
message, so a sentence refused in Python looked like a defect and the same
sentence refused in R looked like an answer. Now it reads the way R's does, with
your own call at the top:

```
Traceback (most recent call last):
  File "<stdin>", line 1, in <module>
    collect(sales >> keep(col.nope > 1))
god.columns.GodError:
illegal: there is no column called `nope`. The table has: region, revenue
```

### `god_table` reads a local copy before it reaches the network

It now looks for a `data/<name>.csv` in your working directory, or in any
directory above it, and only fetches the published copy if there is none. Keep
a folder of your own and nothing needs a connection; keep none and it works
exactly as before.

```r
sales <- god_table("sales")     # data/sales.csv if you have one
```

```python
sales = god_table("sales")
```

A name is now checked to be a name. `god_table("../somewhere/else")` is refused
with a message saying what a name is, because a name that used to make a bad
web address now also names a file on your disk.

Thirteen more of the book's tables are reachable by name: `contacts`,
`listed`, `mixed`, `notes`, `patchy`, `pupils`, `repeats`, `sensors`,
`sittings`, `sparse`, `stamps`, `stocked` and `twice`. Between them they cover
every chapter fixture the manual computes from, so any example in the book can
now be reproduced with one call.

### One question, asked of many columns at once

`pick`, `add` and `summarize` have always taken a rule instead of a list of
columns. `keep` did not, so asking eight score columns one question meant
writing it eight times joined by `or`. Now it takes the same rule, with `any`
or `every` in front:

```r
survey |> keep(where_any(startsWith(name, "q"), value > 3))
survey |> keep(where_every(startsWith(name, "q"), value > 3))
```

```python
survey >> keep(where_any(name.starts("q"), value > 3))
survey >> keep(where_every(name.starts("q"), value > 3))
```

The text form is `keep where any name starts "q" as value > 3`. A rule that
matches no column is refused rather than answered, because nothing is true of
no columns and everything is, and neither answer is one you could have meant.

### `latest`, for the last value that was there

Fills a hole with the most recent value above it, in the order a `sort` has
declared. `by` restarts it for each group.

```r
readings |> sort(taken_at) |> add(reading = latest(reading))
```

```python
readings >> sort(col.taken_at) >> add(reading = latest(col.reading))
```

**If you were using `first_present(x, previous(x))` for this, it was filling
only one row.** That is what the refusal message and this changelog used to
recommend, and on a run of two holes it left the second one empty without
saying anything. `latest` fills the run. The old advice is corrected
everywhere it appeared.

### `take` can keep the rows level with the cut

`take 3` gives three rows. If the third and fourth are tied on the sort key,
which one survives was the engine's choice. `with ties` keeps both:

```r
scores |> sort(descending(points)) |> take(2, ties = TRUE)
```

```python
scores >> sort(descending(col.points)) >> take(2, ties=True)
```

It needs a `sort` in front of it, and the row count is no longer something you
can read off the sentence, which is why it has to be asked for. Coming from
dplyr, note that `slice_max` keeps ties by default and this does not.

### `remainder`

What is left over after dividing: `remainder(col.n, 3)`. For every third row,
for buckets, for asking whether a number is even.

The answer takes the sign of the divisor, so `remainder(-7, 2)` is 1. That is
R's answer and Python's; SQL engines give -1, and god now gives the same answer
wherever it runs.

### `to_whole` is now `round_below` and `round_above`

`to_whole` is gone, and writing it gets a refusal that names both replacements.

It had two problems and they had the same cause. It gave different answers on
different engines — `to_whole(7.5)` was 8 when the pipeline ran and 7 when the
same pipeline was handed to R or pandas and run there. And nobody could tell
from the name which way it went, because the name did not say.

The cause of both is that it was not a conversion. god has one kind of number,
so `to_whole` turned a number into a number: it was a rounding wearing the
`to_` prefix, and the prefix made it look like a type change nobody needed to
think about.

```r
sales |> add(each = round_below(revenue / 7), pages = round_above(revenue / 7))
```

```python
sales >> add(each = round_below(col.revenue / 7), pages = round_above(col.revenue / 7))
```

`round_below` always goes toward the smaller number and `round_above` toward
the larger, so `round_below(-5.5)` is -6 and `round_above(-5.5)` is -5. A value
that is already whole does not move under either. Both give the same answer on
every engine god runs on or prints to.

They are `below` and `above` rather than down and up because spreadsheets round
"down" toward zero, which would make -5.5 into -5. One word meaning two things
depending on which tool you were last in is the thing this grammar exists to
avoid.

There is no word for the nearest whole number, because you can write it:
`round_below(col.x + 0.5)`.

**This changes answers**, in two ways. A pipeline that used `to_whole` will now
refuse rather than run, and the replacement you pick may return a different
number than `to_whole` did on negatives.

### `join` on a key the two tables name differently

Real tables rarely agree on what the key is called: one holds `id` and the
other holds `customer_id`. Until now `by` took a single name used on both
sides, so the only way through was to rename a column first and rename it
back afterwards. Say both names instead, with `is` between them, and this
table's comes first:

```r
orders |> join(customers, by = customer_id == id)
```

```python
orders >> join(customers, by = col.customer_id == col.id)
```

The answer keeps this table's name for the key, so the rest of the sentence
goes on reading the way it was written. Keys of both kinds mix in one `by`,
separated by commas, and `matching` takes the same form:

```r
sales |> join(managers, by = c(product, region == area))
sales |> keep(matching(listed, by = product == item))
```

The text form is `join managers by [region] is [area]`.

### `previous` and `following` take how far to look

One row back is still what you get by default. A second argument says how
many, which is what a year-over-year comparison on monthly rows needs:

```r
sales |> sort(month) |> add(year_ago = previous(revenue, 12), by = region)
```

```python
sales >> sort(col.month) >> add(year_ago = previous(col.revenue, 12), by = col.region)
```

It has to be a written whole number rather than a column, because a distance
that changed from row to row would be a different question. Nought is refused
as the column itself, and a negative is refused by naming the word that goes
the other way.

### dplyr no longer takes god's verbs away, in R

god and dplyr share four names: `collect`, `pick`, `rename` and
`summarize`. Attaching dplyr after god handed all four to dplyr, and a
pipeline given to one of them stopped with R's own complaint rather than
an answer:

```
no applicable method for 'collect' applied to an object of class "god_pipeline"
```

It also named a spelling nobody typed, answering about `summarise` when
the line said `summarize`.

Three of the four are generics, and god now gives each of them a method,
so `collect`, `rename` and `summarize` write the same sentence and
return the same table whichever package was attached last:

```r
sales |> keep(region == "West") |> dplyr::summarize(takings = total(revenue))
```

`pick` is the exception. dplyr's `pick` is not a generic, so nothing can
be registered for it; write `god::pick` where both packages are
attached.

Attaching god does not load dplyr, and never did.

## 0.2.1

### `take_last`, for the rows at the far end

`take` gives the rows a pipeline reaches first. `take_last` gives the
ones at the other end, in the order the `sort` asked for:

```r
sales |> sort(revenue) |> take_last(3)
```

```python
sales >> sort(col.revenue) >> take_last(3)
```

Sorting the other way and taking the first three returns the same rows
backwards, which is a different table and usually not the one you
wanted. `by` works here as it does on `take`: the last of each group.

**It always needs a `sort` before it**, where a bare `take` does not.
The rows a pipeline reaches first are at least the rows it reached
first; the rows at the far end are a claim about an end, and a table
does not have one until something says which way it runs.

### One exception to catch, in Python

`GodExpressionError` was not a `GodError`, so `except GodError` — the
line every chapter of the manual teaches — did not catch it. A mistake
the binding spots before a sentence is built, like passing a whole table
where a column belongs, escaped the handler and crashed. It is a
`GodError` now. One idea, one exception to catch.

### A column position says what it was handed

Passing a whole table where a column belongs used to print the entire
table into the error message. It now says that it is a table and how to
name a column of it. Passing a list says to write the columns one at a
time; passing a computed value says to make it a column with `add`
first; anything else is named with its kind. In R, the message said the
verb twice — "`sort` names a column in sort" — and now says it once.

### R 4.1 is the stated minimum, and now it is the true one

The R package said it needed R 4.0. It needs 4.1, and always has: a
pipeline is written with `|>`, the native pipe, which arrived in 4.1. On
4.0 the package could not have installed at all. Anyone on 4.1 or later —
which is anyone who could ever have used it — is unaffected.

## 0.2.0

Three new words, and a worked example on every one of them. `pip install
-U grammar-of-data` in Python; R-universe serves the same version as
`god` on its next sync.

### Every word now has a worked example, in both languages

`?keep` in R and `help(god.keep)` in Python each show a small table going
in and the answer coming out. Every verb, every function and every
grammar word has one, and the two languages show the same example: the
same table, the same numbers, in each language's spelling.

```r
?add_combinations
```

```python
help(god.add_combinations)
```

Eight aggregations — `total`, `average`, `median`, `smallest`,
`largest`, `first`, `last` and `unique_count` — had no Python
documentation at all and now answer `help()`. `first` and `last` say
that they want a `sort` before them, which is the part that bites.

Three R help pages did not exist: `?add_combinations`, `?show_steps`
and `?format.god_steps`. They do now.

### `add_combinations`, for the rows that are not there

Group a table by region and product and the combination nobody recorded
is not a zero in the answer. It is not in the answer. `add_combinations`
makes every combination of the values two columns already hold into a
row, so the gap becomes a line you can read:

```r
sales |>
  add_combinations(region, product) |>
  fill_missing(revenue = 0) |>
  summarize(sold = total(revenue), by = c(region, product))
```

A new row is missing in every other column, and `fill_missing` is where
you say otherwise. That is two steps rather than an argument on purpose:
a region that sold none sold zero, and a sensor that was switched off
did not record a zero, and only you know which one you have.

The values come from the table and nowhere else, so a month with no rows
anywhere is never invented. Nothing already in the table is touched: no
row is dropped, reordered or filled in, including one whose value is
missing.

`by` makes the combinations inside each group instead of across the whole
table, which is what keeps students from being crossed against another
school's questions:

```python
sittings >> add_combinations(col.student, col.question, by = col.school)
```

Two columns or more, always. One column crossed with nothing is the
values it already holds, so the sentence is refused rather than handing
the table back unchanged.

### `join_text`, for putting text together

The grammar could take text apart with `split_text` and had no way to join
it back. `join_text` is that word, and a separator is written where it
goes rather than set somewhere else:

```r
sales |> add(label = join_text(region, " ", product))
```

**A missing value anywhere makes the whole answer missing**, which is the
rule addition already follows. Several engines quietly do the opposite,
dropping the absent part and handing back the rest, so a label built from
a name nobody recorded comes back looking finished. To fill the hole
instead of losing the row, say what to fill it with:
`join_text(first, " ", first_present(last, ""))`.

Numbers are refused rather than converted, and the message names
`to_text`, because how a number should look is a decision you make.

### The drawing is a diagram now

`show_steps` drew a ladder of text and colored it. On a page that is a
picture of a terminal. It draws a diagram instead: each step's columns
are a block of chips, so the block is as wide as the table is. A
`summarize` that takes five columns away draws a third of the width of
the one above it, and a join draws more. You see what a step did before
you read what it was.

A wide table wraps rather than running off the page, and the columns
that left wrap the same way, so a table with forty columns is drawn with
forty columns and nothing is counted or hidden.

At a console it is still the ladder, because that is what the rest of a
session looks like.

### See what a pipeline does to the table, before it runs

`show_steps` draws a pipeline step by step: the table as it stands once
each step has run, with the columns that step makes marked and the ones
it takes away marked where they leave. Nothing executes — the grammar
already reads the whole sentence against the columns before anything
runs, and this is that reading, drawn.

A second table gets a row of its own under the step that reads it, so a
join shows what crossed over and what matched, and the three ways a
second table can take part are drawn apart: a join brings columns and
may multiply the rows, a `matching` brings nothing and can only remove
them, `add_rows` brings rows and leaves the columns alone.

**A sentence god refuses is still drawn**, as far as it checked, with
the refusal under the words that stopped it. So when a column is missing
because an earlier step took it away, the drawing shows where it went —
which is the question an error message on its own cannot answer.

At a console it prints as text. Inside a notebook or a rendered document
it draws itself instead, and neither is something you ask for.

```r
sales |> join(products, by = id) |> summarize(gross = total(revenue), by = region) |> show_steps()
```

```python
god.show_steps(sales >> god.join(products, by=col.id))
```

### The book answers the technical reader

A new appendix, the machine underneath, answers the four questions a
technical evaluator asks: where the work happens (an engine, handed the
whole pipeline as one query), when it runs (one surface, always lazy),
who optimizes it (the engine's planner, which is the point of arriving
whole), and what happens when data outgrows the machine (the frame is
the ceiling on one machine; the warehouse and cluster path is the
answer at scale, and the missing file-scan is recorded honestly in the
coverage appendix). The phrasebook also gains a hard case: one
six-step pipeline, a join through a rank, written out in full in all
seven of `show_as`'s targets, so the claim that translation does not
thin with complexity is shown rather than stated.

### Asking a pipeline a table's question now refuses, in R

`nrow(pipeline)`, `names(pipeline)`, `pipeline$column` and both bracket
forms used to answer `NULL`, or worse: `names` handed back the plan's own
internals, and `sum(pipeline$revenue)` was 0, a plausible number computed
from a question that never ran. Each now refuses and names the repair,
which is always the same word: `collect` first, then ask the table.
Python has always stopped the same slip with its own `TypeError`, so the
two languages now fail it the same loud way. The chapter on laziness
shows the refusal and closes with the rule in one sentence: display runs
a pipeline for free; anything else asks with `collect`.

## 0.1.0

On PyPI as [`grammar-of-data`](https://pypi.org/project/grammar-of-data/):
`pip install grammar-of-data`, then `import god`. The same version ships on
R-universe as `god`. Everything below landed between the first release and
this one.

### A window can no longer fill a hole

`fill_missing [x] as previous([x])` used to compile, with no sort demanded,
so what filled the hole depended on an order nothing had declared — the one
place in the grammar where that could happen. It is refused now, and the
refusal names the spelling that says the same thing with the order settled:
`sort`, then `add [x] as first_present([x], previous([x]))`. Filling with a
plain value is unchanged.

### `show_as` on a sentence that names two tables

Asking for the translation of a text pipeline with a `join` in it used to fail
in both languages — R stopped on an error about a condition, Python with a
refusal that described only the head table. `show_as` now finds every table the
sentence names, the way `run` always did: in your scope, passed by name, or,
where an engine has been given, described by the engine that holds it. A
pipeline built from the verbs was never affected.

### The book measures where the speed lives

A new appendix holds the performance question to the framing the preface
states: the grammar's own toll is clocked live while the page is built,
about five milliseconds per pipeline, and beside it sits a dated record
of six operations on twenty million real taxi rows across god, dplyr,
data.table, pandas and both spellings of polars, every answer proven
identical across all seven implementations before any time was read.
The numbers are read from the committed measurement files, never typed,
and the page says plainly that the record ages: the engine is the
story, not the language.

### Python pipelines on large frames run about three times faster

The binding hands a pandas frame to the engine as an Arrow table now,
because the engine re-analyzed a registered frame on every query: on
twenty million rows, registration alone cost 1.7 seconds per pipeline
and now costs a twentieth of that, and grouped queries dropped by up to
eight times. Answers are unchanged, and the parity harness proves it
sentence by sentence. A frame Arrow cannot convert keeps the old road,
because slower is not wrong.

### The book's tables, one call away

`god_table("sales")` fetches any table from the book's cast, in either
language, from the copies published beside the book itself — so the rows in
your session are the rows every page was computed from, and nothing ships
stale inside a package. The helper is deliberately not named the way the
sibling package names its own: the two are meant to be loaded together, and
neither should mask the other's tables.

### The composition rules are one table, and you can ask for it

`god --seats` prints where each kind of function can stand: every seat and
every kind, stands or refused, with the note the cell carries. The engine's
own tests run every cell through the parser and checker, so the table cannot
drift from what the engine does. A new appendix in the book prints the same
grid, and a new law joins the others: well-formed is enough. A sentence the
seats accept is answered however strange it is, and a refusal always stands
on a law and names what to write instead, never on "nobody would want that".

### The whole vocabulary meets you on one early page

Right after the first pipeline, a new chapter lays out every word the
grammar has, asked from the engine while the page builds, so a new reader
sees how small the thing they are learning is before meeting it one word at
a time. The appendix carrying the same list stays as the reference card.

### A refusal now carries its spelling

Filling a `widen` cell with a window function was refused with a direction
in words alone; the message now writes the repair out: make the place a
column with `add`, then widen from that.

### A column named with a space now runs from R

A column called `order date`, written the way R writes any such name, in
backticks, used to stop with the engine's usage text: the R launcher handed
the column list to the shell unquoted, so the name split at the space. It is
quoted now, and a test in each language holds the trip open. Python was
already correct, and spells such a name `col["order date"]`.

## 0.0.1

The first release. On R-universe since 2026-08-08 as `god`, installable as a
binary with no toolchain. The Python wheels are built and tested in CI; the
PyPI release, as `grammar-of-data`, is next.

### The book grew from a reference into a book

Every teaching chapter now opens with a question about data and follows its
first example with the same sentence in the grammar's own words, which the
build parses. The verb chapters share one declared shape, ending with what
travels with the verb and what it refuses, live in both languages. Part I
gained a reading-practice and a writing-practice chapter, the book gained a
cookbook part organized by question, a chapter collecting the nine design
laws, an afterword with the only exercise the book assigns, an appendix
mapping the verb you know in dplyr, pandas or SQL to god's words, and an
honest appendix of what this grammar does not do, each gap with its exit.
The examples run on a bigger cast: fifteen orders across a year, a wide
survey, and Gapminder's 1,704 rows where scale is the lesson. The grammar's
lineage is now cited on the page, Codd 1970 onward.

### A share of the whole works, in every spelling

`add [share] as [revenue] / total([revenue])`, with no `by`, used to render a
bare aggregate: the SQL engines demanded a grouping nobody wrote, PySpark
raised, and with a `by` PySpark and pandas quietly ignored the grouping,
pandas totaling the whole table where the group was asked. All fixed: the two
SQL dialects write the window (`OVER ()` for the whole table), PySpark writes
`.over(Window.partitionBy(...))`, and pandas reaches the group through
`groupby(...).transform(...)`. One shape pandas genuinely cannot spell, a
grouped aggregate of an expression, is now refused with the one-step repair
named, rather than answered wrongly. Two corpus sentences pin all of it, on
both engines and all three printed targets.

### `add_rows` can name the table at the head

`sales then add_rows sales` used to be refused as "no other table was
described", which told you to do the thing you had just done. Doubling a table
is a legitimate thing to say, and the head table now counts as described.

### The engine is found the same way in both languages

The order, everywhere: `GOD_CLI`, then the source tree's own build, then the
engine bundled with an installed package, then the working directory's tree,
then the `PATH`. Before this, a bundled copy outranked `GOD_CLI` in both
languages, R never looked at the `PATH`, and Python looked at it before the
source tree — so the two languages could quietly run different engines.

### Every refusal in Python is a `GodError`, including the late one

One refusal fires while the query runs rather than before it: `widen` on a
name that appears twice. It used to arrive as the database driver's own error
type, so `except GodError` missed it. It arrives as a `GodError` now, with
the engine's words intact, and one `except` covers every refusal there is.

### A condition asked for a yes or no refuses, in Python

`col.region == "West"` is a column expression, and Python's `and`, `or`, `in`
and `if` used to read it as plain truth — an object is truthy by default, so
every one of them quietly answered yes. Asking now refuses, naming `&`, `|`
and `~` as the spellings that mean it. Notebook probes against `col` no longer
get an expression back either, so a frame's tab-completion stays honest.

### The command line refuses a flag it would have dropped

A second `--as`, a second schema for the head table, or two `--columns` for
one table is now an error naming the repeat, where the last one used to win in
silence.

### The polars translation of `to_date` keeps the time

`show_as(..., "polars")` wrote a cast to a date, so a text timestamp lost its
clock and `hour` on it could not run. It writes `str.to_datetime` now, which
is what the executing engines already meant.

### Help pages, so `?keep` answers

Every export of the R package now has a help page. `?keep`, `?summarize`,
`?lengthen` and the rest used to say there was no documentation, though the
explanations had been written all along — they were in the source and nothing
turned them into pages.

Two of them were also wrong in a way only the rendered page showed: `lengthen`
and `widen` describe a name built from pieces, `name = "{question}_{year}"`, and
the braces were being eaten. The page taught a shape that would not have worked.

### The R package installs on a machine that has never seen this repository

Installing used to need a copy of the engine already built beside the package, so
it worked where god had been developed and nowhere else. The source tarball now
carries the engine's sources and compiles them during installation, which takes a
few seconds and needs [Rust](https://rustup.rs/) on the machine.

```r
install.packages("god_0.0.1.tar.gz", repos = NULL, type = "source")
```

If you already have an engine, point at it and skip the compile:

```r
Sys.setenv(GOD_CLI = "/path/to/god-cli")
```

An install that can find no engine and no way to build one now **refuses**,
naming every place it looked and every way to fix it, rather than succeeding into
a package that cannot answer a pipeline.

### Windows

Both packages now find their engine on Windows, which neither did before: the
file is called `god-cli.exe` there, and both were asking for `god-cli`.

### `god.__version__` reports the version you installed

It said `0.0.0.dev0` regardless.

### `show_as` in Python returns the query instead of printing it

`show_as` prints in R and returns invisibly. In Python it printed *and* returned
a visible string, so a notebook showed the query twice, or showed it once and
then again as a quoted string with the newlines spelled out. It now returns, and
the value it returns shows itself as the query rather than as a quoted string, so
the Python line is the same as the R one:

```python
show_as(sales >> keep(col.region == "West") >> take(10), "dplyr")
```

It is still a string, so `.strip()`, `.splitlines()` and the rest still work. If
you were relying on the printing, wrap the call in `print`.

### The manual is a book now

Parts, chapters and appendices, in a teaching order. The six verbs come first,
so a reader who stops after them can answer real questions about a table;
reshaping is next; and everything after that adds one idea at a time.

Among the appendices: the two languages, which was a chapter. Every word in
the grammar on one page, generated from the engine's own vocabulary. And the
same sentence written out in every dialect `show_as` prints, for anyone
arriving from one of them.

A closing chapter shows god and gog together: one question from a badly shaped
table to a drawn plot.

The book also has a dark theme now, and a toggle for it.

### Two fixes found while writing those chapters

`add(share = revenue / total(revenue), by = product)` produced a query no engine
would run. The share of a group total works now.

In Python, `upper(trim(col.raw))` was refused as "not a column name". Any value
can go inside `lower` and `upper`, the way R and the text form already allowed.


### A table in a warehouse, named and run where it lives

```
shop.orders then summarize [total] as total([revenue]) by [region]
```

A table's name may have parts now, the way a catalog names one. Dots join them
at the head of a pipeline and in `join`, `add_rows` and `matching`. Everywhere
else a dot still means nothing, so a column is unaffected.

Each part is quoted as its own name in the query, which is what makes it find
the table rather than look for one whose name contains dots.

**Running it somewhere else.** In R, `use_engine(connection)` points god at any
`DBI` connection, which includes `sparklyr` and `odbc` against a warehouse.
Tables it already holds are found by name, described without being fetched, and
never copied up to it. Call `use_engine()` to come back.

In Python there is nothing to say. A Spark frame carries its session, so a
pipeline over Spark tables runs on Spark and one over pandas tables runs where
you are. The answer arrives in the same form you asked with: pandas in, pandas
out; Spark in, Spark out, still on the cluster. A pipeline mixing the two is
refused rather than quietly moving one of them.

### Your pipeline in pandas, polars or PySpark

```r
show_as(sales |> keep(region == "West") |> take(10), "polars")
```

`show_as` had four answers and now has seven: `sql`, `spark`, `dplyr`, `pandas`,
`polars`, `pyspark`, and `god` itself. Ask any pipeline what it would be in the
dataframe library you already use, and get something you can paste.

This is the way out at the edge of the vocabulary. A small grammar covers most of
what people do and never all of it, so the question is what happens when you
reach the end, and the answer is that you are handed the same sentence in a tool
you already know.

Every one of them is now checked by running it rather than by reading it. The
printed code is executed against the same tables as the pipeline it came from,
and the two must return the same rows.

### Dates, and looking along the rows

```r
diary |> sort(on) |> add(so_far = running_total(x), before = previous(x))
```

**Dates.** `year`, `month`, `day`, `weekday` and `hour`. `weekday` counts Monday
as 1 wherever you run it, which is the grammar's numbering rather than the
engine's: asked plainly, one engine calls a Friday 5 and another calls it 4.

**Along the rows.** `running_total` adds up as it goes, `previous` and
`following` look one row back and one row on. All three need a `sort` in front of
them, because a total *so far* means nothing until something has said in what
order, and all three say so if they do not get one. `by` restarts them for each
group.

Adding a column with any of these now keeps the row order the `sort` asked for.
It did not before: computing a window regroups the rows and nothing put them
back, so the same pipeline could hand back its groups in a different order.

With these, everything the grammar set out to be able to say, it can say.

### Tidying text, converting values, and `between`

Nine more words.

```r
messy |>
  add(name = trim(raw)) |>
  add(first = split_text(name, " ", 1), size = characters(name))
```

**Text.** `trim` takes the spaces off both ends. `characters` counts them.
`replace_text(x, "a", "b")` looks for the text itself rather than a pattern.
`split_text(x, " ", 1)` gives one piece, counting from 1, and empty text where
there is no such piece.

**Converting.** `to_number`, `to_whole`, `to_text` and `to_date`. Every
conversion begins `to_`, and nothing converts a column on your behalf. Ask a text
function for a number and you are told which conversion you wanted.

**`between(x, low, high)`** counts both ends, the way SQL and dplyr both do.

### `when`, for answering one way or another

```r
pupils |> add(band = when(score >= 90, "A", score >= 70, "B", otherwise = "C"))
```

The arguments come in pairs, a question and then what it gives, and the first
question that is true wins. Leave out `otherwise` and a row that matched nothing
is missing. Every answer has to be the same kind of thing, because they all end
up in one column.

It is `when` rather than Python's own `if`, in Python too. Writing
`"A" if col.score >= 90 else "B"` would decide the answer once, while the
pipeline was being built, and throw the question away without reporting anything.

### The same pipeline runs on Spark

`show_as(pipeline, "spark")` writes the pipeline as Spark SQL, and `--as spark`
does the same from the command line. The sentence does not change; what executes
it does.

```r
show_as(sales |> summarize(revenue = total(revenue), by = product), "spark")
```

The two dialects differ in five places and none of them is yours to remember: how
a column is quoted, how a backslash inside a text value is written, `EXCEPT`
rather than `EXCLUDE`, `raise_error` rather than `error`, and how rows from a
second table are appended.

Where Spark cannot say what a sentence means, you get a refusal rather than a
query that says something close. Its pivot has to be told which columns to make,
so a `widen` without `giving` is refused for Spark and named as such.

Every pipeline in the test corpus now runs on both engines and is checked to
return the same table.

### A pipeline renders as a table in notebooks and documents

Printing a pipeline inside Quarto, R Markdown or Jupyter now gives a table
rather than console text. In R it prints exactly as the table it returns would,
so the document's own `df-print` setting decides the format. In Python a
pipeline offers HTML to anything that asks for it, which is what Jupyter and
Quarto look for.

At an ordinary prompt nothing changes. A console is not a document, and console
output is what the rest of a session looks like.

### Reshaping: `lengthen` and `widen`

Two new verbs. `lengthen` turns columns into rows and the table grows taller;
`widen` turns rows into columns and it grows wider. Direction is in the name so
there is nothing to look up.

```r
answers |> lengthen(q1, q2, q3)
answers |> lengthen(q1, q2, q3, name = question, value = mark)
```

Both verbs use the same two words, and the verb says which way they flow: `name`
always points at the column holding column names, `value` at the column holding
values. Left unsaid, the two columns are called `name` and `value`, so
`answers |> lengthen(q1, q2, q3) |> widen()` returns the table it started from.

`lengthen` chooses its columns the three ways `pick` does: by listing them, with
`all_but(id)`, or with `where(startsWith(name, "q"))`.

**Column names that hold two things** are written as the shape of the name:

```r
terms |> lengthen(all_but(id), name = "{question}_{year}", value = mark)
```

Braces mark a piece, and the text between them is what separates the pieces.
Writing `{value}` for a piece says that piece picks which value column the row
belongs to, so `"{medium}_{value}"` turns `air_mean, air_max, sea_mean, sea_max`
into a `medium` column beside a `mean` and a `max`.

For `widen`, `by` says which columns identify a row, `missing` says what an empty
cell holds, and `giving` says which columns it makes:

```r
marks |> widen(name = question, value = mark, by = student,
               missing = 0, giving = c(q1, q2))
```

Two rows wanting the same cell is refused rather than silently resolved. Saying
what should happen is a value rather than a new argument: write
`value = average(mark)`.

Without `giving`, the column names come from the data, so nothing can know them
before the query runs and no step may follow the `widen`. With `giving`, the rest
of the pipeline is checked as usual, a value the data holds that you did not list
is refused instead of dropped, and empty cells can be filled.

### The verbs, as Python

A pipeline can now be written as ordinary Python, with `>>` and `col.name`.

```python
from god import *

(sales
  >> keep(col.region == "West")
  >> add(margin = col.revenue - col.cost)
  >> summarize(margin = total(col.margin), orders = row_count(), by = col.product)
  >> sort(descending(col.margin))
  >> take(10))
```

It is the same sentence as the R one below, and differs in two things: the pipe
is `>>` rather than `|>`, and a column is `col.name` rather than a bare name.

Two rules come from Python rather than from the grammar. Comparisons joined by
`&` need parentheses, because `&` binds more tightly than `==`. Negation is `~`,
because `not` cannot be overloaded.

Membership and the missing test are methods, since Python's `in` cannot be
reached: `col.region.is_in(["West", "East"])` and `col.cost.is_missing()`, with
`~` for either one's negation. `all_but` inverts a `pick`, in both languages.

Passing a `set` to `is_in` works and sorts the values first, because Python's
hashing changes between runs and the same pipeline has to read the same way every
time. A list keeps the order you wrote.

### The verbs, as R

A pipeline can now be written as ordinary R, with the native pipe and bare
column names.

```r
sales |>
  keep(region == "West") |>
  add(margin = revenue - cost) |>
  summarize(margin = total(margin), orders = row_count(), by = product) |>
  sort(descending(margin)) |>
  take(10)
```

The six verbs are `keep`, `pick`, `add`, `summarize`, `sort` and `take`.
`descending` marks a sort key, and `all_but` inverts a `pick`. Aggregations are
written as they always were: `total`, `average`, `median`, `smallest`,
`largest`, `first`, `last`, `unique_count` and `row_count`.

Writing the pipeline as text still works and still runs the same query. It is
the form for places with no language to bind into, such as a database cell or a
configuration file.

### Nothing runs until you ask

A verb returns a pipeline rather than a table. Printing one runs it; `collect()`
returns the table when you want it directly.

This is also what protects you when a name collides. god's `sort` replaces base
R's for the session, and a pipeline handed to some other package's `sort` has no
meaning there, so it stops rather than returning a plausible answer.

### R's own spellings are translated

You write R, and the verbs write the grammar. `==` becomes `is`, `!=` becomes
`is not`, `&` and `|` and `!` become `and` and `or` and `not`, `%in%` becomes a
set, `is.na(x)` becomes `x is missing`, `TRUE` becomes `yes`, and `NA` becomes
`missing`.

A set of values has to be written out, as `c("West", "East")`. Naming a variable
that holds them is refused rather than guessed at.

### `rows()` is now `row_count()`

The expression that counts rows was called `rows()`, which named the things being
counted rather than the value it returns. Every other function names its value:
`total(x)` gives you the total, `average(x)` the average. `rows()` could be read
as the rows themselves, or as their order, and was.

```r
summarize(orders = row_count(), by = product)
```

It pairs with `unique_count(x)`, which counts distinct values of a column.

### `take` can take the first rows of each group

```r
orders |> sort(id, descending(seen)) |> take(1, by = id)
```

The latest row per id, keeping every column, in one clause. `take(3, by = id)`
gives the top three of each group.

It needs a `sort` before it, and is refused without one, because "the first rows
of each group" means nothing until something has said first by what. That is the
difference between this and pandas' `drop_duplicates(subset=)`, which answers the
question either way and only sometimes answers it the way you meant.

### Refusals say what to write instead

Sixteen messages that named a problem now also name the fix. Naming a subset on
`drop_duplicates` offers both spellings that work; comparing text to a number
says to convert one; filling a number column with text says what to fill it with.

### The rest of the vocabulary

`add_rows`, `drop_duplicates`, `rename`, `drop_missing` and `fill_missing`.

```r
sales |> rename(earned = revenue)
sales |> drop_missing(revenue) |> fill_missing(cost = 0)
sales |> drop_duplicates() |> add_rows(more_sales)
```

```python
sales >> rename(earned = col.revenue)
sales >> drop_missing(col.revenue) >> fill_missing(cost = 0)
sales >> drop_duplicates() >> add_rows(more_sales)
```

**`rename` puts the new name first**, the way assignment reads and the way every
other verb that makes a column already works. If you are arriving from pandas,
read the pair twice: `rename(columns={"old": "new"})` is the other way round, and
both spellings are legal here.

**`drop_duplicates` takes no columns.** Naming a subset means different things in
pandas and in dplyr, and pandas' meaning is unspecified about which row survives.
Write `pick` first and the sentence says exactly what you meant.

Rows come back in a settled order, because dropping repeats says nothing about
the order of what is left and an answer that reorders itself between runs is not
predictable.

**`add_rows` needs both tables to have the same columns.** A column on one side
only is refused rather than filled in with missing values: that is either a
mistake or a decision, and you are the one who knows which.

**`fill_missing` will not change what a column holds.** Filling a number column
with text is refused.

### `join`

Another table's columns, matched on the columns that say which rows correspond.

```r
sales |> join(products, by = product)
```

```python
sales >> join(products, by = col.product)
```

Leave `by` out and god matches on the names both tables share, then tells you
which it chose. It is never silent about a choice you did not make.

`unmatched` says whose unmatched rows survive, because that is the only thing
that varies between the four joins you may know by name:

| `unmatched =` | Which unmatched rows survive | Called elsewhere |
|---|---|---|
| `"this"` (the default) | this table's | left join |
| `"none"` | neither table's | inner join |
| `"both"` | both tables' | full join |

There is no `"other"`. A right join is this join with the tables the other way
round, so it adds no meaning and gets no word.

Three things are refused rather than guessed: a key that is a different kind of
thing in each table, a key one table does not have, and a non-key column that
exists in both, which would otherwise come back twice under a name you did not
choose.

One thing god cannot tell you: if the other table has more than one row per key,
your rows multiply. That is a fact about your data rather than about your
pipeline, and the checker never sees a row.

### `lower` and `upper`, and case in a name test

```r
mixed |> pick(where(startsWith(tolower(name), "q")))
mixed |> keep(tolower(region) == "west")
```

```python
mixed >> pick(where(lower(name).starts("q")))
mixed >> keep(lower(col.Region) == "west")
```

The name tests match exactly, so `name starts "q"` does not find `Q1_score`.
There is no flag for that, because a name is text and text has a case: fold it
and ask the question. The same two words work on a value.

Folding every name automatically would be wrong, since two columns can differ
only by case.

### Choosing columns by what they hold

```r
survey |> pick(where(kind == "number"))
survey |> summarize(where(kind == "number", average(value)))
```

```python
survey >> pick(where(kind == "number"))
survey >> summarize(where(kind == "number", average(value)))
```

`kind` is one of `"number"`, `"text"`, `"truth"` or `"date"`. It sits in the same
`where` as a name test, so the two join:
`where(kind == "number" & startsWith(name, "q"))`.

The `summarize` above names no column at all, so it keeps working when the table
gains one. This is dplyr's `where(is.numeric)` and pandas' `select_dtypes`.

### One value, applied to every column that matches

```r
survey |> add(where(startsWith(name, "q"), value * 10))
survey |> summarize(where(endsWith(name, "_score"), average(value)), by = region)
```

```python
survey >> add(where(name.starts("q"), value * 10))
survey >> summarize(where(name.ends("_score"), average(value)), by = col.region)
```

`value` stands for the column being worked on. The matched columns keep their
names, because `add` already means make or replace.

This is dplyr's `across`. It needed no new verb: the pattern is the same one
`pick` takes, and `value` is the other half of the pair `name` belongs to.

Everything that applies to a value you wrote out applies here too. Asking
`summarize` for something that does not collapse a group is refused, and the
message names the column rather than the pattern. `show_as(..., "god")` shows
what it expanded to.

### Choosing columns by the shape of their name

When a table has thirty columns and you want the eight beginning with `q`:

```r
survey |> pick(where(startsWith(name, "q")))
```

```python
survey >> pick(where(name.starts("q")))
```

`name` is the word for whichever column is being considered. Three tests apply to
it, `starts`, `ends` and `contains`, and they join with `and`, `or` and `not`.

```r
survey |> pick(where(endsWith(name, "_score") | name == "respondent"))
```

A pattern that matches no column is refused rather than handing back a table with
no columns.

### `starts`, `ends` and `contains`

The same three tests ask what is **inside** a column.

```r
sales |> keep(startsWith(region, "W"))
sales |> keep(grepl("adge", product, fixed = TRUE))
```

```python
sales >> keep(col.region.starts("W"))
sales >> keep(col.product.contains("adge"))
```

R spells them with base R's own `startsWith`, `endsWith` and `grepl`, which the
verbs translate. Python spells them as methods, for the same reason `is_in` and
`is_missing` are methods.

The subject is always written, which is why `pick(where(...))` says `name`. It
means `starts` never quietly changes what it is testing.

The value is matched literally, so searching for `"100%"` finds a percent sign
rather than matching everything.

### `first_present`, for when the value is in one of several columns

```r
contacts |> add(reach = first_present(mobile, landline, email))
```

```python
contacts >> add(reach = first_present(col.mobile, col.landline, col.email))
```

The mobile where there is one, otherwise the landline, otherwise the email. SQL
and dplyr both call this `coalesce`.

Two things about it, and both are places people are usually surprised.

The columns are a **priority order**, not a set. It reads left to right and stops
at the first one that has a value, so writing them in a different order gives a
different answer.

The only thing it skips is a **missing** value. A zero, an empty text and a `no`
are all values, and they come back. If you want zero treated as missing too, say
so in a step of its own.

Every column has to hold the same kind of thing, since one of them is going to be
the answer. If all of them are missing for a row, the answer for that row is
missing.

### `rank` and `row_number`

Every row can be given its place.

```r
races |> add(place = rank(descending(score)), by = heat)
```

```python
races >> add(place = rank(descending(col.score)), by = col.heat)
```

**Ties share a place and the next value skips it**, the way a race is scored:
1, 2, 2, 4. That is what people mean by rank, so it gets the word. dplyr calls
this one `min_rank`.

`descending` marks the column exactly as it does in `sort`, and `by` restarts the
numbering inside each group.

`row_number()` numbers the rows 1, 2, 3, 4 and never ties. It takes no argument,
so it can only mean the order the rows are already in, and it is refused without
a `sort` before it rather than answering differently on different runs. `rank`
says what it goes by, so it never needs one.

A place can only be made with `add`. Asking for one inside `keep` is refused,
because a place is worked out over the rows that are left and so cannot be what
chooses them. Make the column and then filter on it, or use `sort` then
`take 3 by group`, which the message names.

dplyr's other four, `dense_rank`, `percent_rank`, `cume_dist` and `ntile`, are
not here. They are real but specialist, and four more words that everyone has to
tell apart is a poor trade for them.

### `except` is now `all_but`, in both languages

The marker that inverts a `pick` is `all_but`, spelled the same in R, in Python
and in the text form.

```r
sales |> pick(all_but(cost, region))
```

```python
sales >> pick(all_but(col.cost, col.region))
```

It was `except`, which Python had to write `except_`, because `except` is a
Python keyword. That was the one word in the vocabulary spelled differently in
the two languages, and there is no longer any such word.

`all_but` also reads as what it does. "pick all but cost and region" is the
sentence; "pick except cost and region" was not quite English. And `except` was
borrowing SQL's name for a different job: SQL's `EXCEPT` removes rows, while this
removes columns.

Writing `except` now names the word that works, and so do `exclude`, `drop`,
`omit` and `without`.

### `matching`, for when you want the rows and not the columns

Sometimes you do not want another table's columns. You want to know which of your
rows appear in it. That is a question about a row, so it is written as one.

```r
sales |> keep(matching(products, by = product))     # the rows with a partner
sales |> keep(!matching(products, by = product))    # the rows without one
```

```python
sales >> keep(matching(products, by = col.product))
sales >> keep(~matching(products, by = col.product))
```

Nothing is added. You get the columns you started with and fewer rows, which is
why it is not spelled `join`. `by` works as it does on `join`, and leaving it out
means the same thing: god matches on the names both tables share and says which
it chose.

**It cannot multiply your rows, and a join can.** If the other table has three
rows for a product, joining gives you three rows back for each of yours. Asking
whether a partner exists does not: a row either has one or it does not, and how
many it has never reaches the answer. Where you want "the rows that appear over
there", this is both the shorter sentence and the safe one.

`matching` is the whole question `keep` asks rather than one part of one, so it
does not combine with `&`. Ask it in its own step, and the refusal says so.

```r
sales |> keep(matching(products, by = product)) |> keep(revenue > 100)
```

### A step sees the table as it arrives

Every value in one step is worked out from the incoming table, so a column made
in a step is not available to the other values in that same step. Use two steps;
the second sees the first.

```r
sales |> add(margin = revenue - cost, doubled = margin * 2)   # refused
sales |> add(margin = revenue - cost) |> add(doubled = margin * 2)
```

Replacing a column is a different case and works, because the old value is on
the table when the step begins: `add(revenue = revenue * 2)`.

This differs from dplyr's `mutate` and pandas' `assign`, which both let a later
column read an earlier one in the same call. The refusal now says so and names
the spelling that works, instead of reporting a missing column you can see
yourself writing.

### Better messages

Calling a verb on something that is not a table now names the function god
replaced and how to reach it:

```
god's `sort` orders the rows of a table, and `c(3, 1, 2)` is not a table.
  For R's own, write `base::sort(c(3, 1, 2))`.
```

### The engine is found where you built it

Both packages now look for the engine at `target/release/god-cli` when running
from a source tree. Before this, the message telling you to run
`cargo build --release` named a fix that did not work: you ran it, nothing
changed, and setting `GOD_CLI` by hand was the only way through.

### The manual

Every example is written twice, in an R tab and a Python tab, above one
explanation. Click the language you write and the other is one click away rather
than one chapter away.

Both tabs are executed when the book is built, so a tab showing a table is a tab
whose code ran, and the two tables come back from the same engine. The chapter
before the verbs is the exception: it is about the two places the languages
differ, and there they are not the same sentence.

Every table on every page is computed by running the code above it. Nothing is
pasted.
