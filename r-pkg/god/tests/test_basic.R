# The R launcher, checked against the rows that come back.
#
# Run from the repository root:
#
#     GOD_CLI=target/release/god-cli Rscript r-pkg/god/tests/test_basic.R
#
# **The package is loaded the way a user loads it**, through `NAMESPACE`, rather
# than by sourcing the files. Sourcing skips the namespace entirely, so a
# function can pass every test here while being invisible to `library(god)`.

suppressMessages({
  library(DBI)
  library(duckdb)
})

# **This file runs in two situations and has to tell them apart**, because the
# paths here are relative to the repository root and only one of those
# situations has a repository.
#
#   * From the repo root, as the development suite. The package is loaded from
#     source with `pkgload::load_all()`, which honors the hand-written
#     `NAMESPACE` (`export_all = FALSE`), so a missing `export()` fails right
#     here — the class of defect plain `source()` can never see, because
#     `source()` bypasses `NAMESPACE` entirely.
#
#   * Under `R CMD check`, from `<pkg>.Rcheck/tests/`, where `r-pkg/god` does not
#     exist and the package has *already been installed* into the check library.
#     There, loading the installed copy is not a fallback but the point: it is
#     the thing being checked.
#
# **Testing for pkgload before testing for the repository is what broke this**,
# and it broke it everywhere at once: the farm's first build of god failed on all
# eight targets with pkgload's "does your project have a DESCRIPTION file?",
# which says nothing about god and points at no fix. The package had installed
# cleanly on every one of them.
pkg_src <- "r-pkg/god"
if (dir.exists(pkg_src) && requireNamespace("pkgload", quietly = TRUE)) {
  pkgload::load_all(pkg_src, export_all = FALSE, quiet = TRUE)
} else if (dir.exists(pkg_src)) {
  # No pkgload: source the files, and say so, because this path does not honor
  # NAMESPACE and a green run under it proves less.
  message("note: pkgload is not installed, so NAMESPACE is not being honored")
  for (f in list.files(file.path(pkg_src, "R"), pattern = "\\.R$", full.names = TRUE)) source(f)
} else {
  library(god)
}

passed <- 0
failed <- 0

check <- function(label, actual, expected) {
  if (isTRUE(all.equal(actual, expected))) {
    passed <<- passed + 1
    cat(sprintf("  ok    %s\n", label))
  } else {
    failed <<- failed + 1
    cat(sprintf("  FAIL  %s\n        wanted: %s\n        got:    %s\n",
                label, paste(format(expected), collapse = " "),
                paste(format(actual), collapse = " ")))
  }
}

check_error <- function(label, expr, expected_fragment) {
  message <- tryCatch({ force(expr); NULL }, error = function(e) conditionMessage(e))
  if (is.null(message)) {
    failed <<- failed + 1
    cat(sprintf("  FAIL  %s\n        it was accepted, and should not have been\n", label))
  } else if (grepl(expected_fragment, message, fixed = TRUE)) {
    passed <<- passed + 1
    cat(sprintf("  ok    %s\n", label))
  } else {
    failed <<- failed + 1
    cat(sprintf("  FAIL  %s\n        wanted a message containing: %s\n        got: %s\n",
                label, expected_fragment, message))
  }
}

# The same fixture the Rust suite uses, so the two can be compared by eye.
sales <- data.frame(
  region  = c("West", "West", "West", "West", "East"),
  product = c("Widget", "Widget", "Gadget", "Gadget", "Widget"),
  revenue = c(100, 200, 300, 150, 500),
  cost    = c(40, 50, 100, 50, 100),
  stringsAsFactors = FALSE
)

cat("\nthe pipeline, end to end\n")

answer <- run("
sales
  then keep where [region] is \"West\"
  then add [margin] as [revenue] - [cost]
  then summarize [margin] as total([margin]), [orders] as row_count() by [product]
  then sort [margin] descending
  then take 10
")

check("columns come back in the grammar's order", names(answer), c("product", "margin", "orders"))
check("it is a data frame",                       is.data.frame(answer), TRUE)
check("two products survive the filter",          nrow(answer), 2L)
check("Gadget totals 300 and sorts first",        answer$margin, c(300, 210))
check("each product had two orders",              as.integer(answer$orders), c(2L, 2L))
check("Gadget first, Widget second",              answer$product, c("Gadget", "Widget"))

cat("\nthe table is found where you are standing\n")

local({
  elsewhere <- data.frame(a = c(3, 1, 2))
  check("a table in scope needs no naming", nrow(run("elsewhere then take 2")), 2L)
})
check("a table can be passed by name",
      run("t then take 1", t = data.frame(a = 1))$a, 1)

cat("\nR's own types reach the grammar\n")

# Checked through what the grammar *does* with each type rather than by reading
# the string the launcher builds. A type that arrived wrongly would still produce
# a plausible-looking string; only running a sentence that depends on it says the
# mapping is right.
typed <- data.frame(
  when  = as.Date(c("2026-01-01", "2026-06-01")),
  flag  = c(TRUE, FALSE),
  label = c("a", "b"),
  n     = c(1.5, 2.5),
  stringsAsFactors = FALSE
)

check("a logical column compares against yes",
      nrow(run("typed then keep where [flag] is yes")), 1L)
check("a text column compares against text",
      nrow(run("typed then keep where [label] is \"a\"")), 1L)
check("a number column compares against a number",
      nrow(run("typed then keep where [n] > 2")), 1L)
check_error("comparing a number column to text is refused",
            run("typed then keep where [n] is \"a\""),
            "can never match")
check_error("totalling a text column is refused",
            run("typed then summarize [x] as total([label])"),
            "works on numbers")

factored <- data.frame(f = factor(c("x", "y")))
check("a factor is text to the grammar",
      nrow(run("factored then keep where [f] is \"x\"")), 1L)

cat("\na refusal is the R error, with its caret\n")

check_error("an unknown column names the nearest one",
            run("sales then keep where [reveune] > 1"),
            "Did you mean `revenue`?")
check_error("and the caret survives the trip",
            run("sales then keep where [reveune] > 1"),
            "^^^^^^^")
check_error("a host habit is answered with the grammar's word",
            run("sales then keep where [region] is 'West'"),
            "Write `\"West\"`")
check_error("a missing table says how to pass one",
            run("no_such_table then take 1"),
            "Pass it by name")
check_error("a window cannot fill a hole, and the repair is named",
            run("sales then fill_missing [revenue] as previous([revenue])"),
            "first_present")

cat("\nthe same pipeline, written as dplyr\n")

check("show_as returns the translation",
      show_as("sales then keep where [region] is \"West\" then take 10"),
      "sales |>\n  filter((region == \"West\")) |>\n  head(10)")

# **Printed once and returned invisibly**, which is the pair Python matches by
# returning a value whose repr is the query and printing nothing. Pinned on both
# sides so a change to either is caught rather than discovered in a rendered
# page showing the same query twice.
check("show_as prints the translation once",
      length(capture.output(show_as("sales then take 1", "sql"))),
      3L)
check("show_as returns invisibly",
      withVisible(show_as("sales then take 1", "sql"))$visible,
      FALSE)

# **Every table a join names is described**, not only the head. The second is
# found in scope the way the first is, because `show_as` resolves the same
# names `run` does; treating the grammar's answer as one name broke on the
# first sentence that had two.
products <- data.frame(
  product = c("Widget", "Gadget"),
  maker   = c("Acme", "Bolt"),
  stringsAsFactors = FALSE
)
check("show_as describes every table a join names",
      show_as("sales then join products by [product] then take 3"),
      "sales |>\n  left_join(products, by = join_by(product)) |>\n  head(3)")

cat("\nwhat a pipeline does to the table, drawn\n")

# **Nothing here runs the pipeline**, and that is the point of the feature: the
# grammar reads the whole sentence against the columns before anything executes,
# so the drawing is available while the answer is not.

steps <- show_steps("sales then summarize [gross] as total([revenue]) by [region]")

check("a column the step makes is marked",
      grepl("+gross", format(steps, "text"), fixed = TRUE),
      TRUE)
check("and the ones it takes away are marked where they leave",
      grepl("-product", format(steps, "text"), fixed = TRUE),
      TRUE)

# **The drawing resolves the same names `show_as` does**, because both ask the
# one lookup. A join names a second table, and the drawing gives it a row of its
# own under the step that reads it.
check("a table joining in gets a row of its own",
      grepl("└ products", format(show_steps("sales then join products by [product]"), "text"), fixed = TRUE),
      TRUE)

# **A refused sentence is still drawn**, which is the thing an error message on
# its own cannot do: it says how far the pipeline got, and where the column went.
refused <- format(
  show_steps("sales then summarize [g] as total([revenue]) by [region] then keep where [product] is \"x\""),
  "text"
)
check("a refused sentence is drawn as far as it checked",
      grepl("+g", refused, fixed = TRUE),
      TRUE)
check("and carries the refusal under the words that stopped it",
      grepl("^^", refused, fixed = TRUE),
      TRUE)

# **The console gets the ladder and a page gets the picture**, and neither is an
# argument anybody passes: the method that draws is registered for knitr only.
check("the picture is a whole document",
      substr(format(steps, "svg"), 1, 5),
      "<svg ")
check("printing a drawing prints the ladder, not the picture",
      any(grepl("summarize", capture.output(print(steps)), fixed = TRUE)),
      TRUE)
check("a way of drawing that does not exist is refused",
      tryCatch({ format(steps, "picture"); "no refusal" },
               error = function(e) "refused"),
      "refused")

cat("\nthe verbs write the grammar's own sentence\n")

# **Asserted on the text rather than on the rows**, deliberately. A verb's whole
# job is to write a sentence; checking the table it eventually produces would
# pass just as happily on a sentence that meant something else and happened to
# agree on this fixture.

check("keep translates R's equality",
      format(sales |> keep(region == "West")),
      "sales\n  then keep where ([region] is \"West\")")

check("pick names its columns",
      format(sales |> pick(product, revenue)),
      "sales\n  then pick [product, revenue]")

check("all_but inverts the list rather than adding a verb",
      format(sales |> pick(all_but(cost))),
      "sales\n  then pick all_but [cost]")

check("add names the column it makes",
      format(sales |> add(margin = revenue - cost)),
      "sales\n  then add [margin] as ([revenue] - [cost])")

check("summarize carries its grouping",
      format(sales |> summarize(total = total(revenue), by = product)),
      "sales\n  then summarize [total] as total([revenue]) by [product]")

check("several grouping columns are written as a list",
      format(sales |> summarize(n = row_count(), by = c(region, product))),
      "sales\n  then summarize [n] as row_count() by [region, product]")

check("descending is a modifier on a column",
      format(sales |> sort(descending(revenue), cost)),
      "sales\n  then sort [revenue] descending, [cost]")

check("take counts rows",
      format(sales |> take(3)), "sales\n  then take 3")

check("the steps chain in the order they were written",
      format(sales |> keep(revenue > 100) |> take(2)),
      "sales\n  then keep where ([revenue] > 100)\n  then take 2")

cat("\nR's habits become the grammar's words\n")

# Each of these is a token that means something different in R, Python and SQL,
# so each one had to be replaced rather than passed through (§2.4). They are
# checked one at a time because a translator that got one wrong would still
# produce a sentence that parses.

written <- function(expr) sub("^sales\n  then keep where ", "", format(expr))

check("!= becomes is not",        written(sales |> keep(region != "West")), "([region] is not \"West\")")
check("& becomes and",            written(sales |> keep(revenue > 1 & cost > 1)), "(([revenue] > 1) and ([cost] > 1))")
check("| becomes or",             written(sales |> keep(revenue > 1 | cost > 1)), "(([revenue] > 1) or ([cost] > 1))")
check("! becomes not",            written(sales |> keep(!(region == "West"))), "(not ([region] is \"West\"))")
check("%in% becomes a set",       written(sales |> keep(region %in% c("West", "East"))), "([region] in {\"West\", \"East\"})")
check("a negated %in% is not in", written(sales |> keep(!(region %in% c("West")))), "([region] not in {\"West\"})")
check("is.na becomes is missing", written(sales |> keep(is.na(cost))), "([cost] is missing)")
check("!is.na has its own words", written(sales |> keep(!is.na(cost))), "([cost] is not missing)")
check("TRUE becomes yes",         written(sales |> keep(region == TRUE)), "([region] is yes)")
check("FALSE becomes no",         written(sales |> keep(region == FALSE)), "([region] is no)")
check("NA becomes missing",       written(sales |> keep(region == NA)), "([region] is missing)")

check("a large number is never written in scientific notation",
      written(sales |> keep(revenue > 100000)), "([revenue] > 100000)")

cat("\nnothing runs until the answer is wanted\n")

check("a verb returns a pipeline, not a table",
      is.data.frame(sales |> take(1)), FALSE)
check("and it says what it is",
      inherits(sales |> take(1), "god_pipeline"), TRUE)
check("collect runs it",
      nrow(collect(sales |> take(2))), 2L)
check("converting runs it too",
      is.data.frame(as.data.frame(sales |> take(2))), TRUE)

# A backticked name may hold a space, and the launcher has to carry it whole:
# `system2` quotes nothing on its own, so the schema used to split at the
# space and the engine answered with its usage text instead of the rows.
spaced <- data.frame(`order date` = c("2026-01-02", "2026-01-05"),
                     total = c(40, 90), check.names = FALSE)
check("a column named with a space survives the launcher",
      collect(spaced |> keep(`order date` > "2026-01-03"))$total, 90)

check("the rows are the ones the text form gives",
      collect(sales |> keep(region == "West") |> sort(descending(revenue)) |> take(2))$revenue,
      run("sales then keep where [region] is \"West\" then sort [revenue] descending then take 2")$revenue)

cat("\nthe R form and the text form are the same sentence\n")

# The third witness (§13.2), asserted here as well as in the parity harness so
# that the R suite alone can catch a translator that has drifted.
same_query <- function(native, text) {
  columns <- god:::columns_of(sales)
  god_sql(format(native), columns) == god_sql(text, columns)
}

check("a filter agrees",
      same_query(sales |> keep(region == "West"), "sales then keep where [region] is \"West\""), TRUE)
check("a grouped summary agrees",
      same_query(sales |> summarize(total = total(revenue), by = product),
                 "sales then summarize [total] as total([revenue]) by [product]"), TRUE)
check("a sort agrees",
      same_query(sales |> sort(descending(revenue), cost),
                 "sales then sort [revenue] descending, [cost]"), TRUE)

cat("\none value applied to every column that matches\n")

survey <- data.frame(
  respondent = c(1, 2),
  q1_score   = c(4, 5),
  q2_score   = c(2, 5),
  region     = c("West", "East"),
  stringsAsFactors = FALSE
)

check("add writes the pattern and the value",
      format(survey |> add(where(startsWith(name, "q"), value * 2))),
      "survey\n  then add where (name starts \"q\") as (value * 2)")
check("the matched columns keep their names",
      names(collect(survey |> add(where(startsWith(name, "q"), value * 2)))),
      c("respondent", "region", "q1_score", "q2_score"))
check("and every one of them was doubled",
      collect(survey |> add(where(startsWith(name, "q"), value * 2)))$q1_score,
      c(8, 10))
check("summarize takes the same shape",
      collect(survey |> summarize(where(endsWith(name, "_score"), average(value))))$q2_score,
      3.5)
check("and it groups",
      nrow(collect(survey |> summarize(where(endsWith(name, "_score"), average(value)), by = region))),
      2L)

# Same split as `name`: R marks `value` only inside `where(...)`, so everywhere
# else it is an ordinary column reference. That is what keeps the check below
# true, and Python answers this one with the core's refusal instead.
check_error("outside where, a bare value is a column like any other",
            collect(survey |> add(x = value * 2)),
            "there is no column called `value`")
check_error("a pattern matching nothing makes nothing",
            collect(survey |> add(where(startsWith(name, "zzz"), value * 2))),
            "no column's name matches")
check_error("where has to say what to make of each column",
            survey |> add(where(startsWith(name, "q"))),
            "what to make of each column")
check("a column really called value is still reachable",
      nrow(collect(data.frame(value = c(1, 9)) |> keep(value > 5))), 1L)

cat("\nchoosing columns by the shape of their name\n")

wide <- data.frame(q1 = 1, q2 = 2, region = "W", revenue = 3, stringsAsFactors = FALSE)

check("pick where writes a question about a name",
      format(wide |> pick(where(startsWith(name, "q")))),
      "wide\n  then pick where (name starts \"q\")")
check("and the columns that matched come back",
      names(collect(wide |> pick(where(startsWith(name, "q"))))),
      c("q1", "q2"))
check("it joins with or",
      names(collect(wide |> pick(where(startsWith(name, "q") | name == "region")))),
      c("q1", "q2", "region"))
check("and with not",
      names(collect(wide |> pick(where(!startsWith(name, "q"))))),
      c("region", "revenue"))
check("ends and contains work on a name too",
      names(collect(wide |> pick(where(endsWith(name, "1") | grepl("ven", name, fixed = TRUE))))),
      c("q1", "revenue"))

check("columns can be chosen by what they hold",
      names(collect(wide |> pick(where(kind == "number")))),
      c("q1", "q2", "revenue"))
check("and by what they do not hold",
      names(collect(wide |> pick(where(kind != "number")))),
      "region")
check("kind and name join, which is the point of the where",
      names(collect(wide |> pick(where(kind == "number" & startsWith(name, "q"))))),
      c("q1", "q2"))
check("one aggregation over every number, whatever they are called",
      names(collect(wide |> summarize(where(kind == "number", average(value))))),
      c("q1", "q2", "revenue"))
check_error("a kind the grammar does not have lists the ones it does",
            collect(wide |> pick(where(kind == "numeric"))),
            "`number`")

# The name tests are case-sensitive, and folding the case is how you ask for
# either. That is two words the vocabulary already had rather than a flag.
mixed <- data.frame(Q1 = 1, q2 = 2, Region = "x", stringsAsFactors = FALSE)
check("a name test is case-sensitive on its own",
      names(collect(mixed |> pick(where(startsWith(name, "q"))))), "q2")
check("and folding the case catches both",
      names(collect(mixed |> pick(where(startsWith(tolower(name), "q"))))),
      c("Q1", "q2"))
check("the same two words fold a value",
      nrow(collect(mixed |> keep(tolower(Region) == "x"))), 1L)
check_error("only text has a case",
            collect(mixed |> keep(tolower(q2) == "x")),
            "Only text has a case")

check("the same three words test a value, with the subject written",
      format(wide |> keep(startsWith(region, "W"))),
      "wide\n  then keep where ([region] starts \"W\")")
check("and they run",
      nrow(collect(wide |> keep(grepl("e", region, fixed = TRUE)))), 0L)

# **R and Python differ here, and both are right.** `where()` has its own reader
# in R, so a bare `name` anywhere else is an ordinary column reference, which is
# what keeps a column actually called `name` reachable. Python has no such
# reader, so `name` there is the keyword object and the core refuses it outside
# a `pick`.
check_error("outside where, a bare name is a column like any other",
            collect(wide |> keep(startsWith(name, "q"))),
            "there is no column called `name`")
check("and a column really called name is reachable",
      names(collect(data.frame(name = "x", n = 1) |> keep(startsWith(name, "x")))),
      c("name", "n"))
check_error("a pattern matching nothing is refused",
            collect(wide |> pick(where(startsWith(name, "zzz")))),
            "no column's name matches")
check_error("where chooses on its own",
            wide |> pick(where(startsWith(name, "q")), region),
            "nothing goes beside it")

cat("\nthe first column that has a value\n")

patchy_three <- data.frame(a = c(1, NA, NA), b = c(NA, 2, NA), c = c(9, 9, 9))

check("it reads left to right and takes the first one present",
      collect(patchy_three |> add(best = first_present(a, b, c)))$best,
      c(1, 2, 9))
check("order is priority, so swapping the arguments changes the answer",
      collect(patchy_three |> add(best = first_present(c, a, b)))$best,
      c(9, 9, 9))
check("a zero is present, and only missing is skipped",
      collect(data.frame(a = c(0, NA), b = c(5, 5)) |> add(best = first_present(a, b)))$best,
      c(0, 5))
check("all missing leaves it missing",
      is.na(collect(data.frame(a = NA_real_, b = NA_real_) |> add(best = first_present(a, b)))$best),
      TRUE)

check_error("one column is not a choice",
            collect(patchy_three |> add(best = first_present(a))),
            "at least 2 columns")
check_error("the columns have to hold the same kind of thing",
            collect(data.frame(a = 1, b = "x") |> add(best = first_present(a, b))),
            "same kind of thing")

cat("\na place is worked out over the rows, not for one of them\n")

ranked <- data.frame(
  heat  = c("x", "x", "y", "y"),
  name  = c("a", "b", "c", "d"),
  score = c(20, 20, 5, 50),
  stringsAsFactors = FALSE
)

check("rank writes an ordering key, not a value",
      format(ranked |> add(place = rank(descending(score)))),
      "ranked\n  then add [place] as rank([score] descending)")
check("ties share a place and the next one skips",
      collect(ranked |> add(place = rank(score)) |> sort(name))$place,
      c(2, 2, 1, 4))
check("a group restarts the numbering",
      collect(ranked |> add(place = rank(descending(score)), by = heat) |> sort(name))$place,
      c(1, 1, 2, 1))
check("row_number never ties where rank does",
      collect(ranked |> sort(score) |> add(n = row_number()) |> sort(n))$n,
      c(1, 2, 3, 4))

check_error("row_number without a sort says what to write",
            collect(ranked |> add(n = row_number())),
            "nothing has said what that order is")
check_error("a window cannot choose the rows it is computed over",
            collect(ranked |> keep(rank(score) <= 2)),
            "cannot be what chooses them")
check_error("a window in a summarize is refused in its own words",
            collect(ranked |> summarize(p = rank(score), by = heat)),
            "nowhere to go")

cat("\na filtering join reads a second table from inside a condition\n")

# `matching` is the only expression that names a table, so the verb has to
# notice it and hand that table over. Nothing else in a sentence reaches outside
# the table at its head, which is why this is worth its own section.
catalog <- data.frame(
  product = c("Widget", "Gizmo"),
  maker   = c("Acme", "Globex"),
  stringsAsFactors = FALSE
)

check("a semi join keeps only the rows with a partner",
      collect(sales |> keep(matching(catalog, by = product)))$product,
      c("Widget", "Widget", "Widget"))
check("an anti join keeps exactly the others",
      unique(collect(sales |> keep(!matching(catalog, by = product)))$product),
      "Gadget")
check("the table travels with the pipeline",
      "catalog" %in% names(.subset2(sales |> keep(matching(catalog, by = product)), "tables")),
      TRUE)

# **A pipeline refuses a table's questions.** R's own answer for each would
# be NULL or the plan's internals, silently; `sum(p$revenue)` would be 0.
# Python raises a TypeError for the same misuse without being asked, so the
# two languages fail the same slip the same loud way.
check_error("a column cannot be taken from a plan",
            (sales |> take(1))$revenue,
            "collect")
check_error("a plan has no row count",
            nrow(sales |> take(1)),
            "collect")
check_error("a plan does not name its columns",
            names(sales |> take(1)),
            "collect")
check_error("a plan cannot be subset",
            (sales |> take(1))[1, ],
            "collect")
check("the key can be left to the shared names",
      nrow(collect(sales |> keep(matching(catalog)))), 3L)
check("a filtering join adds no columns",
      names(collect(sales |> keep(matching(catalog, by = product)))),
      names(sales))

check_error("matching cannot be half of a bigger question",
            collect(sales |> keep(matching(catalog, by = product) & revenue > 100)),
            "its own step")
check_error("matching needs a table rather than a value",
            sales |> keep(matching("catalog")),
            "matching(products, by = id)")

cat("\ndates, and looking along the rows\n")

diary <- data.frame(
  g = c("a", "a", "b"),
  on_ = c("2026-01-02", "2026-01-05", "2026-01-06"),
  x = c(10, 20, 30),
  stringsAsFactors = FALSE
)

dated <- collect(diary |> add(d = to_date(on_)) |> add(y = year(d), m = month(d), wd = weekday(d)))
check("year and month read what they say", c(dated$y[1], dated$m[1]), c(2026, 1))
# Monday is 1, and it is the grammar's numbering rather than the engine's: asked
# plainly, the two engines disagree about this Friday and neither complains.
check("weekday counts Monday as 1", dated$wd, c(5, 1, 2))

check_error("a date part refuses a number and names the conversion",
            collect(diary |> add(y = year(x))), "to_date(...)")

running <- collect(diary |> sort(on_) |> add(so_far = running_total(x)))
check("the running total adds up as it goes", running$so_far, c(10, 30, 60))

grouped <- collect(diary |> sort(on_) |> add(so_far = running_total(x), by = g))
check("by restarts it", grouped$so_far, c(10, 30, 30))
# Computing a window regroups the rows, so the sort has to be said again in the
# query or the same sentence comes back in different orders on different engines.
check("and the order that was asked for survives", grouped$on_,
      c("2026-01-02", "2026-01-05", "2026-01-06"))

steps <- collect(diary |> sort(on_) |> add(before = previous(x), after = following(x)))
check("previous looks one row back", steps$before, c(NA, 10, 20))
check("following looks one row on", steps$after, c(20, 30, NA))

check_error("a window that is not told an order needs a sort",
            collect(diary |> add(v = running_total(x))),
            "nothing has said what that order is")

cat("\nconverting, text, and between\n")

# `n` is written as whole numbers on purpose. R's `c(7, 99)` is a double, so
# `to_text` would give "7.0" rather than "7", which is correct and would make the
# character counts below say something about the fixture rather than about the
# functions. The Python suite's fixture is integer for the same reason.
messy <- data.frame(raw = c("  ann marie  ", "  bob  "), n = c(7L, 99L),
                    stringsAsFactors = FALSE)

tidied <- collect(
  messy |>
    add(name = trim(raw)) |>
    add(first = split_text(name, " ", 1), size = characters(name),
        fixed = replace_text(name, "a", "A"))
)
check("trim takes the spaces off both ends", tidied$name, c("ann marie", "bob"))
check("split_text says which piece it wants", tidied$first, c("ann", "bob"))
check("characters counts them", tidied$size, c(9, 3))
check("replace_text looks for text, not a pattern", tidied$fixed, c("Ann mArie", "bob"))

check("between counts both ends",
      collect(messy |> keep(between(n, 7, 99)))$n, c(7, 99))
check("and nothing is between the ends when they exclude everything",
      nrow(collect(messy |> keep(between(n, 8, 98)))), 0L)

check("a conversion says what it gives",
      collect(messy |> add(word = to_text(n)) |> add(len = characters(word)))$len,
      c(1, 2))

check_error("a text function refuses a number and names the conversion",
            collect(messy |> add(x = trim(n))), "to_text(...)")
check_error("between needs all three to be the same kind of thing",
            collect(messy |> keep(between(n, 1, "ten"))), "same kind of thing")

cat("\nthe conditional\n")

pupils <- data.frame(name = c("ann", "bob", "cat"), score = c(95, 75, 50),
                     stringsAsFactors = FALSE)

banded <- collect(pupils |> add(band = when(score >= 90, "A", score >= 70, "B", otherwise = "C")))
check("the first question that is true wins", banded$band, c("A", "B", "C"))

# Order is the meaning, and this is the thing people get wrong about a
# conditional, so it is asserted rather than left implied.
check("so the same questions the other way round answer differently",
      collect(pupils |> add(band = when(score >= 70, "B", score >= 90, "A", otherwise = "C")))$band,
      c("B", "B", "C"))

check("a row matching nothing is missing without an otherwise",
      collect(pupils |> add(top = when(score >= 90, "yes")))$top,
      c("yes", NA, NA))

check_error("every answer has to be the same kind of thing",
            collect(pupils |> add(band = when(score >= 90, "A", otherwise = 0))),
            "same kind of thing")
check_error("a question with no answer beside it is refused",
            pupils |> add(band = when(score >= 90, "A", score >= 70)),
            "needs the answer that goes with it")
check_error("and a named argument that is not otherwise",
            pupils |> add(band = when(score >= 90, "A", else_ = "C")),
            "does not take")

cat("\nreshaping, in both directions\n")

# A survey in the shape people actually receive one: a row per person, a column
# per question.
answers <- data.frame(
  student = c("ann", "bob"),
  q1 = c(1, 4), q2 = c(2, 5), q3 = c(3, 6),
  stringsAsFactors = FALSE
)

tall <- collect(answers |> lengthen(q1, q2, q3))
check("the two new columns take the grammar's own words",
      names(tall), c("student", "name", "value"))
check("every column becomes a row",       nrow(tall), 6L)
check("each row's answers stay together", tall$name[1:3], c("q1", "q2", "q3"))
check("and carry their values",           tall$value[1:3], c(1, 2, 3))

check("the two verbs are inverses, spelled with nothing at all",
      collect(answers |> lengthen(q1, q2, q3) |> widen()),
      answers[order(answers$student), ])

check("all_but chooses the same columns as listing them",
      collect(answers |> lengthen(all_but(student))), tall)
check("and so does a question about the name",
      collect(answers |> lengthen(where(startsWith(name, "q")))), tall)

# Names that hold two things, which is where `pivot_longer` stops being easy.
terms <- data.frame(id = 1, q1_2020 = 10, q1_2021 = 11, stringsAsFactors = FALSE)
split <- collect(terms |> lengthen(all_but(id), name = "{question}_{year}", value = answer))
check("a pattern splits one name into two columns",
      names(split), c("id", "question", "year", "answer"))
check("and the pieces are the pieces",  split$year, c("2020", "2021"))

wide <- collect(
  answers |>
    lengthen(q1, q2, q3) |>
    widen(name = name, value = value, by = student, giving = c(q1, q2, q3)) |>
    add(gain = q3 - q1)
)
check("a widen that says what it makes can be carried on from",
      names(wide), c("student", "q1", "q2", "q3", "gain"))
check("and the arithmetic after it is real", wide$gain, c(2, 2))

check_error("stacking two kinds of column is refused",
            collect(answers |> lengthen(student, q1)),
            "two kinds of thing in one column")
check_error("a step after a widen that declares nothing is refused",
            collect(answers |> lengthen(q1, q2, q3) |> widen() |> take(1)),
            "giving [q1, q2, q3]")
check_error("lengthen needs the columns that become rows",
            answers |> lengthen(),
            "lengthen(q1, q2, q3)")

cat("\nthe rows at the far end\n")

ends <- data.frame(
  region  = c("West", "East", "West", "East"),
  revenue = c(100, 120, 150, 90),
  stringsAsFactors = FALSE
)
last_two <- collect(ends |> sort(revenue) |> take_last(2))
check("take_last gives the far end, in the order the sort asked for",
      last_two$revenue, c(120, 150))
check("and take still gives the near end", collect(ends |> sort(revenue) |> take(2))$revenue,
      c(90, 100))
check("by takes the last of each group",
      collect(ends |> sort(revenue) |> take_last(1, by = region) |> sort(region))$revenue,
      c(120, 150))
check_error("take_last needs a sort even ungrouped",
            collect(ends |> take_last(2)),
            "nothing has said which end that is")
check_error("and it names take as the verb that does not",
            collect(ends |> take_last(2)),
            "`take` needs no sort")
check_error("take_last wants a whole number",
            ends |> take_last("two"),
            "whole number of rows")

cat("\nthe combinations that are not there\n")

# One region sells one product and the other sells two, so exactly one pair of
# the four is absent, and it can be named rather than counted.
shop <- data.frame(
  region  = c("West", "West", "East"),
  product = c("Widget", "Gadget", "Widget"),
  revenue = c(100, 200, 300),
  stringsAsFactors = FALSE
)

filled <- collect(shop |> add_combinations(region, product))
check("the pair that was absent arrives", nrow(filled), 4L)
check("and the rows that were there are untouched, in order",
      filled$revenue[1:3], c(100, 200, 300))
check("the new row is East and Gadget",
      c(filled$region[4], filled$product[4]), c("East", "Gadget"))
check("and it holds nothing anywhere else", is.na(filled$revenue[4]), TRUE)

check("what a new row holds is a second step",
      collect(shop |> add_combinations(region, product) |> fill_missing(revenue = 0))$revenue,
      c(100, 200, 300, 0))

# `by` holds a column fixed and crosses inside it. ann sat two questions and
# bob one, both at the same school, so bob gains q2 and nobody crosses schools.
sittings <- data.frame(
  school   = c("east", "east", "east", "west"),
  student  = c("ann", "ann", "bob", "cal"),
  question = c("q1", "q2", "q1", "q3"),
  stringsAsFactors = FALSE
)
inside <- collect(sittings |> add_combinations(student, question, by = school))
check("by crosses inside each group only", nrow(inside), 5L)
check("and the new row keeps its group rather than going missing",
      c(inside$school[5], inside$student[5], inside$question[5]),
      c("east", "bob", "q2"))

check_error("one column has no combinations to make",
            collect(shop |> add_combinations(region)),
            "two columns or more")
check_error("and a column cannot be crossed and held fixed at once",
            collect(shop |> add_combinations(region, product, by = region)),
            "crossed and held fixed")
check_error("add_combinations needs the columns whose combinations to make",
            shop |> add_combinations(),
            "add_combinations(region, product)")

# The verb was named twice: "`sort` names a column in sort". `where` is already
# the verb, so saying it once is the whole fix.
check_error("a column position names the verb once, not twice",
            collect(sales |> sort(3.14)),
            "`sort` names a column, and `3.14` is not a column name")

cat("\nthe grammar still owns every refusal\n")

check_error("an unknown column is caught by the grammar, not the verbs",
            collect(sales |> keep(reveune > 1)),
            "Did you mean `revenue`?")
check_error("a summarize that does not aggregate is refused",
            collect(sales |> summarize(x = revenue)),
            "summarize")

cat("\nmasking is paid for with a message\n")

check_error("sort on something that is not a table names base::sort",
            sort(1:10),
            "base::sort(1:10)")
check_error("add names the column it makes",
            sales |> add(revenue - cost),
            "add(margin = revenue - cost)")
check_error("pick takes columns, not expressions",
            sales |> pick(revenue + cost),
            "is not a column name")
check_error("all_but wants all its columns inside it",
            sales |> pick(all_but(cost), region),
            "pick(all_but(cost, region))")
check_error("take wants a whole number",
            sales |> take(2.5),
            "take(10)")
check_error("a set has to be written out, not named",
            sales |> keep(region %in% wanted),
            "Write them out")

cat("\na pipeline printed in a document is a table, not console text\n")

if (requireNamespace("knitr", quietly = TRUE)) {
  # Registered into knitr's own methods table rather than declared in NAMESPACE,
  # because knitr is suggested and not required. `getS3method` does not consult
  # another package's table, so asking it here reports FALSE on a method that is
  # registered and dispatching. Ask the table.
  check("knit_print is registered with knitr for a pipeline",
        exists("knit_print.god_pipeline",
               envir = get(".__S3MethodsTable__.", envir = asNamespace("knitr")),
               inherits = FALSE),
        TRUE)
  # **Delegating is the assertion, not the format.** The document decides how a
  # table is printed, through `df-print`, and a pipeline that chose for itself
  # would be the one table on the page ignoring the setting. So what is checked
  # is that a pipeline prints exactly as its own table would.
  answer <- sales |> keep(region == "West") |> take(2)
  invisible(capture.output(
    same <- identical(knitr::knit_print(answer), knitr::knit_print(collect(answer)))
  ))
  check("and it prints exactly as the table it returns would", same, TRUE)
} else {
  cat("  note: knitr is not installed, so the document printing is untested\n")
}

cat("\nwhich engine answers\n")

# The order is the contract, and it is the same in Python: GOD_CLI, then a
# source tree's own build, then the bundled copy, then the working directory's
# tree, then the PATH. The second check only means something where a source
# tree exists; under `R CMD check` the installed copy's bundled engine is the
# right answer, so the check is skipped there rather than failed.
tmp_engine <- tempfile()
file.create(tmp_engine)
old_god_cli <- Sys.getenv("GOD_CLI", unset = NA)
Sys.setenv(GOD_CLI = tmp_engine)
check("an explicit GOD_CLI outranks everything", god:::god_binary(), tmp_engine)
if (is.na(old_god_cli)) Sys.unsetenv("GOD_CLI") else Sys.setenv(GOD_CLI = old_god_cli)
if (!is.null(god:::god_walk_up(dirname(god:::god_source_dir())))) {
  check("a source tree's build outranks a bundled copy",
        grepl(file.path("target", "release"), god:::god_binary(), fixed = TRUE),
        TRUE)
} else {
  cat("  ok    (no source tree here; the bundled engine is the right answer)\n")
  passed <- passed + 1
}
unlink(tmp_engine)

# knitr stays optional: `.onLoad` registers `knit_print` only where knitr
# exists, and an `S3method(knit_print, ...)` line in NAMESPACE would make it a
# load-time dependency. A default `roxygenise()` once nearly wrote that line,
# from a stray `@export` tag that is deleted now; this pins the door shut.
check("NAMESPACE never registers knit_print",
      any(grepl("knit_print",
                readLines(system.file("NAMESPACE", package = "god")))), FALSE)

cat("\nthe book is held to the grammar it documents\n")

# Sourced rather than restated, so there is one copy of each rule. Both guards
# `stop()` when they find something, which is right when they are run alone and
# wrong here, so each is caught and counted like any other check.
book_guard <- function(label, file, name) {
  if (!file.exists(file)) {
    cat(sprintf("  skip  %s (not run from the repository root)\n", label))
    return(invisible(NULL))
  }
  # Its own environment, so the guard's helpers cannot collide with anything
  # here. Sourcing from inside a function also leaves `sys.nframe()` above zero,
  # which is what stops each file's standalone runner from firing twice.
  env <- new.env(parent = globalenv())
  source(file, local = env)
  # **`capture.output` wraps the `tryCatch`, not the other way round.** Wrapped
  # the other way an error unwinds through `capture.output`, which discards
  # everything the guard printed on its way down, so a failure arrives as its
  # one-line condition and nothing else. Every guard here names the chunk, the
  # file and the line in that discarded text. Locally it costs nothing, because
  # you rerun the guard on its own; in CI there is no second run, and a red job
  # said only "1 chunk(s) wrong" across a book of 51 files.
  outcome <- NULL
  said <- capture.output(
    outcome <- tryCatch({ get(name, envir = env)("book"); NULL },
                        error = function(e) conditionMessage(e))
  )
  if (is.null(outcome)) {
    passed <<- passed + 1
    cat(sprintf("  ok    %s\n", label))
  } else {
    failed <<- failed + 1
    cat(sprintf("  FAIL  %s\n        %s\n", label, outcome))
    for (line in said) cat("        ", line, "\n", sep = "")
  }
}

book_guard("every pipeline the book shows, the grammar reads",
           "book/check_grammar.R", "check_grammar")
book_guard("the book's voice is consistent",
           "book/check_prose.R", "check_prose")
# **The manual documenting the whole grammar is a test, not a hope.** When the
# book was split into chapters, several words turned out to be demonstrated
# nowhere. Prose can name a verb and still build clean, and a grammar can grow a
# word without any page mentioning it.
book_guard("every word the grammar has, the book demonstrates",
           "book/check_vocabulary.R", "check_vocabulary")
# **`error: true` tolerates an error; it never asserts one.** So a chunk the book
# presents as a refusal can quietly stop refusing, render a table, and the build
# still exits 0. A pipeline is lazy, so this one has to force each chunk or it
# would report every refusal as passing while testing none.
book_guard("every refusal the book shows, the grammar makes",
           "book/check_refusals.R", "check_refusals")
book_guard("every tabset holds one sentence in both languages",
           "book/check_tabs.R", "check_tabs")
book_guard("every verb chapter follows the template",
           "book/check_template.R", "check_template")
book_guard("the book keeps the rules its preface states",
           "book/check_promises.R", "check_promises")
# **The only guard that reads the render rather than the source.** Every other
# one reads `.qmd`, so the defects pandoc and Quarto introduce on the way out
# were caught by the publishing workflow alone, which is the far side of the
# push that publishes them. It skips, loudly, where there is no render or where
# the render is older than the sources, because grading a stale one is grading a
# book nobody has.
book_guard("the rendered book carries no defect its sources hide",
           "book/check_render.R", "check_render")

# **A guard nobody runs is worse than no guard, because it reads as coverage.**
# `check_vocabulary.R` sat on disk, complete and invoked by nothing, and looked
# exactly like coverage until a survey caught it. So the checkers are
# themselves checked: every `book/check_*` file has to be named by one of the
# two suites, which are the places a guard is actually run from.
if (dir.exists("book")) {
  checkers <- list.files("book", pattern = "^check_.*[.](R|py)$")
  suites <- c(readLines("r-pkg/god/tests/test_basic.R", warn = FALSE),
              readLines("py-pkg/god/tests/test_basic.py", warn = FALSE))
  orphaned <- Filter(function(f) !any(grepl(f, suites, fixed = TRUE)), checkers)
  check("no book guard sits unwired", orphaned, character(0))
} else {
  cat("  skip  no book guard sits unwired (not run from the repository root)\n")
}

# **Every exported name has a help page, and this exists because three did
# not.** `?add_combinations`, `?show_steps` and `?format.god_steps` all failed
# on an installed copy: the roxygen comment was written in the same change as
# the verb, `NAMESPACE` was edited by hand, and `man/` is *generated*, so
# nothing regenerated it and nothing noticed. Every other check in this project
# holds one copy of a fact against another; this is the copy that had none.
#
# The failure is invisible from the source tree. `pkgload::load_all` needs no
# `.Rd` at all, so the whole suite, the whole book and every parity harness run
# green over a verb a user cannot look up.
if (file.exists("r-pkg/god/NAMESPACE")) {
  exported <- sub("^export\\((.*)\\)$", "\\1",
                  grep("^export\\(", readLines("r-pkg/god/NAMESPACE", warn = FALSE),
                       value = TRUE))
  # An S3 method is documented under the generic it belongs to rather than
  # under its own name, so those are asked for by the file roxygen writes.
  methods <- sub("^S3method\\((.*),(.*)\\)$", "\\1.\\2",
                 grep("^S3method\\(", readLines("r-pkg/god/NAMESPACE", warn = FALSE),
                      value = TRUE))
  documented <- sub("[.]Rd$", "", list.files("r-pkg/god/man", pattern = "[.]Rd$"))
  # A page may document several names through `@rdname`, so the alias is what
  # settles it rather than the filename.
  aliases <- unlist(lapply(
    list.files("r-pkg/god/man", pattern = "[.]Rd$", full.names = TRUE),
    function(f) sub("^\\\\alias\\{(.*)\\}$", "\\1",
                    grep("^\\\\alias\\{", readLines(f, warn = FALSE), value = TRUE))))
  undocumented <- setdiff(exported, union(documented, aliases))
  check("every exported name has a help page", undocumented, character(0))

  # **And every page's example is run**, which is the half that decides whether
  # the page is worth having. `R CMD check` runs `\examples{}` and CI does not
  # run `R CMD check`, so without this the examples are read and never executed
  # — the exact shape of defect the book's live chunks exist to refuse.
  #
  # `\dontrun{}` is honored rather than stripped: `use_engine`'s example opens
  # a connection and `god_table`'s reaches the network, and a suite has to pass
  # on a laptop with neither.
  ran <- 0L
  broke <- character()
  for (page in list.files("r-pkg/god/man", pattern = "[.]Rd$", full.names = TRUE)) {
    parsed <- tools::parse_Rd(page)
    code <- character()
    for (piece in parsed) {
      if (identical(attr(piece, "Rd_tag"), "\\examples")) {
        code <- c(code, paste(
          vapply(piece, function(x)
            if (identical(attr(x, "Rd_tag"), "\\dontrun")) "" else paste(unlist(x), collapse = ""),
            character(1)),
          collapse = ""))
      }
    }
    code <- paste(code, collapse = "\n")
    if (!nzchar(trimws(code))) next
    # **The `#>` lines are compared, not decorated.** An example that shows an
    # answer is making a claim about what the engine returns today, and a claim
    # nothing checks is the species of defect every guard here exists for. This
    # is R's half of what doctest does on the Python side, and the two pages
    # show the same table so a drift on either side is visible on both.
    outcome <- tryCatch({
      env <- new.env(parent = globalenv())
      parsed <- parse(text = code, keep.source = TRUE)
      claimed <- sub("^#> ?", "", grep("^#>", strsplit(code, "\n")[[1]], value = TRUE))
      shown <- character()
      for (k in seq_along(parsed)) {
        # `capture.output` wraps the evaluation, not only the printing, so a
        # statement that writes with `cat` is caught too. Without that,
        # `god_sql`'s example printed a query into the middle of this suite's
        # own output and nothing compared it.
        shown <- c(shown, capture.output({
          result <- withVisible(eval(parsed[[k]], env))
          if (result$visible) print(result$value)
        }))
      }
      if (length(claimed) && !identical(trimws(claimed), trimws(shown))) {
        sprintf("the answer it shows is not the answer it gives\n        shows: %s\n        gives: %s",
                paste(claimed, collapse = " | "), paste(shown, collapse = " | "))
      } else "ok"
    }, error = function(e) conditionMessage(e))
    if (identical(outcome, "ok")) ran <- ran + 1L
    else broke <- c(broke, sprintf("%s: %s", basename(page), outcome))
  }
  check("every example on every help page runs", broke, character(0))
  cat(sprintf("        (%d pages' examples run)\n", ran))
  # The other direction: a page for something no longer exported is a help
  # topic pointing at a function an installed copy does not have.
  stale <- setdiff(documented, c(exported, methods, aliases, "figures"))
  check("no help page survives the thing it documented", stale, character(0))
} else {
  cat("  skip  every exported name has a help page (not run from the repository root)\n")
}

cat("\nthe guard can fail\n")
local({
  before <- failed
  check("a deliberately wrong expectation fails", 1 + 1, 3)
  if (failed == before) {
    cat("  FAIL  the checker cannot fail, so nothing above is evidence\n")
    failed <<- failed + 1
  } else {
    # Undo the deliberate failure; it was the point.
    failed <<- before
    passed <<- passed + 1
    cat("  ok    (the failure above was deliberate, and the checker caught it)\n")
  }
})

cat("\nrunning somewhere other than this machine\n")

# **A warehouse table is not a local variable and never will be.** Given a
# connection, god asks it for the table's columns instead of looking in the
# caller's scope, and asks for no rows: the shape is all the grammar needs, and
# fetching the table in order to describe it is the one thing this design avoids.
# Checked with duckdb because it is the connection this suite already has; a
# sparklyr or odbc handle takes the same path.
local({
  con <- DBI::dbConnect(duckdb::duckdb())
  on.exit({ use_engine(); DBI::dbDisconnect(con, shutdown = TRUE) }, add = TRUE)
  DBI::dbExecute(con, "CREATE SCHEMA sch")
  DBI::dbExecute(con, "CREATE TABLE sch.orders (product VARCHAR, revenue DOUBLE)")
  DBI::dbExecute(con, "INSERT INTO sch.orders VALUES ('Widget', 100), ('Gadget', 300)")

  use_engine(con, "sql")
  answer <- run("sch.orders then sort [revenue] descending then take 1")
  check("a table named in parts is found in the engine", answer$product, "Gadget")
  check("and it is described without fetching it",      nrow(answer), 1L)

  use_engine()
  sales_here <- data.frame(region = "West", revenue = 10)
  check("and the engine here takes over again",
        nrow(run("sales_here then take 1")), 1L)
})

# The parts of a name are quoted one at a time. Quoting the whole of `sch.orders`
# names a table nobody has, so the query parses and then finds nothing.
local({
  orders <- data.frame(product = "Widget", revenue = 10)
  written <- capture.output(show_as("sch.orders then take 1", "spark",
                                    `sch.orders` = orders))
  joined <- paste(written, collapse = "\n")
  check("each part of a name is its own identifier", grepl("`sch`.`orders`", joined, fixed = TRUE), TRUE)
  check("the whole name is not one identifier",      grepl("`sch.orders`", joined, fixed = TRUE), FALSE)
})


# --- god_table(): the book's tables, fetched by name -------------------------
# Binding plumbing rather than a word of the grammar. The offline checks always
# run; the fetch itself is never in the suite, because a suite has to pass on a
# laptop with no network. The sibling package ships `book_table` for its own
# book, and the two names differ on purpose: loaded together, neither masks
# the other's tables.
local({
  refusal <- tryCatch(god_table(42), error = function(e) conditionMessage(e))
  check("god_table refuses a non-name",        is.character(refusal), TRUE)
  check("and the message names the helper",    grepl("god_table", refusal, fixed = TRUE), TRUE)
  check("the tables come from the published book",
        god:::god_book_data_url, "https://psychometrician.github.io/god-book/data/")

  # **A local `data/` wins, and that is the half the network cannot be asked
  # about.** The fetch stays out of the suite; what is checked here is the
  # resolution — that a `data/<name>.csv` above the working directory is found,
  # read, and returned without anything reaching for a connection.
  nest <- file.path(tempfile("god-data-"), "one", "two")
  dir.create(file.path(dirname(dirname(nest)), "data"), recursive = TRUE)
  dir.create(nest, recursive = TRUE)
  writeLines(c("region,revenue", "West,10", "East,20"),
             file.path(dirname(dirname(nest)), "data", "local_only.csv"))
  was <- setwd(nest)
  on.exit(setwd(was), add = TRUE)

  found <- god_table("local_only")
  check("a data/ above the working directory is read",  nrow(found), 2L)
  check("and it is the local file, not a fetch",        found$region[1], "West")

  # A name is a name. Before the walk-up, the worst a bad one could do was make
  # a 404; now it names a file, so `..` has to stop at the check rather than at
  # the filesystem.
  escape <- tryCatch(god_table("../secrets"), error = function(e) conditionMessage(e))
  check("god_table refuses a path",            grepl("not a table name", escape, fixed = TRUE), TRUE)
  check("and says what a name is",             grepl("letters, digits", escape, fixed = TRUE), TRUE)

  # The other direction: no `data/` anywhere above, so the published copy is
  # what it would reach for. Checked through the resolver rather than by
  # fetching, because the suite stays offline.
  check("with no local data/, nothing is found and the URL is the fallback",
        is.na(god:::god_walk_up_data(nest, "sales")), TRUE)
})


# --- dplyr masks god, and the sentence still runs ----------------------------
# The verbs above shadow other people's names and pay for it with a message.
# This is the other direction, and the common one: dplyr attached after god owns
# `collect`, `pick`, `rename` and `summarize`, and a pipeline handed to one of
# them reached a generic with no method for it. Three of the four are generics,
# so god registers a method on each. `pick` is not one, and cannot be reached.
cat("\ndplyr masks god, and the sentence still runs\n")

# **Asked before dplyr is, because asking for it loads it.** Registering from
# `.onLoad` with `requireNamespace` would load dplyr for everyone who has it
# installed, in every session that never mentions it. The hook god leaves
# instead costs a session nothing until dplyr actually arrives.
check("loading god does not load dplyr", isNamespaceLoaded("dplyr"), FALSE)

if (!requireNamespace("dplyr", quietly = TRUE)) {
  cat("  dplyr is not installed, so the rest of these are skipped\n")
} else local({
  masked <- sales |> keep(region == "West")

  check("dplyr's collect reaches the pipeline", nrow(dplyr::collect(masked)), 4L)

  # The bar is not that each does something reasonable. It is that each writes
  # *the same sentence* the unmasked call writes, because the fix is finished
  # only when the attach order stops being observable.
  check("dplyr's rename writes god's sentence",
        format(dplyr::rename(masked, area = region)),
        format(rename(masked, area = region)))
  check("dplyr's summarize writes god's sentence",
        format(dplyr::summarize(masked, takings = total(revenue))),
        format(summarize(masked, takings = total(revenue))))

  # `by` names columns and the grammar reads it unevaluated, so forwarding it as
  # a value would hand god the symbol `by` instead of the column. This is the
  # check that catches that.
  check("and carries `by`, which cannot be forwarded",
        format(dplyr::summarize(masked, takings = total(revenue), by = product)),
        format(summarize(masked, takings = total(revenue), by = product)))

  # `summarize` is an alias dispatching through `summarise`, so a method
  # registered under the American spelling is one nothing would ever call.
  check("the British spelling is the same generic",
        format(dplyr::summarise(masked, takings = total(revenue))),
        format(summarize(masked, takings = total(revenue))))

  # **Two matching sentences are not two working sentences.** The checks above
  # compare text, and text agreed while both sides said `sum(revenue)`, which the
  # grammar has no word for — the engine refuses it and neither `format` call
  # ever asked. So the masked sentence is run here, and the rows are read back.
  answered <- dplyr::collect(
    dplyr::summarize(masked, takings = total(revenue), by = product) |> sort(product)
  )
  check("and the masked sentence runs, rather than only matching",
        answered$takings, c(450, 300))

  # **A canary rather than a note of defeat.** dplyr's `pick` reads the data mask
  # of the verb around it and is not a generic, which is why no method can be
  # registered for it and `god::pick` is the only repair. Should this ever fail,
  # dplyr has made it one and god should register a method there too.
  check("dplyr's pick is not a generic, so no method can reach it",
        grepl("UseMethod", paste(deparse(body(dplyr::pick)), collapse = " "), fixed = TRUE),
        FALSE)
})

cat(sprintf("\n%d passed, %d failed\n", passed, failed))
if (failed > 0) quit(status = 1)
