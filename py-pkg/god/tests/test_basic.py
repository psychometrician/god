"""The Python launcher, checked against the rows that come back.

Run from the repository root::

    GOD_CLI=target/release/god-cli python3 py-pkg/god/tests/test_basic.py

**Every assertion is about the frame**, not about the SQL that produced it.
A query can be exactly what was expected and the answer still wrong, and a suite
built on proxies passes for years while the thing it covers is broken.
"""

import contextlib
import io
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import pandas as pd  # noqa: E402

import god  # noqa: E402
from god import GodError  # noqa: E402

passed = 0
failed = 0


def check(label, actual, expected):
    global passed, failed
    if actual == expected:
        passed += 1
        print(f"  ok    {label}")
    else:
        failed += 1
        print(f"  FAIL  {label}\n        wanted: {expected!r}\n        got:    {actual!r}")


def check_error(label, thunk, fragment):
    """Refusal cases hold thunks, not values.

    A table taking a *value* never sees an error raised while the argument is
    being built — the failure happens before the driver is called, and the case
    silently tests nothing.
    """
    global passed, failed
    try:
        thunk()
    except Exception as e:
        if fragment in str(e):
            passed += 1
            print(f"  ok    {label}")
        else:
            failed += 1
            print(f"  FAIL  {label}\n        wanted a message containing: {fragment}\n        got: {e}")
        return
    failed += 1
    print(f"  FAIL  {label}\n        it was accepted, and should not have been")


# The same fixture the Rust and R suites use, so the three can be compared by eye.
sales = pd.DataFrame(
    {
        "region": ["West", "West", "West", "West", "East"],
        "product": ["Widget", "Widget", "Gadget", "Gadget", "Widget"],
        "revenue": [100.0, 200.0, 300.0, 150.0, 500.0],
        "cost": [40.0, 50.0, 100.0, 50.0, 100.0],
    }
)

print("\nthe pipeline, end to end")

answer = god.run(
    """
sales
  then keep where [region] is "West"
  then add [margin] as [revenue] - [cost]
  then summarize [margin] as total([margin]), [orders] as row_count() by [product]
  then sort [margin] descending
  then take 10
"""
)

check("columns come back in the grammar's order", list(answer.columns), ["product", "margin", "orders"])
check("two products survive the filter", len(answer), 2)
check("Gadget totals 300 and sorts first", list(answer["margin"]), [300.0, 210.0])
check("each product had two orders", [int(n) for n in answer["orders"]], [2, 2])
check("Gadget first, Widget second", list(answer["product"]), ["Gadget", "Widget"])

print("\nthe table is found where you are standing")


def in_a_function():
    elsewhere = pd.DataFrame({"a": [3.0, 1.0, 2.0]})
    return len(god.run("elsewhere then take 2"))


check("a table in a local scope needs no naming", in_a_function(), 2)
check(
    "a table can be passed by name",
    list(god.run("t then take 1", t=pd.DataFrame({"a": [1.0]}))["a"]),
    [1.0],
)

# A name with a space is written col["order date"], and the whole trip has to
# survive it: the sentence, the schema handed to the engine, and the query.
# The R launcher shipped a quoting defect exactly here, so both suites pin it.
spaced = pd.DataFrame({"order date": ["2026-01-02", "2026-01-05"], "total": [40, 90]})
check(
    "a column named with a space survives the launcher",
    list(god.collect(spaced >> god.keep(god.col["order date"] > "2026-01-03"))["total"]),
    [90],
)

print("\npandas' own types reach the grammar")

typed = pd.DataFrame(
    {
        "when": pd.to_datetime(["2026-01-01", "2026-06-01"]),
        "flag": [True, False],
        "label": ["a", "b"],
        "n": [1.5, 2.5],
    }
)

check("a boolean column compares against yes", len(god.run('typed then keep where [flag] is yes')), 1)
check("a text column compares against text", len(god.run('typed then keep where [label] is "a"')), 1)
check("a number column compares against a number", len(god.run("typed then keep where [n] > 2")), 1)
check_error(
    "comparing a number column to text is refused",
    lambda: god.run('typed then keep where [n] is "a"'),
    "can never match",
)
check_error(
    "totalling a text column is refused",
    lambda: god.run("typed then summarize [x] as total([label])"),
    "works on numbers",
)

print("\na refusal is the Python error, with its caret")

check_error(
    "an unknown column names the nearest one",
    lambda: god.run("sales then keep where [reveune] > 1"),
    "Did you mean `revenue`?",
)
check_error(
    "and the caret survives the trip",
    lambda: god.run("sales then keep where [reveune] > 1"),
    "^^^^^^^",
)
check_error(
    "a host habit is answered with the grammar's word",
    lambda: god.run("sales then keep where [region] is 'West'"),
    'Write `"West"`',
)
check_error(
    "None is answered with `missing`",
    lambda: god.run("sales then keep where [revenue] is None"),
    "Write `missing`",
)
check_error(
    "a missing table says how to pass one",
    lambda: god.run("no_such_table then take 1"),
    "Pass it by name",
)
check_error(
    "a window cannot fill a hole, and the repair is named",
    lambda: god.run("sales then fill_missing [revenue] as previous([revenue])"),
    "first_present",
)

print("\nthe same pipeline, written as dplyr")

check(
    "show_as returns the translation",
    god.show_as('sales then keep where [region] is "West" then take 10').strip(),
    'sales |>\n  filter((region == "West")) |>\n  head(10)',
)

# **It returns and does not print, and both halves matter.** A notebook and a
# prompt each show what an expression evaluates to, so a `show_as` that printed
# as well would show the query twice; the book had exactly that on two pages.
# The repr is the text rather than a quoted string with `\n` in it, because the
# thing being echoed is a query someone asked to read.
_written = god.show_as('sales then take 1', "sql")
check("show_as reprs as the query itself", repr(_written), str(_written))
check("show_as is still a string", isinstance(_written, str), True)

_out = io.StringIO()
with contextlib.redirect_stdout(_out):
    god.show_as('sales then take 1', "sql")
check("show_as prints nothing of its own", _out.getvalue(), "")

# **Every table a join names is described**, not only the head. The second is
# found in scope the way the first is, because `show_as` resolves the same
# names `run` does; describing the head alone left the grammar refusing a
# sentence the caller could run.
products = pd.DataFrame({"product": ["Widget", "Gadget"], "maker": ["Acme", "Bolt"]})
check(
    "show_as describes every table a join names",
    god.show_as("sales then join products by [product] then take 3").strip(),
    "sales |>\n  left_join(products, by = join_by(product)) |>\n  head(3)",
)

print("\nwhat a pipeline does to the table, drawn")

# **Nothing here runs the pipeline**, and that is the point of the feature: the
# grammar reads the whole sentence against the columns before anything executes,
# so the drawing is available while the answer is not.

_steps = god.show_steps("sales then summarize [gross] as total([revenue]) by [region]")

check("a column the step makes is marked", "+gross" in _steps.text, True)
check(
    "and the ones it takes away are marked where they leave",
    "-product" in _steps.text,
    True,
)

# **The drawing resolves the same names `show_as` does**, because both ask the
# one lookup. A join names a second table, and the drawing gives it a row of its
# own under the step that reads it.
check(
    "a table joining in gets a row of its own",
    "└ products" in god.show_steps("sales then join products by [product]").text,
    True,
)

# **A refused sentence is still drawn**, which is the thing an error message on
# its own cannot do: it says how far the pipeline got, and where the column went.
_refused = god.show_steps(
    'sales then summarize [g] as total([revenue]) by [region] then keep where [product] is "x"'
).text
check("a refused sentence is drawn as far as it checked", "+g" in _refused, True)
check(
    "and carries the refusal under the words that stopped it", "^^" in _refused, True
)

# **A prompt gets the ladder and a page gets the picture**, and neither is an
# argument anybody passes: the notebook asks for HTML and nothing else does.
check("the picture is a whole document", _steps.svg.startswith("<svg "), True)
check("the page hook is the picture", _steps._repr_html_(), _steps.svg)
check("the prompt gets the ladder", repr(_steps), _steps.text.rstrip("\n"))
check("and it does not end on a blank line", repr(_steps).endswith("\n"), False)

_out = io.StringIO()
with contextlib.redirect_stdout(_out):
    god.show_steps("sales then take 1")
check("show_steps prints nothing of its own", _out.getvalue(), "")

print("\nthe verbs write the grammar's own sentence")

# **Asserted on the text rather than on the rows**, deliberately. A verb's whole
# job is to write a sentence; checking the frame it eventually produces would
# pass just as happily on a sentence that meant something else and happened to
# agree on this fixture.

from god import (  # noqa: E402
    add, all_but, average, col, collect, descending, keep, largest,
    first_present, kind, lengthen, lower, matching, name, pick, rank, row_count,
    row_number, smallest,
    sort, summarize, upper, value, when, where, widen,
    take, total,
    to_number, to_whole, to_text, to_date,
    trim, characters, replace_text, split_text, between,
    year, month, day, weekday, hour,
    running_total, previous, following,
)

check("keep translates Python's equality",
      (sales >> keep(col.region == "West")).written(),
      'sales\n  then keep where ([region] is "West")')

check("pick names its columns",
      (sales >> pick(col.product, col.revenue)).written(),
      "sales\n  then pick [product, revenue]")

check("all_but inverts the list rather than adding a verb",
      (sales >> pick(all_but(col.cost))).written(),
      "sales\n  then pick all_but [cost]")

check("add names the column it makes",
      (sales >> add(margin = col.revenue - col.cost)).written(),
      "sales\n  then add [margin] as ([revenue] - [cost])")

check("summarize carries its grouping",
      (sales >> summarize(total = total(col.revenue), by = col.product)).written(),
      "sales\n  then summarize [total] as total([revenue]) by [product]")

check("several grouping columns are written as a list",
      (sales >> summarize(n = row_count(), by = [col.region, col.product])).written(),
      "sales\n  then summarize [n] as row_count() by [region, product]")

check("descending is a modifier on a column",
      (sales >> sort(descending(col.revenue), col.cost)).written(),
      "sales\n  then sort [revenue] descending, [cost]")

check("take counts rows", (sales >> take(3)).written(), "sales\n  then take 3")

check("the steps chain in the order they were written",
      (sales >> keep(col.revenue > 100) >> take(2)).written(),
      "sales\n  then keep where ([revenue] > 100)\n  then take 2")

print("\nthe table's own name reaches the sentence")

# The one thing Python had to solve differently from R: `>>` hands over a frame
# with no name attached, so the caller's scope is read backwards to find it.
check("a named frame keeps its name",
      (sales >> take(1)).written().splitlines()[0], "sales")
check("a frame with no name of its own falls back, as R does",
      (pd.DataFrame({"a": [1]}) >> take(1)).written().splitlines()[0], "table")

print("\nPython's habits become the grammar's words")

def written(pipeline):
    return pipeline.written().replace("sales\n  then keep where ", "")

check("!= becomes is not", written(sales >> keep(col.region != "West")), '([region] is not "West")')
check("& becomes and", written(sales >> keep((col.revenue > 1) & (col.cost > 1))), "(([revenue] > 1) and ([cost] > 1))")
check("| becomes or", written(sales >> keep((col.revenue > 1) | (col.cost > 1))), "(([revenue] > 1) or ([cost] > 1))")
check("~ becomes not", written(sales >> keep(~(col.region == "West"))), '(not ([region] is "West"))')
check("is_in becomes a set", written(sales >> keep(col.region.is_in(["West", "East"]))), '([region] in {"West", "East"})')
check("a negated is_in is not in", written(sales >> keep(~col.region.is_in(["West"]))), '([region] not in {"West"})')
check("is_missing becomes is missing", written(sales >> keep(col.cost.is_missing())), "([cost] is missing)")
check("and its negation has its own words", written(sales >> keep(~col.cost.is_missing())), "([cost] is not missing)")
check("True becomes yes", written(sales >> keep(col.region == True)), "([region] is yes)")
check("False becomes no", written(sales >> keep(col.region == False)), "([region] is no)")
check("None becomes missing", written(sales >> keep(col.region == None)), "([region] is missing)")
check("a large number is never written in scientific notation",
      written(sales >> keep(col.revenue > 100000)), "([revenue] > 100000)")

# Python's hashing is randomized per process, so a set written out as it came
# would emit a different sentence on every run.
check("a set is put in a settled order, so the sentence does not change between runs",
      written(sales >> keep(col.region.is_in({"West", "East", "North"}))),
      '([region] in {"East", "North", "West"})')

print("\nnothing runs until the answer is wanted")

check("a verb returns a pipeline, not a frame",
      isinstance(sales >> take(1), pd.DataFrame), False)
check("collect runs it", len(collect(sales >> take(2))), 2)
check("the rows are the ones the text form gives",
      list(collect(sales >> keep(col.region == "West") >> sort(descending(col.revenue)) >> take(2))["revenue"]),
      list(god.run('sales then keep where [region] is "West" then sort [revenue] descending then take 2')["revenue"]))

print("\nthe Python form and the text form are the same sentence")

# The fourth witness, asserted here as well as in the parity harness so that the
# Python suite alone can catch a translator that has drifted.
def same_query(native, text):
    from god.run import _columns_of
    columns = _columns_of(sales)
    return god.god_sql(native.written(), columns) == god.god_sql(text, columns)

check("a filter agrees",
      same_query(sales >> keep(col.region == "West"), 'sales then keep where [region] is "West"'), True)
check("a grouped summary agrees",
      same_query(sales >> summarize(total = total(col.revenue), by = col.product),
                 "sales then summarize [total] as total([revenue]) by [product]"), True)
check("a sort agrees",
      same_query(sales >> sort(descending(col.revenue), col.cost),
                 "sales then sort [revenue] descending, [cost]"), True)

print("\none value applied to every column that matches")

survey = pd.DataFrame({
    "respondent": [1, 2],
    "q1_score":   [4, 5],
    "q2_score":   [2, 5],
    "region":     ["West", "East"],
})

check("add writes the pattern and the value",
      (survey >> add(where(name.starts("q"), value * 2))).written(),
      'survey\n  then add where (name starts "q") as (value * 2)')
check("the matched columns keep their names",
      list(collect(survey >> add(where(name.starts("q"), value * 2))).columns),
      ["respondent", "region", "q1_score", "q2_score"])
check("and every one of them was doubled",
      list(collect(survey >> add(where(name.starts("q"), value * 2)))["q1_score"]),
      [8, 10])
check("summarize takes the same shape",
      list(collect(survey >> summarize(where(name.ends("_score"), average(value))))["q2_score"]),
      [3.5])
check("and it groups",
      len(collect(survey >> summarize(where(name.ends("_score"), average(value)), by = col.region))),
      2)

check_error("value is not a word outside where",
            lambda: collect(survey >> add(x = value * 2)),
            "only `add where` and `summarize where`")
check_error("a pattern matching nothing makes nothing",
            lambda: collect(survey >> add(where(name.starts("zzz"), value * 2))),
            "no column's name matches")
check_error("where has to say what to make of each column",
            lambda: survey >> add(where(name.starts("q"))),
            "what to make of each column")

print("\nchoosing columns by the shape of their name")

wide = pd.DataFrame({"q1": [1], "q2": [2], "region": ["W"], "revenue": [3]})

check("pick where writes a question about a name",
      (wide >> pick(where(name.starts("q")))).written(),
      'wide\n  then pick where (name starts "q")')
check("and the columns that matched come back",
      list(collect(wide >> pick(where(name.starts("q")))).columns),
      ["q1", "q2"])
check("it joins with or",
      list(collect(wide >> pick(where(name.starts("q") | (name == "region")))).columns),
      ["q1", "q2", "region"])
check("and with not",
      list(collect(wide >> pick(where(~name.starts("q")))).columns),
      ["region", "revenue"])
check("ends and contains work on a name too",
      list(collect(wide >> pick(where(name.ends("1") | name.contains("ven")))).columns),
      ["q1", "revenue"])

check("columns can be chosen by what they hold",
      list(collect(wide >> pick(where(kind == "number"))).columns),
      ["q1", "q2", "revenue"])
check("and by what they do not hold",
      list(collect(wide >> pick(where(kind != "number"))).columns),
      ["region"])
check("kind and name join, which is the point of the where",
      list(collect(wide >> pick(where((kind == "number") & name.starts("q")))).columns),
      ["q1", "q2"])
check("one aggregation over every number, whatever they are called",
      list(collect(wide >> summarize(where(kind == "number", average(value)))).columns),
      ["q1", "q2", "revenue"])
check_error("a kind the grammar does not have lists the ones it does",
            lambda: collect(wide >> pick(where(kind == "numeric"))),
            "`number`")

mixed = pd.DataFrame({"Q1": [1], "q2": [2], "Region": ["x"]})
check("a name test is case-sensitive on its own",
      list(collect(mixed >> pick(where(name.starts("q")))).columns), ["q2"])
check("and folding the case catches both",
      list(collect(mixed >> pick(where(lower(name).starts("q")))).columns), ["Q1", "q2"])
check("the same two words fold a value",
      len(collect(mixed >> keep(lower(col.Region) == "x"))), 1)
check_error("only text has a case",
            lambda: collect(mixed >> keep(lower(col.q2) == "x")),
            "Only text has a case")

check("the same three words test a value, with the subject written",
      (wide >> keep(col.region.starts("W"))).written(),
      'wide\n  then keep where ([region] starts "W")')
check("and they run",
      len(collect(wide >> keep(col.region.contains("e")))), 0)

check_error("name is not a word outside pick where",
            lambda: collect(wide >> keep(name.starts("q"))),
            "`pick where` is the one place")
check_error("a pattern matching nothing is refused",
            lambda: collect(wide >> pick(where(name.starts("zzz")))),
            "no column's name matches")
check_error("where chooses on its own",
            lambda: wide >> pick(where(name.starts("q")), col.region),
            "nothing goes beside it")

print("\nthe first column that has a value")

patchy_three = pd.DataFrame({"a": [1.0, None, None], "b": [None, 2.0, None], "c": [9.0, 9.0, 9.0]})

check("it reads left to right and takes the first one present",
      list(collect(patchy_three >> add(best = first_present(col.a, col.b, col.c)))["best"]),
      [1.0, 2.0, 9.0])
check("order is priority, so swapping the arguments changes the answer",
      list(collect(patchy_three >> add(best = first_present(col.c, col.a, col.b)))["best"]),
      [9.0, 9.0, 9.0])
check("a zero is present, and only missing is skipped",
      list(collect(pd.DataFrame({"a": [0.0, None], "b": [5.0, 5.0]})
                   >> add(best = first_present(col.a, col.b)))["best"]),
      [0.0, 5.0])

check_error("one column is not a choice",
            lambda: collect(patchy_three >> add(best = first_present(col.a))),
            "at least two columns")
check_error("the columns have to hold the same kind of thing",
            lambda: collect(pd.DataFrame({"a": [1.0], "b": ["x"]})
                            >> add(best = first_present(col.a, col.b))),
            "same kind of thing")

print("\na place is worked out over the rows, not for one of them")

ranked = pd.DataFrame({
    "heat":  ["x", "x", "y", "y"],
    "name":  ["a", "b", "c", "d"],
    "score": [20.0, 20.0, 5.0, 50.0],
})

check("rank writes an ordering key, not a value",
      (ranked >> add(place = rank(descending(col.score)))).written(),
      "ranked\n  then add [place] as rank([score] descending)")
check("ties share a place and the next one skips",
      list(collect(ranked >> add(place = rank(col.score)) >> sort(col.name))["place"]),
      [2, 2, 1, 4])
check("a group restarts the numbering",
      list(collect(ranked >> add(place = rank(descending(col.score)), by = col.heat)
                   >> sort(col.name))["place"]),
      [1, 1, 2, 1])
check("row_number never ties where rank does",
      list(collect(ranked >> sort(col.score) >> add(n = row_number()) >> sort(col.n))["n"]),
      [1, 2, 3, 4])

check_error("row_number without a sort says what to write",
            lambda: collect(ranked >> add(n = row_number())),
            "nothing has said what that order is")
check_error("a window cannot choose the rows it is computed over",
            lambda: collect(ranked >> keep(rank(col.score) <= 2)),
            "cannot be what chooses them")
check_error("a window in a summarize is refused in its own words",
            lambda: collect(ranked >> summarize(p = rank(col.score), by = col.heat)),
            "nowhere to go")

print("\na filtering join reads a second table from inside a condition")

# `matching` is the only expression that names a table, so the verb has to
# notice it and hand that table over. Nothing else in a sentence reaches outside
# the table at its head, which is why this is worth its own section.
catalog = pd.DataFrame({"product": ["Widget", "Gizmo"], "maker": ["Acme", "Globex"]})

check("a semi join keeps only the rows with a partner",
      list(collect(sales >> keep(matching(catalog, by = col.product)))["product"]),
      ["Widget", "Widget", "Widget"])
check("an anti join keeps exactly the others",
      sorted(set(collect(sales >> keep(~matching(catalog, by = col.product)))["product"])),
      ["Gadget"])
check("the table travels with the pipeline",
      "catalog" in (sales >> keep(matching(catalog, by = col.product))).tables, True)
check("the key can be left to the shared names",
      len(collect(sales >> keep(matching(catalog)))), 3)
check("a filtering join adds no columns",
      list(collect(sales >> keep(matching(catalog, by = col.product))).columns),
      list(sales.columns))
# The sentence rather than the query, because `same_query` describes only the
# table at the head and a filtering join names a second one.
check("an anti join writes the sentence R writes",
      (sales >> keep(~matching(catalog, by = col.product))).written(),
      "sales\n  then keep where (not matching(catalog, by [product]))")

check_error("matching cannot be half of a bigger question",
            lambda: collect(sales >> keep(matching(catalog, by = col.product) & (col.revenue > 100))),
            "its own step")
check_error("matching needs a table rather than a value",
            lambda: keep(matching("catalog")), "needs another table")

# **A pipeline refuses a table's questions before collect.** Python does this
# of its own accord — a Pipeline has no len, no [] and no iteration — and the
# suite pins it because R had to build the same loudness by hand: there, the
# language's own answer was NULL, silently.
check_error("a plan has no length", lambda: len(sales >> take(1)), "Pipeline")
check_error("a plan cannot be subscripted",
            lambda: (sales >> take(1))["revenue"], "Pipeline")
check_error("a plan does not iterate", lambda: list(sales >> take(1)), "Pipeline")

print("\ndates, and looking along the rows")

diary = pd.DataFrame({
    "g": ["a", "a", "b"],
    "on_": ["2026-01-02", "2026-01-05", "2026-01-06"],
    "x": [10, 20, 30],
})

dated = collect(diary >> add(d = to_date(col.on_))
                      >> add(y = year(col.d), m = month(col.d), wd = weekday(col.d)))
check("year and month read what they say",
      [int(dated["y"][0]), int(dated["m"][0])], [2026, 1])
# Monday is 1, and it is the grammar's numbering rather than the engine's.
check("weekday counts Monday as 1", [int(n) for n in dated["wd"]], [5, 1, 2])

check_error("a date part refuses a number and names the conversion",
            lambda: collect(diary >> add(y = year(col.x))), "to_date(...)")

running = collect(diary >> sort(col.on_) >> add(so_far = running_total(col.x)))
check("the running total adds up as it goes",
      [int(n) for n in running["so_far"]], [10, 30, 60])

grouped = collect(diary >> sort(col.on_) >> add(so_far = running_total(col.x), by = col.g))
check("by restarts it", [int(n) for n in grouped["so_far"]], [10, 30, 30])
check("and the order that was asked for survives", list(grouped["on_"]),
      ["2026-01-02", "2026-01-05", "2026-01-06"])

steps = collect(diary >> sort(col.on_) >> add(before = previous(col.x), after = following(col.x)))
check("previous looks one row back",
      [None if pd.isna(v) else int(v) for v in steps["before"]], [None, 10, 20])
check("following looks one row on",
      [None if pd.isna(v) else int(v) for v in steps["after"]], [20, 30, None])

check_error("a window that is not told an order needs a sort",
            lambda: collect(diary >> add(v = running_total(col.x))),
            "nothing has said what that order is")

print("\nconverting, text, and between")

messy = pd.DataFrame({"raw": ["  ann marie  ", "  bob  "], "n": [7, 99]})

tidied = collect(
    messy
    >> add(name = trim(col.raw))
    >> add(first = split_text(col.name, " ", 1), size = characters(col.name),
           fixed = replace_text(col.name, "a", "A"))
)
check("trim takes the spaces off both ends", list(tidied["name"]), ["ann marie", "bob"])
check("split_text says which piece it wants", list(tidied["first"]), ["ann", "bob"])
check("characters counts them", [int(n) for n in tidied["size"]], [9, 3])
check("replace_text looks for text, not a pattern", list(tidied["fixed"]), ["Ann mArie", "bob"])

check("between counts both ends",
      [int(n) for n in collect(messy >> keep(between(col.n, 7, 99)))["n"]], [7, 99])
check("and nothing is between the ends when they exclude everything",
      len(collect(messy >> keep(between(col.n, 8, 98)))), 0)
check("a conversion says what it gives",
      [int(n) for n in collect(messy >> add(word = to_text(col.n))
                                     >> add(size = characters(col.word)))["size"]],
      [1, 2])

check_error("a text function refuses a number and names the conversion",
            lambda: collect(messy >> add(x = trim(col.n))), "to_text(...)")
check_error("between needs all three to be the same kind of thing",
            lambda: collect(messy >> keep(between(col.n, 1, "ten"))), "same kind of thing")

print("\nthe conditional")

pupils = pd.DataFrame({"name": ["ann", "bob", "cat"], "score": [95, 75, 50]})

check("the first question that is true wins",
      list(collect(pupils >> add(band = when(col.score >= 90, "A", col.score >= 70, "B",
                                             otherwise = "C")))["band"]),
      ["A", "B", "C"])
# Order is the meaning, and it is the thing people get wrong, so it is asserted
# rather than left implied by the example above.
check("so the same questions the other way round answer differently",
      list(collect(pupils >> add(band = when(col.score >= 70, "B", col.score >= 90, "A",
                                             otherwise = "C")))["band"]),
      ["B", "B", "C"])
check("a row matching nothing is missing without an otherwise",
      [v if isinstance(v, str) else "missing"
       for v in collect(pupils >> add(top = when(col.score >= 90, "yes")))["top"]],
      ["yes", "missing", "missing"])

check_error("every answer has to be the same kind of thing",
            lambda: collect(pupils >> add(band = when(col.score >= 90, "A", otherwise = 0))),
            "same kind of thing")
check_error("a question with no answer beside it is refused",
            lambda: when(col.score >= 90, "A", col.score >= 70),
            "needs the answer that goes with it")
# Python's own conditional cannot carry this, which is why the word exists.
check_error("something that is not a question is refused",
            lambda: when("A", "B"), "is not a question")

print("\nreshaping, in both directions")

# A survey in the shape people actually receive one: a row per person, a column
# per question.
answers = pd.DataFrame({
    "student": ["ann", "bob"],
    "q1": [1, 4], "q2": [2, 5], "q3": [3, 6],
})

tall = collect(answers >> lengthen(col.q1, col.q2, col.q3))
check("the two new columns take the grammar's own words",
      list(tall.columns), ["student", "name", "value"])
check("every column becomes a row", len(tall), 6)
check("each row's answers stay together", list(tall["name"][:3]), ["q1", "q2", "q3"])
check("and carry their values", [int(v) for v in tall["value"][:3]], [1, 2, 3])

check("the two verbs are inverses, spelled with nothing at all",
      collect(answers >> lengthen(col.q1, col.q2, col.q3) >> widen()).to_dict("list"),
      answers.to_dict("list"))

check("all_but chooses the same columns as listing them",
      collect(answers >> lengthen(all_but(col.student))).to_dict("list"),
      tall.to_dict("list"))
check("and so does a question about the name",
      collect(answers >> lengthen(where(name.starts("q")))).to_dict("list"),
      tall.to_dict("list"))

# Names that hold two things, which is where `pivot_longer` stops being easy.
terms = pd.DataFrame({"id": [1], "q1_2020": [10], "q1_2021": [11]})
split = collect(terms >> lengthen(all_but(col.id), name = "{question}_{year}", value = col.answer))
check("a pattern splits one name into two columns",
      list(split.columns), ["id", "question", "year", "answer"])
check("and the pieces are the pieces", list(split["year"]), ["2020", "2021"])

wide = collect(
    answers
    >> lengthen(col.q1, col.q2, col.q3)
    >> widen(name = col.name, value = col.value, by = col.student,
             giving = [col.q1, col.q2, col.q3])
    >> add(gain = col.q3 - col.q1)
)
check("a widen that says what it makes can be carried on from",
      list(wide.columns), ["student", "q1", "q2", "q3", "gain"])
check("and the arithmetic after it is real", [int(g) for g in wide["gain"]], [2, 2])

check_error("stacking two kinds of column is refused",
            lambda: collect(answers >> lengthen(col.student, col.q1)),
            "two kinds of thing in one column")
check_error("a step after a widen that declares nothing is refused",
            lambda: collect(answers >> lengthen(col.q1, col.q2, col.q3) >> widen() >> take(1)),
            "giving [q1, q2, q3]")
check_error("lengthen needs the columns that become rows",
            lambda: lengthen(), "lengthen(col.q1")

print("\na pipeline printed in a notebook is a table, not console text")

rendered = (sales >> keep(col.region == "West") >> take(2))._repr_html_()
check("a pipeline offers HTML to anything that asks for it",
      "<table" in rendered, True)
check("and the table holds the answer", "West" in rendered, True)
# pandas' row numbers are its bookkeeping rather than anything the table says,
# and R's side of the same example does not show them.
check("without the row numbers", "<th>0</th>" not in rendered, True)
check("repr is still console text, because a prompt is not a document",
      "<table" in repr(sales >> take(1)), False)

print("\nthe grammar still owns every refusal")

check_error("an unknown column is caught by the grammar, not the verbs",
            lambda: collect(sales >> keep(col.reveune > 1)), "Did you mean `revenue`?")
check_error("a column made in a step is not there yet for that step",
            lambda: collect(sales >> add(margin = col.revenue - col.cost, doubled = col.margin * 2)),
            "is made by this same `add`")

print("\nthe verbs refuse what they cannot write")

check_error("pick takes columns, not expressions",
            lambda: sales >> pick(col.revenue + col.cost), "is not a column name")
check_error("all_but wants all its columns inside it",
            lambda: sales >> pick(all_but(col.cost), col.region), "pick(all_but(col.cost, col.region))")
check_error("take wants a whole number", lambda: sales >> take(2.5), "take(10)")
check_error("a verb needs a table", lambda: 3 >> take(1), "works on a table")
check_error("an expression is not a yes or no",
            lambda: bool(col.revenue == 100), "not a yes or no")
check("a notebook's probe is not a column",
      hasattr(col, "_ipython_canary_method_should_not_exist_"), False)

print("\nwhich engine answers")

# The order is the contract, and it is the same in R: GOD_CLI, then a source
# tree's own build, then the bundled copy, then the working directory's tree,
# then the PATH. The second check is the one with a history: a bundled copy
# left behind by a wheel build used to outrank everything, so a harness could
# spend a day testing last week's engine.
import os as _os  # noqa: E402
import tempfile as _tempfile  # noqa: E402
from god.run import _EXE as _exe, _binary as _resolve  # noqa: E402

with _tempfile.NamedTemporaryFile(delete=False) as _f:
    _named_engine = _f.name
_old_god_cli = _os.environ.pop("GOD_CLI", None)
_os.environ["GOD_CLI"] = _named_engine
check("an explicit GOD_CLI outranks everything", _resolve(), _named_engine)
_os.environ.pop("GOD_CLI", None)
check("a source tree's build outranks a bundled copy",
      _resolve().endswith(_os.path.join("target", "release", _exe)), True)
if _old_god_cli is not None:
    _os.environ["GOD_CLI"] = _old_god_cli
_os.unlink(_named_engine)

print("\nthe two bindings agree with the engine about every word")

# **Run here rather than left to whoever remembers.** The R suite runs the book
# guards for the same reason: a check nobody runs is a check that does not exist,
# and this one covers a defect that shipped for a day and was found by a person
# reading the manual.
import subprocess  # noqa: E402

_guard = subprocess.run(
    [sys.executable, str(Path(__file__).resolve().parents[3] / "parity" / "vocabulary.py")],
    capture_output=True, text=True,
)
if _guard.returncode == 0:
    passed += 1
    print("  ok    the vocabulary guard agrees")
else:
    failed += 1
    print("  FAIL  the vocabulary guard disagrees\n" + _guard.stdout.rstrip())

# The corpus on both engines, for the same reason and with one difference: this
# one needs pyspark and a JVM, so where they are missing it says so rather than
# failing. **A skip is not a pass**, and it prints as neither, because the whole
# defect it covers is the kind that looks like success.
try:
    import pyspark  # noqa: F401

    _has_spark = True
except ImportError:
    _has_spark = False

if _has_spark:
    _spark = subprocess.run(
        [sys.executable, str(Path(__file__).resolve().parents[3] / "parity" / "spark.py")],
        capture_output=True, text=True,
    )
    if _spark.returncode == 0:
        passed += 1
        print("  ok    the corpus returns the same tables on DuckDB and on Spark")
    else:
        failed += 1
        print("  FAIL  the two engines disagree\n" + _spark.stdout.rstrip()[-2000:])
else:
    print("  note: pyspark is not installed here, so the Spark dialect is unchecked.")
    print("        Two of its five differences from DuckDB do not raise anything,")
    print("        so this is worth installing: pip install pyspark")

# The printing backends, run rather than read. **Nothing else executes them**,
# which is why a rendering can read perfectly and mean something else: a bare
# string is a column name to polars, so `then("big")` is not the word "big".
# Each target skips cleanly when its library is missing and the guard says so.
_printed = subprocess.run(
    [sys.executable, str(Path(__file__).resolve().parents[3] / "parity" / "printed.py")],
    capture_output=True, text=True,
)
if _printed.returncode == 0:
    passed += 1
    print("  ok    the printed code returns what the sentence meant")
else:
    failed += 1
    print("  FAIL  a printing backend renders something else\n"
          + _printed.stdout.rstrip()[-2000:])

# The Python half of every documented refusal. The R suite forces the
# `error: true` chunks; the try/except twins beside them were proved by
# nothing until this guard, and a binding can drift visibly on a page while
# parity stays green — `show_as` did exactly that for a day.
_refusals = subprocess.run(
    [sys.executable, str(Path(__file__).resolve().parents[3] / "book" / "check_refusals.py")],
    capture_output=True, text=True,
)
if _refusals.returncode == 0:
    passed += 1
    print("  ok    every Python refusal in the book still refuses")
else:
    failed += 1
    print("  FAIL  a Python refusal in the book stopped refusing\n"
          + _refusals.stdout.rstrip()[-2000:])

print("\nwhat the package exports")

check("the six verbs are exported",
      [v for v in ["keep", "pick", "add", "summarize", "sort", "take"] if v not in god.__all__], [])
check("so is every function the grammar has",
      [f for f in ["total", "average", "median", "smallest", "largest",
                   "first", "last", "unique_count", "row_count"] if f not in god.__all__], [])
check("and the text form is still there",
      [n for n in ["run", "show_as", "god_sql", "col", "collect"] if n not in god.__all__], [])

print("\nthe guard can fail")
_before = failed
check("a deliberately wrong expectation fails", 1 + 1, 3)
if failed == _before:
    print("  FAIL  the checker cannot fail, so nothing above is evidence")
    failed += 1
else:
    failed = _before
    passed += 1
    print("  ok    (the failure above was deliberate, and the checker caught it)")

print("\na table named in parts, for a catalog")

# **The whole of `main.sales.orders` quoted as one name is a table nobody has.**
# The query parses and then fails looking for something that was never there, so
# this is a mistake that reads correctly and cannot work.
_parts = god.show_as("shop.orders then take 1", "spark", **{"shop.orders": pd.DataFrame({"a": [1]})})
check("each part of the name is quoted on its own", "`shop`.`orders`" in _parts, True)
check("and the whole name is not quoted as one", "`shop.orders`" in _parts, False)

# Spark is the one printing target with a catalog to look a name up in.
_py = god.show_as("shop.orders then take 1", "pyspark", **{"shop.orders": pd.DataFrame({"a": [1]})})
check("pyspark asks the session for it", 'spark.table("shop.orders")' in _py, True)

check_error("a dot with nothing after it says so",
            lambda: god.show_as("shop. then take 1", "sql", **{"shop": pd.DataFrame({"a": [1]})}),
            "names a table in parts")


# --- god_table(): the book's tables, fetched by name -------------------------
# Binding plumbing rather than a word of the grammar. The offline checks always
# run; the fetch itself is never in the suite, because a suite has to pass on a
# laptop with no network. The sibling package ships book_table for its own
# book, and the two names differ on purpose: loaded together, neither masks
# the other's tables.
check("the tables come from the published book",
      god.tables.GOD_BOOK_DATA_URL,
      "https://psychometrician.github.io/god-book/data/")
check_error("god_table with something that is not a name",
            lambda: god.god_table(42), "god_table")

print(f"\n{passed} passed, {failed} failed")
sys.exit(1 if failed else 0)
