# The verbs, as R.
#
# **Every function here is a syntax builder.** It arranges words into the
# grammar's own text and hands that over; it decides nothing about what the words
# mean. Whether a column exists, whether an aggregation may appear where it was
# written, what an empty group comes out as — none of that is answerable here,
# and answering it here would answer it twice, once per language, with two
# answers that drift.
#
# So the whole of this file is: capture what the caller wrote without evaluating
# it, hand each expression to the translator, and join the results with `then`.
# The result is a sentence in the canonical text, and the canonical text is what
# the grammar reads. A verb that needed to know something about the data would be
# a verb in the wrong place.
#
# **A verb returns a pipeline, not a table**, and that is a safety property as
# much as a laziness one (§4.9). These names shadow other people's — `sort` is
# base R's, `keep` is purrr's, `summarize` is dplyr's — and a shadowed function
# that receives a pipeline has no method for it and stops. A verb that returned a
# data frame would instead hand a plausible object to the wrong function and get
# a plausible answer back.

# What each verb shadows, for the message it owes when it is called on something
# that is not a table.
#
# Only `sort` shadows a base R function among the six. The rest collide with
# packages a reader may or may not have attached, and a message naming a package
# they do not have would be worse than one naming none.
god_shadowed <- c(sort = "base::sort")

#' Keep the rows where a condition holds
#'
#' @param .data A table, or a pipeline.
#' @param condition A condition, written in R: `region == "West"`.
#' @return A pipeline. Print it, or `collect()` it, to get a table.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |> keep(region == "West")
#' #>   region revenue
#' #> 1   West     100
#' #> 2   West     150
#' @export
keep <- function(.data, condition) {
  pipeline <- god_head(.data, substitute(.data), "keep")
  condition <- substitute(condition)

  # **A condition can name a table, which no other expression does.**
  # `keep(matching(products, by = id))` reads `products` without any verb
  # mentioning it, so the tables it names are collected here and registered
  # exactly as a verb's would be. Missing this is silent until the engine
  # reports a table it was never handed.
  here <- parent.frame()
  for (name in god_tables_named(condition)) {
    found <- mget(name, envir = here, ifnotfound = list(NULL))[[1L]]
    if (is.null(found)) {
      stop(sprintf("`matching` names a table called `%s`, and there is no such table here", name), call. = FALSE)
    }
    pipeline <- god_use_table(pipeline, name, found)
  }

  god_step(pipeline, sprintf("keep where %s", god_expr(condition)))
}

# The `where(pattern, value)` a verb was given, if it was given one.
#
# Returns the clause after the verb's own word, or NULL when the caller wrote
# ordinary named columns instead.
god_across <- function(given, verb) {
  if (!length(given)) {
    return(NULL)
  }
  first <- given[[1L]]
  if (!is.call(first) || !identical(as.character(first[[1L]]), "where")) {
    return(NULL)
  }
  if (length(given) > 1L) {
    stop(
      sprintf("`%s` takes named columns, or one `where(...)` and nothing beside it", verb),
      call. = FALSE
    )
  }
  parts <- as.list(first)[-1L]
  if (length(parts) != 2L) {
    stop(
      sprintf(
        "`%s(where(...))` has to say what to make of each column: where(startsWith(name, \"q\"), value * 2)",
        verb
      ),
      call. = FALSE
    )
  }
  sprintf(
    "%s as %s",
    god_pick_condition(parts[[1L]]),
    god_expr(god_mark_value(parts[[2L]]))
  )
}

# A second table this pipeline reads, checked and recorded.
#
# Shared by every place that reaches for one, so a table named in a condition
# gets the same treatment as a table named by a verb.
god_use_table <- function(pipeline, name, other) {
  if (!is.data.frame(other)) {
    stop(sprintf("`%s` is not a table", name), call. = FALSE)
  }
  tables <- .subset2(pipeline, "tables")
  if (!is.null(tables[[name]]) && !identical(tables[[name]], other)) {
    stop(
      sprintf("this pipeline already reads a different table called `%s`", name),
      call. = FALSE
    )
  }
  tables[[name]] <- other
  pipeline[["tables"]] <- tables
  pipeline
}

# Every table named by a `matching` anywhere inside an expression.
#
# The walk looks for the call rather than trusting a fixed argument position,
# because `not matching(...)` wraps it and a condition may nest it further.
god_tables_named <- function(e) {
  if (!is.call(e)) {
    return(character())
  }
  found <- character()
  head <- e[[1L]]
  args <- as.list(e)[-1L]

  if (is.symbol(head) && identical(as.character(head), "matching")) {
    named <- names(args)
    if (is.null(named)) named <- rep("", length(args))
    positional <- args[named == ""]
    if (length(positional) == 1L && is.symbol(positional[[1L]])) {
      found <- as.character(positional[[1L]])
    }
  }

  for (part in args) {
    found <- c(found, god_tables_named(part))
  }
  unique(found)
}

#' Just these columns, or all but these
#'
#' @param .data A table, or a pipeline.
#' @param ... Column names. Wrap them in `all_but()` to name the ones to drop
#'   instead: `pick(all_but(cost))`.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West", "North"),
#'   product = c("Widget", "Gadget", "Doohickey", "Widget"),
#'   revenue = c(100, 120, 120, 150),
#'   cost    = c(40, 80, 75, 60)
#' )
#' sales |> pick(region, revenue)
#' #>   region revenue
#' #> 1   West     100
#' #> 2   East     120
#' #> 3   West     120
#' #> 4  North     150
#' @export
pick <- function(.data, ...) {
  pipeline <- god_head(.data, substitute(.data), "pick")
  chosen <- as.list(substitute(list(...)))[-1L]

  if (!length(chosen)) {
    stop("`pick` needs at least one column", call. = FALSE)
  }

  # `where` chooses the columns by the shape of their name, so it stands alone.
  first <- chosen[[1L]]
  if (is.call(first) && identical(as.character(first[[1L]]), "where")) {
    if (length(chosen) > 1L) {
      stop(
        "`where` chooses the columns on its own, so nothing goes beside it: pick(where(startsWith(name, \"q\")))",
        call. = FALSE
      )
    }
    inner <- as.list(first)[-1L]
    if (length(inner) != 1L) {
      stop("`where` takes one question about a column's name", call. = FALSE)
    }
    return(god_step(pipeline, sprintf("pick where %s", god_pick_condition(inner[[1L]]))))
  }

  # `all_but` inverts the list rather than adding a second verb: choosing columns
  # is choosing columns whichever way you say which ones.
  inverted <- is.call(first) && identical(as.character(first[[1L]]), "all_but")
  if (inverted) {
    if (length(chosen) > 1L) {
      stop(
        "write the columns inside `all_but()`, all of them: pick(all_but(cost, region))",
        call. = FALSE
      )
    }
    chosen <- as.list(first)[-1L]
    if (!length(chosen)) {
      stop("`all_but` needs at least one column", call. = FALSE)
    }
  }

  names <- vapply(chosen, god_name, character(1), where = "pick")
  god_step(
    pipeline,
    sprintf("pick %s[%s]", if (inverted) "all_but " else "", paste(names, collapse = ", "))
  )
}

#' Add or replace a column
#'
#' @param .data A table, or a pipeline.
#' @param ... Named columns: `margin = revenue - cost`.
#' @param by Columns to group by. An aggregate is then broadcast back over each
#'   group rather than collapsing it: `add(share = revenue / total(revenue), by = product)`.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |> add(doubled = revenue * 2)
#' #>   region revenue doubled
#' #> 1   West     100     200
#' #> 2   East     120     240
#' #> 3   West     150     300
#' @export
add <- function(.data, ..., by) {
  pipeline <- god_head(.data, substitute(.data), "add")
  grouping <- if (missing(by)) character() else god_names(substitute(by), "by")
  given <- as.list(substitute(list(...)))[-1L]

  # One value applied to every column whose name matches, which is dplyr's
  # `across`. The matched columns keep their names, because `add` already
  # covers making a column and replacing one.
  rule <- god_across(given, "add")
  if (!is.null(rule)) {
    return(god_step(pipeline, sprintf("add where %s%s", rule, god_grouping(grouping))))
  }

  god_step(
    pipeline,
    sprintf(
      "add %s%s",
      god_assignments(given, "add"),
      god_grouping(grouping)
    )
  )
}

#' Collapse to one row per group
#'
#' @param .data A table, or a pipeline.
#' @param ... Named columns, each one an aggregation:
#'   `margin = total(margin)`.
#' @param by Columns to group by. Without it the whole table is one group.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |> summarize(sold = total(revenue), by = region)
#' #>   region sold
#' #> 1   East  120
#' #> 2   West  250
#' @export
summarize <- function(.data, ..., by) {
  pipeline <- god_head(.data, substitute(.data), "summarize")
  grouping <- if (missing(by)) character() else god_names(substitute(by), "by")
  given <- as.list(substitute(list(...)))[-1L]

  # One value applied to every column whose name matches, which is dplyr's
  # `across`. The matched columns keep their names, because `summarize` already
  # covers making a column and replacing one.
  rule <- god_across(given, "summarize")
  if (!is.null(rule)) {
    return(god_step(pipeline, sprintf("summarize where %s%s", rule, god_grouping(grouping))))
  }

  god_step(
    pipeline,
    sprintf(
      "summarize %s%s",
      god_assignments(given, "summarize"),
      god_grouping(grouping)
    )
  )
}

#' Order the rows
#'
#' @param .data A table, or a pipeline.
#' @param ... Columns to order by. Wrap one in `descending()` to reverse it.
#'   There is deliberately no `ascending()`: ascending is what happens when you
#'   do not ask for anything.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |> sort(descending(revenue))
#' #>   region revenue
#' #> 1   West     150
#' #> 2   East     120
#' #> 3   West     100
#' @export
sort <- function(.data, ...) {
  pipeline <- god_head(.data, substitute(.data), "sort")
  keys <- as.list(substitute(list(...)))[-1L]

  if (!length(keys)) {
    stop("`sort` needs at least one column to order by", call. = FALSE)
  }

  written <- vapply(keys, function(key) {
    if (is.call(key) && identical(as.character(key[[1L]]), "descending")) {
      inner <- as.list(key)[-1L]
      if (length(inner) != 1L) {
        stop("`descending` takes one column: sort(descending(revenue))", call. = FALSE)
      }
      return(sprintf("[%s] descending", god_name(inner[[1L]], "sort")))
    }
    sprintf("[%s]", god_name(key, "sort"))
  }, character(1))

  god_step(pipeline, sprintf("sort %s", paste(written, collapse = ", ")))
}

#' The first n rows
#'
#' @param .data A table, or a pipeline.
#' @param n How many. An ordinary R value, so a threshold held in a variable
#'   works: `take(wanted)`.
#' @param by Columns to group by, for the first n rows of each group. It needs a
#'   `sort` before it, because "the first rows" means nothing until something
#'   says first by what.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |> take(2)
#' #>   region revenue
#' #> 1   West     100
#' #> 2   East     120
#' @export
take <- function(.data, n, by) {
  pipeline <- god_head(.data, substitute(.data), "take")
  count <- n
  if (!is.numeric(count) || length(count) != 1L || is.na(count) || count != trunc(count)) {
    stop("`take` needs a whole number of rows: take(10)", call. = FALSE)
  }
  grouping <- if (missing(by)) character() else god_names(substitute(by), "by")
  god_step(
    pipeline,
    sprintf(
      "take %s%s",
      format(count, scientific = FALSE, trim = TRUE),
      god_grouping(grouping)
    )
  )
}

#' The last n rows, or the last n of each group
#'
#' The other end of `take`. It always needs a `sort` before it, where a bare
#' `take` does not: "the first rows" of an unsorted table is at least the rows
#' the pipeline reached first, and "the last rows" is a claim about an end that
#' a table does not have until something says which way it runs.
#'
#' @param .data A table, or a pipeline.
#' @param n How many rows.
#' @param by Columns to group by, for the last n rows of each group.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |> sort(revenue) |> take_last(2)
#' @export
take_last <- function(.data, n, by) {
  pipeline <- god_head(.data, substitute(.data), "take_last")
  count <- n
  if (!is.numeric(count) || length(count) != 1L || is.na(count) || count != trunc(count)) {
    stop("`take_last` needs a whole number of rows: take_last(10)", call. = FALSE)
  }
  grouping <- if (missing(by)) character() else god_names(substitute(by), "by")
  god_step(
    pipeline,
    sprintf(
      "take_last %s%s",
      format(count, scientific = FALSE, trim = TRUE),
      god_grouping(grouping)
    )
  )
}

#' Add another table's columns
#'
#' @param .data A table, or a pipeline.
#' @param other The other table.
#' @param by The columns that say which rows correspond. Left out, the columns
#'   both tables share are used, and god says which it chose. Where the two
#'   tables name a key differently, write both with `==` between them and this
#'   table's first: `by = customer_id == id`. The answer keeps this table's
#'   name. Several keys go in a `c()`, and the two forms mix:
#'   `by = c(region, customer_id == id)`.
#' @param unmatched Whose unmatched rows survive: `"this"` keeps this table's
#'   and is the default, `"none"` keeps neither, `"both"` keeps both. There is
#'   no `"other"`, because that is this join with the tables the other way
#'   round.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West", "North"),
#'   product = c("Widget", "Gadget", "Doohickey", "Widget"),
#'   revenue = c(100, 120, 120, 150),
#'   cost    = c(40, 80, 75, 60)
#' )
#' products <- data.frame(
#'   product = c("Widget", "Gadget"),
#'   maker   = c("Acme", "Globex")
#' )
#' sales |>
#'   pick(product, revenue) |>
#'   join(products, by = product) |>
#'   sort(product)
#' #>     product revenue  maker
#' #> 1 Doohickey     120   <NA>
#' #> 2    Gadget     120 Globex
#' #> 3    Widget     100   Acme
#' #> 4    Widget     150   Acme
#' @export
join <- function(.data, other, by, unmatched = "this") {
  pipeline <- god_head(.data, substitute(.data), "join")

  name <- substitute(other)
  if (!is.symbol(name)) {
    stop(
      "`join` needs the other table by name: join(products, by = id)",
      call. = FALSE
    )
  }
  name <- as.character(name)

  pipeline <- god_use_table(pipeline, name, other)

  matched <- if (missing(by)) "" else god_join_keys(substitute(by), "by")
  survivors <- if (identical(unmatched, "this")) {
    ""
  } else {
    sprintf(" unmatched \"%s\"", unmatched)
  }

  god_step(
    pipeline,
    sprintf(
      "join %s%s%s",
      name,
      if (nzchar(matched)) sprintf(" by %s", matched) else "",
      survivors
    )
  )
}

#' Add another table's rows
#'
#' Both tables need the same columns. A column on one side only is refused
#' rather than filled in with missing values, because a column that is half
#' empty and says nothing is how a mistake survives.
#'
#' @param .data A table, or a pipeline.
#' @param other The other table.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' late <- data.frame(region = "North", revenue = 80)
#' sales |> add_rows(late)
#' #>   region revenue
#' #> 1   West     100
#' #> 2   East     120
#' #> 3   West     150
#' #> 4  North      80
#' @export
add_rows <- function(.data, other) {
  pipeline <- god_head(.data, substitute(.data), "add_rows")
  name <- substitute(other)
  if (!is.symbol(name)) {
    stop("`add_rows` needs the other table by name: add_rows(more_sales)", call. = FALSE)
  }
  name <- as.character(name)
  if (!is.data.frame(other)) {
    stop(sprintf("`%s` is not a table", name), call. = FALSE)
  }
  tables <- .subset2(pipeline, "tables")
  tables[[name]] <- other
  pipeline[["tables"]] <- tables
  god_step(pipeline, sprintf("add_rows %s", name))
}

#' Make the absent combinations appear
#'
#' Every combination of the values these columns already hold, as rows. The rows
#' that were there are handed on untouched; the ones that were not arrive with
#' every other column missing, and `fill_missing` is what says otherwise.
#'
#' The values come from the table and nowhere else, so a month with no row
#' anywhere is never invented. A missing value is not a category and makes no
#' combinations, and no row is lost by that: nothing already in the table is
#' touched at all.
#'
#' @param .data A table, or a pipeline.
#' @param ... The columns whose combinations to make. Two or more: one column on
#'   its own has no combinations to make.
#' @param by Columns to make the combinations inside. Without it the whole table
#'   is one group. With it, a new row keeps these columns filled in rather than
#'   going missing, which is the reason to write one.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   product = c("Widget", "Widget", "Gadget"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |>
#'   add_combinations(region, product) |>
#'   fill_missing(revenue = 0)
#' #>   region product revenue
#' #> 1   West  Widget     100
#' #> 2   East  Widget     120
#' #> 3   West  Gadget     150
#' #> 4   East  Gadget       0
#' @export
add_combinations <- function(.data, ..., by) {
  pipeline <- god_head(.data, substitute(.data), "add_combinations")
  chosen <- as.list(substitute(list(...)))[-1L]
  if (!length(chosen)) {
    stop(
      "`add_combinations` needs the columns whose combinations to make: add_combinations(region, product)",
      call. = FALSE
    )
  }
  names <- vapply(chosen, god_name, character(1), where = "add_combinations")
  grouping <- if (missing(by)) character() else god_names(substitute(by), "by")
  god_step(
    pipeline,
    sprintf(
      "add_combinations [%s]%s",
      paste(names, collapse = ", "),
      god_grouping(grouping)
    )
  )
}

#' Drop repeated rows
#'
#' Rows that are identical across every column. The answer comes back in a
#' settled order, because dropping duplicates says nothing about order and an
#' answer that reorders itself between runs is not predictable.
#'
#' @param .data A table, or a pipeline.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   product = c("Widget", "Gadget", "Widget")
#' )
#' sales |> drop_duplicates()
#' #>   region product
#' #> 1   East  Gadget
#' #> 2   West  Widget
#' @export
drop_duplicates <- function(.data) {
  god_step(god_head(.data, substitute(.data), "drop_duplicates"), "drop_duplicates")
}

#' Rename a column
#'
#' The new name goes first, the way it does everywhere else in the grammar and
#' the way assignment reads: `rename(margin = profit)`.
#'
#' @param .data A table, or a pipeline.
#' @param ... `new = old` pairs.
#' @return A pipeline.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' sales |> rename(area = region)
#' #>   area revenue
#' #> 1 West     100
#' #> 2 East     120
#' #> 3 West     150
#' @export
rename <- function(.data, ...) {
  pipeline <- god_head(.data, substitute(.data), "rename")
  pairs <- as.list(substitute(list(...)))[-1L]
  names <- names(pairs)
  if (!length(pairs) || is.null(names) || any(!nzchar(names))) {
    stop("`rename` takes `new = old` pairs: rename(margin = profit)", call. = FALSE)
  }
  written <- vapply(seq_along(pairs), function(i) {
    sprintf("[%s] as [%s]", names[[i]], god_name(pairs[[i]], "rename"))
  }, character(1))
  god_step(pipeline, sprintf("rename %s", paste(written, collapse = ", ")))
}

#' Drop rows with missing values
#'
#' @param .data A table, or a pipeline.
#' @param ... Columns to look at. With none, every column.
#' @return A pipeline.
#' @examples
#' patchy <- data.frame(
#'   product = c("Widget", "Gadget"),
#'   revenue = c(100, NA),
#'   listed  = c(90, 60)
#' )
#' patchy |> drop_missing(revenue)
#' #>   product revenue listed
#' #> 1  Widget     100     90
#' @export
drop_missing <- function(.data, ...) {
  pipeline <- god_head(.data, substitute(.data), "drop_missing")
  chosen <- as.list(substitute(list(...)))[-1L]
  if (!length(chosen)) {
    return(god_step(pipeline, "drop_missing"))
  }
  names <- vapply(chosen, god_name, character(1), where = "drop_missing")
  god_step(pipeline, sprintf("drop_missing [%s]", paste(names, collapse = ", ")))
}

#' Replace missing values in a column
#'
#' @param .data A table, or a pipeline.
#' @param ... `column = value` pairs.
#' @return A pipeline.
#' @examples
#' patchy <- data.frame(
#'   product = c("Widget", "Gadget"),
#'   revenue = c(100, NA),
#'   listed  = c(90, 60)
#' )
#' patchy |> fill_missing(revenue = 0)
#' #>   product revenue listed
#' #> 1  Widget     100     90
#' #> 2  Gadget       0     60
#' @export
fill_missing <- function(.data, ...) {
  pipeline <- god_head(.data, substitute(.data), "fill_missing")
  god_step(
    pipeline,
    sprintf("fill_missing %s", god_assignments(as.list(substitute(list(...)))[-1L], "fill_missing"))
  )
}

#' Turn columns into rows
#'
#' The table grows taller, which is what the name says. Direction goes in the
#' name because nobody could ever remember which of `melt` and `cast` did this.
#'
#' @param .data A table, or a pipeline.
#' @param ... The columns that become rows. `all_but(id)` names the ones to
#'   leave where they are, and `where(startsWith(name, "q"))` chooses them by
#'   the shape of their name — the same three ways `pick` chooses columns.
#' @param name What the new column of names is called. A bare name, or the shape
#'   of the old names in quotes when they hold more than one thing:
#'   `name = "\{question\}_\{year\}"`. Writing `\{value\}` for a piece says that
#'   piece picks which value column a row belongs to.
#' @param value What the new column of values is called. Left out, the two are
#'   called `name` and `value`, which are the grammar's own words for them.
#' @return A pipeline.
#' @examples
#' answers <- data.frame(
#'   student = c("ann", "bob"),
#'   q1 = c(1, 4), q2 = c(2, 5)
#' )
#' answers |> lengthen(q1, q2)
#' #>   student name value
#' #> 1     ann   q1     1
#' #> 2     ann   q2     2
#' #> 3     bob   q1     4
#' #> 4     bob   q2     5
#' @export
lengthen <- function(.data, ..., name, value) {
  pipeline <- god_head(.data, substitute(.data), "lengthen")
  chosen <- as.list(substitute(list(...)))[-1L]

  if (!length(chosen)) {
    stop(
      "`lengthen` needs the columns that become rows: lengthen(q1, q2, q3). `all_but(id)` names the ones to leave instead",
      call. = FALSE
    )
  }

  # The three ways of choosing columns are `pick`'s three, read by the same
  # code, so learning one is learning the other.
  first <- chosen[[1L]]
  if (is.call(first) && identical(as.character(first[[1L]]), "where")) {
    if (length(chosen) > 1L) {
      stop(
        "`where` chooses the columns on its own, so nothing goes beside it: lengthen(where(startsWith(name, \"q\")))",
        call. = FALSE
      )
    }
    inner <- as.list(first)[-1L]
    if (length(inner) != 1L) {
      stop("`where` takes one question about a column's name", call. = FALSE)
    }
    selector <- sprintf("where %s", god_pick_condition(inner[[1L]]))
  } else {
    inverted <- is.call(first) && identical(as.character(first[[1L]]), "all_but")
    if (inverted) {
      if (length(chosen) > 1L) {
        stop(
          "write the columns inside `all_but()`, all of them: lengthen(all_but(id, region))",
          call. = FALSE
        )
      }
      chosen <- as.list(first)[-1L]
      if (!length(chosen)) {
        stop("`all_but` needs at least one column", call. = FALSE)
      }
    }
    names <- vapply(chosen, god_name, character(1), where = "lengthen")
    selector <- sprintf(
      "%s[%s]",
      if (inverted) "all_but " else "",
      paste(names, collapse = ", ")
    )
  }

  said <- god_naming(
    if (base::missing(name)) NULL else substitute(name),
    if (base::missing(value)) NULL else substitute(value),
    "lengthen",
    value_is_expression = FALSE
  )
  god_step(
    pipeline,
    sprintf("lengthen %s%s", selector, if (nzchar(said)) sprintf(" as %s", said) else "")
  )
}

#' Turn rows into columns
#'
#' The inverse of `lengthen`, reading the same two words the other way: `name`
#' points at the column holding column names in both, and the verb says whether
#' it is being made or read.
#'
#' @param .data A table, or a pipeline.
#' @param name The column the new column names come from, or the shape to build
#'   them in: `name = "\{question\}_\{year\}"`.
#' @param value What fills the cells. A bare column means one row per cell, and
#'   the query stops and names the cell if two rows want one. An aggregation
#'   says what to do about that instead: `value = average(answer)`.
#' @param by The columns that say which rows go together. Left out, it is every
#'   column not named above — which is convenient until one stray column makes
#'   every row unique, so it is worth writing.
#' @param missing What an empty cell holds. Needs `giving`, because saying what
#'   an empty cell holds means knowing which cells there are.
#' @param giving The columns this makes. Without it the columns come from the
#'   data, which nothing can know before the query runs, so nothing may follow.
#' @return A pipeline.
#' @examples
#' marks <- data.frame(
#'   student  = c("ann", "ann", "bob", "bob"),
#'   question = c("q1", "q2", "q1", "q2"),
#'   mark     = c(1, 2, 4, 5)
#' )
#' marks |> widen(name = question, value = mark, by = student,
#'                giving = c(q1, q2))
#' #>   student q1 q2
#' #> 1     ann  1  2
#' #> 2     bob  4  5
#' @export
widen <- function(.data, name, value, by, missing, giving) {
  pipeline <- god_head(.data, substitute(.data), "widen")

  said <- god_naming(
    if (base::missing(name)) NULL else substitute(name),
    if (base::missing(value)) NULL else substitute(value),
    "widen",
    value_is_expression = TRUE
  )
  out <- sprintf("widen%s", if (nzchar(said)) sprintf(" %s", said) else "")

  if (!base::missing(by)) {
    out <- sprintf("%s by [%s]", out, paste(god_names(substitute(by), "by"), collapse = ", "))
  }
  if (!base::missing(missing)) {
    out <- sprintf("%s missing %s", out, god_expr(substitute(missing)))
  }
  if (!base::missing(giving)) {
    out <- sprintf(
      "%s giving [%s]",
      out,
      paste(god_names(substitute(giving), "giving"), collapse = ", ")
    )
  }
  god_step(pipeline, out)
}

# -- building the pipeline ---------------------------------------------------

# The head of a pipeline is the table, and the head of the R call is where it is.
#
# `sales |> keep(…)` becomes `keep(sales, …)`, so the name the caller used is in
# the unevaluated first argument. It is taken from there rather than from the
# frame itself, because a data frame does not know what it is called and the
# grammar's sentence names its table.
god_head <- function(data, expr, verb) {
  if (inherits(data, "god_pipeline")) {
    return(data)
  }

  if (!is.data.frame(data)) {
    god_not_a_table(data, expr, verb)
  }

  name <- if (is.symbol(expr)) as.character(expr) else "table"
  tables <- list(data)
  names(tables) <- name
  structure(
    # `tables` rather than one table, because `join` names a second one and the
    # grammar has to be told about every table a sentence reads.
    list(source = name, tables = tables, steps = character()),
    class = "god_pipeline"
  )
}

# The message a masked name owes when it is called on something that is not a
# table.
#
# **This is the cost of masking, paid back.** god takes `sort` from base R
# deliberately, because a prefix on every sentence is uglier than a good message
# on the few calls that go wrong. The message is only worth the trade if it names
# the function that was shadowed and how to reach it.
god_not_a_table <- function(data, expr, verb) {
  written <- paste(deparse(expr), collapse = " ")
  shadowed <- god_shadowed[[verb]]

  if (!is.null(shadowed) && !is.na(shadowed)) {
    stop(
      sprintf(
        "god's `%s` orders the rows of a table, and `%s` is not a table.\n  For R's own, write `%s(%s)`.",
        verb, written, shadowed, written
      ),
      call. = FALSE
    )
  }

  stop(
    sprintf("`%s` works on a table, and `%s` is not one", verb, written),
    call. = FALSE
  )
}

god_step <- function(pipeline, text) {
  pipeline[["steps"]] <- c(.subset2(pipeline, "steps"), text)
  pipeline
}

# `name ...` and `value ...`, the pair both reshaping verbs take.
#
# One writer, because the two verbs take the same two words and differ only in
# which way they flow. `widen`'s value is a whole expression, since an
# aggregation there is what answers "two rows want the same cell".
god_naming <- function(name, value, verb, value_is_expression) {
  said <- character()
  if (!is.null(name)) {
    said <- c(said, sprintf("name %s", god_pattern(name)))
  }
  if (!is.null(value)) {
    written <- if (value_is_expression) {
      god_expr(value)
    } else {
      sprintf("[%s]", god_name(value, "value"))
    }
    said <- c(said, sprintf("value %s", written))
  }
  paste(said, collapse = ", ")
}

# Either a bare column or the shape of the names in quotes. The bare form is the
# one-part case of the quoted one rather than a second shape, so there is one
# idea here and a shorthand for the common use of it.
god_pattern <- function(e) {
  if (is.character(e) && length(e) == 1L) {
    return(sprintf("\"%s\"", e))
  }
  sprintf("[%s]", god_name(e, "name"))
}

god_assignments <- function(values, verb) {
  if (!length(values)) {
    stop(sprintf("`%s` needs at least one column", verb), call. = FALSE)
  }

  names <- names(values)
  if (is.null(names) || any(!nzchar(names))) {
    stop(
      sprintf(
        "`%s` names the column it makes: %s(margin = revenue - cost)",
        verb, verb
      ),
      call. = FALSE
    )
  }

  paste(
    vapply(
      seq_along(values),
      function(i) sprintf("[%s] as %s", names[[i]], god_expr(values[[i]])),
      character(1)
    ),
    collapse = ", "
  )
}

god_grouping <- function(names) {
  if (!length(names)) {
    return("")
  }
  sprintf(" by [%s]", paste(names, collapse = ", "))
}

# The pipeline as the grammar's own text, which is what gets handed over.
#
# Fields are read with `.subset2` here and everywhere inside the package,
# because `$` and `[[` on a pipeline refuse: they are the reader's mistake
# surface, and the package must not depend on the door it locked.
god_written <- function(pipeline) {
  paste(c(.subset2(pipeline, "source"),
          paste0("  then ", .subset2(pipeline, "steps"))), collapse = "\n")
}

# -- materializing -----------------------------------------------------------

#' Run a pipeline and get the table
#'
#' Nothing runs until the answer is wanted. Printing a pipeline runs it, and so
#' does converting one; this is the explicit form, for when you want the table
#' rather than the look of it.
#'
#' @param pipeline A pipeline.
#' @return A data frame.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' answer <- collect(sales |> keep(region == "West"))
#' nrow(answer)
#' #> [1] 2
#' @export
collect <- function(pipeline) {
  if (!inherits(pipeline, "god_pipeline")) {
    stop("`collect` runs a god pipeline, and this is not one", call. = FALSE)
  }
  god_query(god_written(pipeline),
            .subset2(pipeline, "tables"), .subset2(pipeline, "source"))
}

#' @export
print.god_pipeline <- function(x, ...) {
  print(collect(x), ...)
  invisible(x)
}

#' @export
as.data.frame.god_pipeline <- function(x, ...) {
  collect(x)
}

#' The pipeline as the grammar's own text
#'
#' @param x A pipeline.
#' @param ... Unused.
#' @return The text.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' format(sales |> keep(region == "West"))
#' #> [1] "sales\n  then keep where ([region] is \"West\")"
#' @export
format.god_pipeline <- function(x, ...) {
  god_written(x)
}

# A pipeline is a plan, and a plan answers no question a table answers. Left
# alone, R would answer anyway: `$` and `[[` on the underlying list return
# NULL, `names` returns the plan's own internals as if they were columns,
# `dim` returns NULL so `nrow` does too, and NULL propagates —
# `sum(p$revenue)` is 0, a plausible number computed from a question that
# never ran. Python stops the same misuse with a TypeError of its own
# accord; in R only the object can say so, and these methods are it saying
# so. The package reads its own fields with `.subset2`, which does not
# dispatch here, so the door is locked from the outside only.
god_not_yet_a_table <- function(asked, repair) {
  stop(
    sprintf(
      "a pipeline is a plan for a table, and %s. Nothing has run yet: %s",
      asked, repair
    ),
    call. = FALSE
  )
}

#' @export
`$.god_pipeline` <- function(x, name) {
  god_not_yet_a_table(
    sprintf("`$%s` asks the plan for a column", name),
    sprintf("`collect(pipeline)$%s` asks the table it makes", name)
  )
}

#' @export
`[[.god_pipeline` <- function(x, ...) {
  god_not_yet_a_table(
    "`[[` asks the plan for a column",
    "collect the pipeline first, then take the column from the table"
  )
}

#' @export
`[.god_pipeline` <- function(x, ...) {
  god_not_yet_a_table(
    "`[` asks the plan for rows or columns",
    "collect the pipeline first, then subset the table"
  )
}

#' @export
dim.god_pipeline <- function(x) {
  god_not_yet_a_table(
    "a plan has no rows or columns to count",
    "`nrow(collect(pipeline))` counts the table it makes"
  )
}

#' @export
names.god_pipeline <- function(x) {
  god_not_yet_a_table(
    "a plan does not hold its answer's column names",
    "`names(collect(pipeline))` names the table it makes"
  )
}
