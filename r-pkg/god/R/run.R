# The R launcher.
#
# **This file carries bytes and decides nothing.** Validation, defaults, coercion
# and every message belong to the grammar; a rule implemented here is a rule
# Python would get wrong, and then the two languages disagree about what a
# sentence means. What is left is: find the table, describe it, hand the text
# over, run what comes back.
#
# It does not read the pipeline either. Picking the table's name out of the text
# would be parsing — in a host, and a second time — so the grammar is asked
# instead (`--needs`). The temptation to do it here with a regular expression is
# exactly how two implementations start to differ.

#' Run a pipeline
#'
#' @param pipeline The pipeline, as text.
#' @param ... Tables, named. Usually unnecessary: the table named at the head of
#'   the pipeline is looked up where you are calling from, the way
#'   `duckdb.sql("SELECT * FROM df")` finds `df` in Python.
#' @return A data frame.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' run('
#'   sales
#'     then keep where [region] is "West"
#' ', sales = sales)
#' #>   region revenue
#' #> 1   West     100
#' #> 2   West     150
#' @export
run <- function(pipeline, ...) {
  tables <- list(...)
  # A pipeline can name more than one table, which is what `join` brought. The
  # grammar says which, in the order it names them, and the first is the head.
  sources <- god_needs(pipeline)
  here <- parent.frame()

  for (source in sources) {
    if (!source %in% names(tables)) {
      found <- mget(source, envir = here, ifnotfound = list(NULL))[[1]]
      if (is.null(found)) {
        found <- god_in_engine(source)
      }
      if (is.null(found)) {
        stop(
          sprintf(
            "the pipeline reads a table called `%s`, and there is no such table here.\n  Pass it by name: run(pipeline, %s = your_data)",
            source, source
          ),
          call. = FALSE
        )
      }
      tables[[source]] <- found
    }
  }

  for (name in names(tables)) {
    if (!is.data.frame(tables[[name]])) {
      stop(sprintf("`%s` is not a table", name), call. = FALSE)
    }
  }

  god_query(pipeline, tables, sources[[1]])
}

#' Run pipelines somewhere other than this machine
#'
#' **The sentences do not change and neither does the vocabulary.** What changes
#' is which engine answers them. Point god at a warehouse connection and the same
#' pipeline runs there, against tables it already holds, with nothing copied over.
#'
#' A connection from `sparklyr`, or from `odbc` against a warehouse, is what this
#' is for. Both are `DBI` connections, so god needs to know only two things: the
#' connection, and which dialect of SQL to write for it.
#'
#' Call it with no arguments to go back to the engine on this machine.
#'
#' @param connection A `DBI` connection, or `NULL` to use the engine here.
#' @param dialect Which SQL to write. `"sql"` for DuckDB, `"spark"` for Spark
#'   and Databricks.
#' @return The connection that was in use before, invisibly.
#' @examples
#' \dontrun{
#' 
#' # By default a pipeline runs on DuckDB, in this session.
#' # Point it somewhere else with a DBI connection:
#' con <- DBI::dbConnect(duckdb::duckdb())
#' use_engine(con)
#' 
#' # And back again:
#' use_engine(NULL)
#' }
#' @export
use_engine <- function(connection = NULL, dialect = c("sql", "spark")) {
  dialect <- match.arg(dialect)
  was <- .god$given
  .god$given <- connection
  .god$dialect <- if (is.null(connection)) "sql" else dialect
  invisible(was)
}

# Turn a pipeline into a query, run it, and hand back the rows.
#
# Shared by `run()` and by the native verbs, which differ only in where the text
# came from — a string the caller wrote, or a sentence the verbs built. By the
# time either gets here they are the same thing, and they had better be: a second
# execution path is a second set of answers.
god_query <- function(text, tables, source) {
  # A connection the caller supplied answers in its own dialect, and the tables
  # it already holds are not registered against it: a warehouse table is there
  # under its own name, and copying a frame up to a cluster is not something a
  # pipeline should do behind anyone's back.
  if (!is.null(.god$given)) {
    sql <- god_call(c(columns_args(tables, source), "--as", .god$dialect), text)
    return(DBI::dbGetQuery(.god$given, sql))
  }

  sql <- god_call(columns_args(tables, source), text)

  con <- god_connection()
  for (name in names(tables)) {
    duckdb::duckdb_register(con, name, tables[[name]])
  }
  # The tables are unregistered again so that a name in one pipeline cannot be
  # found by the next one. A connection that quietly remembers is a connection
  # where a typo resolves to last week's data.
  on.exit(
    for (name in names(tables)) try(duckdb::duckdb_unregister(con, name), silent = TRUE),
    add = TRUE
  )
  DBI::dbGetQuery(con, sql)
}

# A table the given engine already holds, described rather than fetched.
#
# **A warehouse table is not a local variable and never will be**, so looking in
# the caller's scope for `catalog.schema.orders` was always going to fail. Where
# a connection has been given, it is asked instead, and asked for no rows: the
# columns and their types are all the grammar needs to check a sentence, and
# fetching a warehouse table in order to describe it would be the one thing this
# design exists to avoid.
god_in_engine <- function(source) {
  if (is.null(.god$given)) {
    return(NULL)
  }
  parts <- strsplit(source, ".", fixed = TRUE)[[1]]
  quoted <- paste0("\"", gsub("\"", "\"\"", parts), "\"", collapse = ".")
  out <- try(
    DBI::dbGetQuery(.god$given, paste0("SELECT * FROM ", quoted, " WHERE 1 = 0")),
    silent = TRUE
  )
  if (inherits(out, "try-error")) NULL else out
}

# One connection, reused.
#
# Opening one per pipeline is a fresh process's worth of work for a query that
# takes a millisecond, and it announces itself on every open. The engine holds no
# state between pipelines — the tables are registered and unregistered around
# each one — so there is nothing for a shared connection to leak.
.god <- new.env(parent = emptyenv())

god_connection <- function() {
  if (is.null(.god$con) || !DBI::dbIsValid(.god$con)) {
    .god$con <- DBI::dbConnect(duckdb::duckdb())
  }
  .god$con
}

#' The same pipeline, written in a language you already know
#'
#' A small vocabulary covers most of what people do and never all of it, so the
#' question is not whether you reach its edge but what happens when you do.
#'
#' @param pipeline The pipeline, as text.
#' @param as Which language. `"sql"`, `"spark"`, `"dplyr"`, `"pandas"`,
#'   `"polars"`, `"pyspark"`, or `"god"` itself. An unknown name is refused and
#'   the message lists the real ones, so this list going stale costs nothing.
#' @param ... Tables, named, if the one at the head is not in scope.
#' @return The text, invisibly, after printing it.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' show_as(sales |> keep(region == "West"), "dplyr")
#' #> sales |>
#' #>   filter((region == "West"))
#' show_as(sales |> keep(region == "West"), "sql")
#' #> WITH step0 AS (SELECT * FROM "sales"),
#' #>      step1 AS (SELECT * FROM step0 WHERE ("region" = 'West'))
#' #> SELECT * FROM step1
#' @export
show_as <- function(pipeline, as = "dplyr", ...) {
  asked <- god_asking(pipeline, list(...), parent.frame())
  text <- god_call(c(asked$args, "--as", as), asked$sentence)
  cat(text)
  invisible(text)
}

#' What a pipeline does to the table, step by step
#'
#' **Nothing runs.** The grammar checks the whole sentence against the columns
#' before anything is executed, so this is a picture of what would happen, drawn
#' from the same reading that would refuse a column that is not there.
#'
#' Every step shows the table as it stands once that step has run, with the
#' columns it makes marked and the ones it takes away marked where they leave. A
#' second table gets a row of its own under the step that reads it, so a join
#' shows what crossed over and what matched. A sentence the grammar refuses is
#' still drawn, as far as it checked, with the refusal under the words that
#' stopped it — which is the question an error message on its own cannot answer.
#'
#' @param pipeline The pipeline, as text or as one built from the verbs.
#' @param ... Tables, named, if the one at the head is not in scope.
#' @return An object that prints as a ladder at the console and draws itself
#'   inside a rendered document. `format(x, "svg")` gives the picture as text,
#'   for writing to a file.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' show_steps(sales |> keep(region == "West") |> take(1))
#' #> sales                              region:text  revenue:number
#' #> ├ keep where ([region] is "West")  region  revenue
#' #> └ take 1                           region  revenue
#' #>     at most 1 rows
#' @export
show_steps <- function(pipeline, ...) {
  structure(
    god_asking(pipeline, list(...), parent.frame()),
    class = "god_steps"
  )
}

#' @export
print.god_steps <- function(x, ...) {
  drawn <- format(x, "text")
  # R reads the engine's output a line at a time and joins it, which drops the
  # last newline. Without this the prompt comes back on the bottom rung of the
  # ladder.
  cat(drawn)
  if (!endsWith(drawn, "\n")) cat("\n")
  invisible(x)
}

#' The drawing as text
#'
#' @param x A drawing, from [show_steps()].
#' @param as `"text"` for the ladder, `"svg"` for the picture.
#' @param ... Unused.
#' @return The drawing, as one string.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' format(show_steps(sales |> take(1)))
#' #> [1] "sales     region:text  revenue:number\n└ take 1  region  revenue\n    at most 1 rows"
#' @export
format.god_steps <- function(x, as = "text", ...) {
  # The engine says which ways of drawing there are, and refuses the rest. A
  # list here would be a second copy of one, and the second copy is the one that
  # goes stale.
  god_call(c(.subset2(x, "args"), "--draw", as), .subset2(x, "sentence"))
}

# Which tables does this pipeline read, and how are they described to the
# grammar?
#
# **Shared by everything that asks the grammar about a pipeline rather than
# running it.** The two callers used to carry a copy each, and a copy of a lookup
# is how one of them ends up resolving a table the other does not.
god_asking <- function(pipeline, tables, here) {
  # A pipeline built from the native verbs already carries its tables and knows
  # what they are called, so there is nothing to look up.
  if (inherits(pipeline, "god_pipeline")) {
    return(list(
      args = columns_args(.subset2(pipeline, "tables"), .subset2(pipeline, "source")),
      sentence = god_written(pipeline)
    ))
  }

  # The same lookup `run` does, and for the same reason: since `join`, a
  # sentence can name more than one table, so every name the grammar reports
  # is resolved rather than only the head.
  sources <- god_needs(pipeline)
  for (source in sources) {
    if (!source %in% names(tables)) {
      found <- mget(source, envir = here, ifnotfound = list(NULL))[[1]]
      if (is.null(found)) {
        found <- god_in_engine(source)
      }
      if (is.null(found)) {
        stop(
          sprintf("the pipeline reads a table called `%s`, and there is no such table here", source),
          call. = FALSE
        )
      }
      tables[[source]] <- found
    }
  }
  list(args = columns_args(tables, sources[[1]]), sentence = pipeline)
}

#' The query a pipeline becomes
#'
#' @param pipeline The pipeline, as text.
#' @param columns The table's columns, as `name:type` separated by commas.
#' @return The query, as text.
#' @examples
#' sales <- data.frame(
#'   region  = c("West", "East", "West"),
#'   revenue = c(100, 120, 150)
#' )
#' cat(god_sql("sales then take 1", "region:text,revenue:number"))
#' #> WITH step0 AS (SELECT * FROM "sales"),
#' #>      step1 AS (SELECT * FROM step0 LIMIT 1)
#' #> SELECT * FROM step1
#' @export
god_sql <- function(pipeline, columns) {
  god_call(c("--columns", columns), pipeline)
}

# -- talking to the grammar -------------------------------------------------

# Which table does this pipeline read? Asked rather than worked out.
god_needs <- function(pipeline) {
  out <- god_call("--needs", pipeline)
  names <- trimws(strsplit(out, "\n", fixed = TRUE)[[1]])
  names[nzchar(names)]
}

# How the tables are described to the grammar.
#
# The head table's columns go in bare, and any other table names itself first.
# One flag with two shapes rather than two flags, because the second shape only
# exists for `join` and a pipeline without one should not have to know about it.
columns_args <- function(tables, source) {
  args <- c("--columns", columns_of(tables[[source]]))
  for (name in names(tables)) {
    if (!identical(name, source)) {
      args <- c(args, "--columns", sprintf("%s=%s", name, columns_of(tables[[name]])))
    }
  }
  args
}

# The one place a process is started.
#
# A refusal arrives on stderr already rendered, with its caret, and becomes the R
# error verbatim. Wrapping it in "god-cli failed (exit 2)" would replace a
# message written for a person with one written for a program.
god_call <- function(args, pipeline) {
  binary <- god_binary()
  out <- tempfile()
  err <- tempfile()
  on.exit(unlink(c(out, err)), add = TRUE)

  # `system2` pastes its arguments into a command line without quoting, so a
  # column called `order date` used to split the schema at the space and the
  # engine answered with its usage text. Quoted, a name may hold anything a
  # data frame allows.
  status <- system2(
    binary, shQuote(args),
    stdout = out, stderr = err,
    input = pipeline
  )

  messages <- readLines(err, warn = FALSE)
  if (status != 0) {
    stop("\n", paste(messages, collapse = "\n"), call. = FALSE)
  }
  # An assumption is not a failure and never stops anything, but it is never
  # silent either.
  if (length(messages)) message(paste(messages, collapse = "\n"))

  paste(readLines(out, warn = FALSE), collapse = "\n")
}

# What the engine is called on this machine.
#
# Windows is the only platform that spells it differently, and it spells it
# differently in three places at once: the copy `configure.win` bundles, the file
# a walk-up finds in `target/release/`, and anything already on the PATH. So the
# name is decided once here rather than written out at each site. A per-site copy
# is how a Windows package installs perfectly and then cannot find the engine it
# just installed — the install and the lookup would each be right about a
# different name.
god_exe <- function() {
  if (identical(.Platform$OS.type, "windows")) "god-cli.exe" else "god-cli"
}

god_binary <- function() {
  # The order is the contract, and Python resolves in the same order. An
  # explicit `GOD_CLI` always wins. A source tree's own build outranks the
  # bundled copy, because an engine staged into `inst/bin` lingers beside the
  # source after an in-place install, exactly as old as it — bundled first is
  # how a harness spends a day testing last week's engine. The bundled engine
  # is the installed package's answer; the working directory's tree and the
  # PATH come last, because neither has a reason to match this copy of the
  # binding. The walk-ups exist **because the message below names
  # `cargo build --release`**, and a message that names a fix the code then
  # ignores is worse than no message.
  named <- Sys.getenv("GOD_CLI", "")
  if (nzchar(named) && file.exists(named)) return(named)

  beside_source <- god_walk_up(dirname(god_source_dir()))
  if (!is.null(beside_source)) return(beside_source)

  bundled <- system.file("bin", god_exe(), package = "god")
  if (nzchar(bundled) && file.exists(bundled)) return(bundled)

  beside_cwd <- god_walk_up(getwd())
  if (!is.null(beside_cwd)) return(beside_cwd)

  on_path <- unname(Sys.which(god_exe()))
  if (nzchar(on_path)) return(on_path)

  stop(
    "the god engine was not found. Build it with `cargo build --release`, or point GOD_CLI at it",
    call. = FALSE
  )
}

# `target/release/god-cli`, in this directory or any above it.
god_walk_up <- function(start) {
  directory <- normalizePath(start, mustWork = FALSE)
  repeat {
    candidate <- file.path(directory, "target", "release", god_exe())
    if (file.exists(candidate)) return(candidate)
    parent <- dirname(directory)
    if (identical(parent, directory)) break
    directory <- parent
  }
  NULL
}

# Where this file is, when there is a source tree to have one.
god_source_dir <- function() {
  own <- getNamespaceInfo("god", "path")
  if (is.character(own) && nzchar(own)) own else getwd()
}

# -- describing a table -----------------------------------------------------

# A data frame's columns, in the grammar's words.
#
# **This is the one thing the launcher knows that the grammar does not**: what R
# calls a column's type. The mapping is deliberately coarse, because the grammar
# draws only the distinctions that change whether a sentence is legal, and a type
# it has no opinion about passes every test rather than failing them.
columns_of <- function(table) {
  kinds <- vapply(table, god_type, character(1))
  paste(sprintf("%s:%s", names(table), kinds), collapse = ",")
}

god_type <- function(column) {
  # **A POSIXct carries a time and a Date does not, and the grammar now knows
  # the difference.** One word depends on it, `hour`, and telling them apart
  # here is what lets the checker refuse an hour a plain date cannot answer.
  # Everything else treats the two as one kind, and `kind` reports both as
  # "date", so a sentence selecting date columns is unaffected.
  if (inherits(column, c("POSIXct", "POSIXt"))) return("timestamp")
  if (inherits(column, "Date")) return("date")
  if (is.logical(column)) return("truth")
  if (is.numeric(column)) return("number")
  if (is.character(column)) return("text")
  # A factor is text with a fixed set of values, and the grammar has no opinion
  # about the fixed set.
  if (is.factor(column)) return("text")
  "unknown"
}
