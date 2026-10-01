# check_agreement.R — every tab of one example answers with one table.
#
# Run from the repository root; sourced by the R test suite.
#
# **The preface promises it, and nothing kept the promise.** "All three tabs
# answer with the same table" is written on the first page, and a disagreeing
# pair is called "a build the book refuses to ship". `check_tabs.R` proves the
# tabs exist and pair up; the parity harness compares its own corpus, never the
# book's examples. So when the 2026-09-30 review read the published page on
# joins, its R tab and its Python tab showed different rows, and a grouped
# `take` on another page listed two groups either way round from one render to
# the next, with every guard green.
#
# **It runs each page the way the render does**: R chunks in R, Python chunks
# through reticulate in the book's virtualenv, in order and from the page's own
# directory, so each tab's table is the one a reader is shown. Then, in every
# tabset whose tabs are `run`, `R` and `Python`, it compares what each tab's last
# expression produced: the column names, and every value in order. **Row order
# is part of the answer**, which is the difference from the same-task
# appendix's comparison, and the reason this exists rather than that.
#
# Values are compared as text, normalized so that the languages' spellings of
# one thing agree: a missing value is `NA`, `NaN`, `None` or `<NA>` depending on
# who prints it, a number is compared to twelve significant digits so R's
# double and Python's integer agree, and truth is `TRUE` or `FALSE`. What
# remains after that is a difference in the answer.
#
# A tab that refuses is the refusal guards' to check, and a tab whose answer is
# not a table, a count or a printed query, is not compared here.
#
# **It needs the book's virtualenv**, because the Python tabs run in it. Where
# there is none, as in the CI job that runs the package suites, it says so and
# grades nothing; a skip is not a pass. The book workflow runs it with one.

check_agreement <- function(dirs = "book") {
  book <- dirs[[1]]
  venv <- file.path(book, ".venv")
  if (!requireNamespace("reticulate", quietly = TRUE) || !dir.exists(venv)) {
    cat("SKIP: check_agreement needs reticulate and", venv,
        "to run the Python tabs, so nothing was compared\n")
    return(invisible(TRUE))
  }
  reticulate::use_virtualenv(normalizePath(venv), required = TRUE)

  was <- options(browser = function(url) invisible(url))
  on.exit(options(was), add = TRUE)

  # The two helpers that read a tab's answer on the Python side: the value of
  # its last expression, and that value as cells of text.
  reticulate::py_run_string(paste(
    "import ast as _god_ast, math as _god_math, datetime as _god_dt",
    "def _god_last(code):",
    "    tree = _god_ast.parse(code)",
    "    if not tree.body:",
    "        return None",
    "    *head, last = tree.body",
    "    g = globals()",
    "    if head:",
    "        exec(compile(_god_ast.Module(head, []), '<tab>', 'exec'), g)",
    "    if isinstance(last, _god_ast.Expr):",
    "        return eval(compile(_god_ast.Expression(last.value), '<tab>', 'eval'), g)",
    "    exec(compile(_god_ast.Module([last], []), '<tab>', 'exec'), g)",
    "    return None",
    "def _god_cell(v):",
    "    import pandas as _pd",
    "    if v is None or v is _pd.NA or v is _pd.NaT:",
    "        return '<missing>'",
    "    if isinstance(v, float) and _god_math.isnan(v):",
    "        return '<missing>'",
    "    if isinstance(v, bool):",
    "        return 'TRUE' if v else 'FALSE'",
    "    if isinstance(v, (int, float)):",
    "        return format(float(v), '.12g')",
    "    if isinstance(v, _god_dt.datetime):",
    "        return v.strftime('%Y-%m-%d') if (v.hour, v.minute, v.second) == (0, 0, 0) else v.strftime('%Y-%m-%d %H:%M:%S')",
    "    if isinstance(v, _god_dt.date):",
    "        return v.isoformat()",
    "    return str(v)",
    "def _god_cells(value):",
    "    import pandas as _pd",
    "    if value is not None and hasattr(value, 'written'):",
    "        value = collect(value)",
    "    if hasattr(value, 'data') and hasattr(value, 'hide'):",
    "        value = value.data",
    "    if not isinstance(value, _pd.DataFrame):",
    "        return None",
    "    return [[str(c) for c in value.columns]] + [[_god_cell(v) for v in value[c].tolist()] for c in value.columns]",
    sep = "\n"))
  py <- reticulate::import_main(convert = FALSE)

  r_cells <- function(value) {
    if (inherits(value, "god_pipeline")) value <- collect(value)
    if (!is.data.frame(value)) return(NULL)
    d <- as.data.frame(value, stringsAsFactors = FALSE)
    c(list(names(d)), unname(lapply(d, function(col) {
      out <- if (inherits(col, "Date")) {
        format(col, "%Y-%m-%d")
      } else if (inherits(col, "POSIXt")) {
        sub(" 00:00:00$", "", format(col, "%Y-%m-%d %H:%M:%S"))
      } else if (is.logical(col)) {
        ifelse(col, "TRUE", "FALSE")
      } else if (is.numeric(col)) {
        sprintf("%.12g", col)
      } else {
        as.character(col)
      }
      out[is.na(col)] <- "<missing>"
      unname(out)
    })))
  }
  py_cells <- function(value) {
    got <- reticulate::py_to_r(py$`_god_cells`(value))
    if (is.null(got)) return(NULL)
    lapply(got, function(x) as.character(unlist(x)))
  }

  # The shared setup, from `book/`, for the reason the refusal guards give.
  setup <- file.path(book, "_setup.qmd")
  setup_lines <- readLines(setup, warn = FALSE)
  shared <- new.env(parent = globalenv())
  owd <- getwd()
  on.exit(setwd(owd), add = TRUE)
  setwd(dirname(setup))
  for (chunk in agreement_chunks(setup_lines)) {
    if (chunk$lang == "r") {
      try(invisible(capture.output(eval(parse(text = chunk$code), envir = shared))), silent = TRUE)
    }
  }
  setup_python <- paste(unlist(lapply(Filter(function(c) c$lang == "python",
                                             agreement_chunks(setup_lines)),
                                      `[[`, "code")), collapse = "\n")
  setwd(owd)

  qmds <- list.files(book, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)
  qmds <- qmds[!grepl("/_", qmds, fixed = TRUE)]

  compared <- 0L
  differ <- character()
  broke <- character()

  for (f in qmds) {
    chunks <- agreement_chunks(readLines(f, warn = FALSE))
    if (!any(vapply(chunks, function(c) !is.na(c$tab), logical(1)))) next

    chapter <- new.env(parent = globalenv())
    for (found in ls(shared)) assign(found, get(found, envir = shared), envir = chapter)
    setwd(dirname(f))
    reticulate::py_run_string(setup_python)
    where <- function(c) sprintf("%s:%d", sub("^.*book/", "", f), c$line)

    answers <- list()
    flush <- function() {
      tables <- Filter(Negate(is.null), answers)
      if (length(tables) > 1L) {
        first <- tables[[1]]
        for (k in seq_along(tables)[-1]) {
          compared <<- compared + 1L
          if (!identical(first, tables[[k]])) {
            differ <<- c(differ, agreement_difference(names(tables)[1], first,
                                                      names(tables)[k], tables[[k]]))
          }
        }
      }
      answers <<- list()
    }

    open <- NA_integer_
    for (chunk in chunks) {
      if (!identical(chunk$tabset, open)) {
        flush()
        open <- chunk$tabset
      }
      if (chunk$skip) next
      label <- sprintf("%s (%s tab)", where(chunk), chunk$tab)
      if (chunk$lang == "r") {
        value <- NULL
        ok <- tryCatch({
          invisible(capture.output(invisible(capture.output({
            for (e in parse(text = chunk$code)) {
              shown <- withVisible(eval(e, envir = chapter))
              value <- if (shown$visible) shown$value else NULL
            }
          }, type = "message"))))
          TRUE
        }, error = function(e) conditionMessage(e))
        if (!is.na(chunk$tab) && !chunk$refuses) {
          if (!isTRUE(ok)) {
            broke <- c(broke, sprintf("%s raised: %s", label, trimws(strsplit(ok, "\n")[[1]][1])))
          } else {
            answers[[label]] <- tryCatch(suppressMessages(r_cells(value)), error = function(e) NULL)
          }
        }
      } else {
        if (!is.na(chunk$tab) && !chunk$refuses) {
          got <- tryCatch({
            out <- NULL
            reticulate::py_capture_output(out <- py$`_god_last`(chunk$code))
            list(value = out)
          }, error = function(e) conditionMessage(e))
          if (is.character(got)) {
            broke <- c(broke, sprintf("%s raised: %s", label, trimws(strsplit(got, "\n")[[1]][1])))
          } else {
            answers[[label]] <- tryCatch({
              cells <- NULL
              reticulate::py_capture_output(cells <- py_cells(got$value))
              cells
            }, error = function(e) NULL)
          }
        } else {
          try(reticulate::py_capture_output(reticulate::py_run_string(chunk$code)), silent = TRUE)
        }
      }
    }
    flush()
    setwd(owd)
  }

  if (!compared) {
    stop("check_agreement: compared no tabs at all, so the scan is broken rather than the book",
         call. = FALSE)
  }
  if (length(differ) || length(broke)) {
    if (length(differ)) {
      cat("FAIL: tabs of one example answer with different tables\n")
      for (line in differ) cat("  ", line, "\n", sep = "")
    }
    if (length(broke)) {
      cat("FAIL: a tab could not be run here, so it could not be compared\n")
      for (line in broke) cat("  ", line, "\n", sep = "")
    }
    stop(sprintf("check_agreement: %d disagreement(s), %d tab(s) that would not run",
                 length(differ), length(broke)), call. = FALSE)
  }
  cat("PASS: every tab of an example answers with the same table (", compared,
      "comparisons )\n")
  invisible(TRUE)
}

# Where two answers part, said in words: which tab, and the first cell.
agreement_difference <- function(a_name, a, b_name, b) {
  if (!identical(a[[1]], b[[1]])) {
    return(sprintf("%s and %s name different columns: %s / %s", a_name, b_name,
                   paste(a[[1]], collapse = ", "), paste(b[[1]], collapse = ", ")))
  }
  for (j in seq_along(a)[-1]) {
    if (!identical(a[[j]], b[[j]])) {
      rows <- max(length(a[[j]]), length(b[[j]]))
      for (i in seq_len(rows)) {
        if (!identical(a[[j]][i], b[[j]][i])) {
          return(sprintf("%s and %s differ in `%s`, row %d: %s / %s", a_name, b_name,
                         a[[1]][j - 1L], i, a[[j]][i], b[[j]][i]))
        }
      }
    }
  }
  sprintf("%s and %s differ", a_name, b_name)
}

# Every chunk of a page in order, with its language, its tab and what it is.
agreement_chunks <- function(lines) {
  out <- list()
  tabset <- NA_integer_
  tab <- NA_character_
  sets <- 0L
  i <- 1L
  while (i <= length(lines)) {
    line <- lines[i]
    if (grepl("^\\s*::: \\{\\.panel-tabset\\}", line)) {
      sets <- sets + 1L
      tabset <- sets
      tab <- NA_character_
    } else if (!is.na(tabset) && grepl("^\\s*:::\\s*$", line)) {
      tabset <- NA_integer_
      tab <- NA_character_
    } else if (!is.na(tabset) && grepl("^\\s*### ", line)) {
      tab <- trimws(sub("^\\s*### ", "", line))
    } else if (grepl("^\\s*```\\{(r|python)\\}\\s*$", line)) {
      lang <- sub("^\\s*```\\{(r|python)\\}\\s*$", "\\1", line)
      start <- i
      body <- character()
      i <- i + 1L
      while (i <= length(lines) && !grepl("^\\s*```\\s*$", lines[i])) {
        body <- c(body, lines[i])
        i <- i + 1L
      }
      options <- grep("^\\s*#\\|", body, value = TRUE)
      code <- paste(body[!grepl("^\\s*#\\|", body)], collapse = "\n")
      # Only god's three spellings are one sentence; any other tab label is a
      # different kind of tabset and is not this guard's to compare.
      in_tab <- !is.na(tabset) && !is.na(tab) && tab %in% c("run", "R", "Python")
      out[[length(out) + 1L]] <- list(
        line = start,
        lang = lang,
        code = code,
        tabset = if (is.na(tabset)) NA_integer_ else tabset,
        tab = if (in_tab) tab else NA_character_,
        skip = any(grepl("eval:\\s*false", options)),
        refuses = any(grepl("error:\\s*true", options)) || grepl("except GodError", code, fixed = TRUE)
      )
    }
    i <- i + 1L
  }
  out
}

if (sys.nframe() == 0L) check_agreement()
