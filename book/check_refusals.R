# check_refusals.R — does every documented refusal actually refuse?
#
# Run from the repository root; sourced by the R test suite.
#
# **`#| error: true` does not assert that a chunk errors. It tolerates one.** So
# a chunk the book presents as a refusal can quietly stop refusing, render a
# perfectly good table, and every build still exits 0. The prose above it goes on
# claiming the grammar catches something it no longer catches, and no guard in
# this directory can see it: `check_grammar.R` asks whether a pipeline parses,
# and this kind of chunk is one that is *supposed* not to.
#
# It is one of the three defects the writing guide names as uncatchable by
# anything else, and the sibling project shipped exactly this: one of its 48
# refusal chunks drew an empty picture instead of refusing, for several sessions.
#
# **A pipeline is lazy here, which is the trap specific to god.** Evaluating
# `sales |> keep(nonsense)` returns a pipeline object and raises nothing at all.
# It is `collect`, or printing it, that hands the sentence to the engine. So a
# check that only evaluated the chunk would report every refusal as passing while
# testing none of them. The forcing below is the whole reason this file is not
# three lines long.
#
# **An error is not automatically a refusal, either.** A chunk that failed
# because this file did not hand it a table looks identical to one that failed
# because the grammar said no, and that turns a broken check into a green one.
# Which errors mean *this file* is broken is the short, stable list, so that is
# the one written down below rather than a list of what refusals look like.

check_refusals <- function(dirs = "book") {
  # A chapter's chunks are evaluated below for their side effects, and one
  # side effect a guard must never keep is opening windows. A gog plot
  # printed outside a notebook writes a temp page and hands it to the
  # system browser, so the chapter that draws with the sibling opened
  # seven tabs on every run of this file; the render itself never does,
  # because knitr embeds the plot instead of printing it. The browser is a
  # no-op for this function's lifetime, and whatever it was is restored.
  was <- options(browser = function(url) invisible(url))
  on.exit(options(was), add = TRUE)

  qmds <- unlist(lapply(dirs, function(d)
    list.files(d, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)))
  # The same rule the other guards apply: `_book/` is output, and a
  # `_`-prefixed file is one the author has deliberately withheld.
  qmds <- qmds[!grepl("/_", qmds, fixed = TRUE)]

  # The shared fixtures every chapter includes. Read as a chapter would read it,
  # so the tables here are the tables the examples ran against.
  shared <- new.env(parent = globalenv())
  setup <- file.path(dirs[[1]], "_setup.qmd")
  if (file.exists(setup)) {
    for (code in r_chunks(readLines(setup, warn = FALSE))) {
      try(eval(parse(text = paste(code$code, collapse = "\n")), envir = shared),
          silent = TRUE)
    }
  }

  # **Asked the other way round: which errors mean this guard is broken?**
  # Listing what a refusal looks like was tried first and was wrong, because
  # refusals come from three places with three shapes: the engine's diagnostics,
  # the bindings' own messages, and the one refusal that fires from inside the
  # query and arrives wrapped by the driver. Any positive list misses one of
  # them and reports a working refusal as a fault.
  #
  # What is short and stable is the other list: the errors R raises when this
  # file has failed to give a chunk what it needs.
  harness <- "object '[^']*' not found|could not find function|unused argument"

  checked <- 0L
  quiet <- character()
  wrong <- character()

  for (f in qmds) {
    lines <- readLines(f, warn = FALSE)
    chunks <- r_chunks(lines)
    if (!any(vapply(chunks, function(c) c$tolerates, logical(1)))) next

    # Chapter-local tables are defined by earlier chunks, so the chapter is
    # walked in order and every chunk before the last refusal is run for its
    # side effects. An ordinary chunk that fails here is left alone: the render
    # is what covers those, and stopping would hide the refusals after it.
    # **The fixtures are copied in rather than inherited**, and that is not a
    # style choice. A verb looks a table up with `mget`, which does not search
    # enclosing environments, so a chapter env whose *parent* held `products`
    # reported that there was no such table. The refusal that came back was a
    # real one for the wrong reason, which is the failure mode this guard exists
    # to avoid in the book and must not have itself.
    chapter <- new.env(parent = globalenv())
    for (found in ls(shared)) assign(found, get(found, envir = shared), envir = chapter)
    last <- max(which(vapply(chunks, function(c) c$tolerates, logical(1))))

    # Chunks are evaluated from the file's own directory, because that is
    # knitr's rule and a chapter is allowed to read a file by a path relative
    # to itself. Evaluating from the repository root quietly broke the first
    # chapter that did. The restore is immediate rather than `on.exit`,
    # because the next file's path is relative to where this loop stands.
    owd <- getwd()
    on.exit(setwd(owd), add = TRUE)
    setwd(dirname(f))

    for (i in seq_len(last)) {
      chunk <- chunks[[i]]
      code <- chunk$code[nzchar(trimws(chunk$code))]
      if (!length(code)) next
      where <- sprintf("%s:%d", sub("^.*book/", "", f), chunk$line)

      if (!chunk$tolerates) {
        # Run for its tables, not for its output. A chapter prints queries and
        # translations, and none of that belongs in this guard's report.
        try(invisible(capture.output(
          invisible(capture.output(
            eval(parse(text = paste(code, collapse = "\n")), envir = chapter),
            type = "message"
          ))
        )), silent = TRUE)
        next
      }

      checked <- checked + 1L
      # An assumption is reported on stderr and is not a refusal, so it is
      # caught here rather than printed into this guard's own report.
      outcome <- tryCatch({
        value <- NULL
        invisible(capture.output(
          value <- eval(parse(text = paste(code, collapse = "\n")), envir = chapter),
          type = "message"
        ))
        # **Nothing has run yet if this is a pipeline.** Printing is what forces
        # one in a chapter, so the check forces one here, or a refusal that no
        # longer refuses would look exactly like a refusal that does.
        if (inherits(value, "god_pipeline")) {
          invisible(capture.output(collect(value), type = "message"))
          "the pipeline ran and returned a table"
        } else {
          "it evaluated without complaint"
        }
      }, error = function(e) conditionMessage(e))

      if (!grepl("^(the pipeline ran|it evaluated)", outcome)) {
        if (grepl(harness, outcome)) {
          wrong <- c(wrong, sprintf("%s: %s", where,
                                    trimws(strsplit(outcome, "\n")[[1]][1])))
        }
        next
      }
      quiet <- c(quiet, sprintf("%s (%s): %s", where, outcome,
                                paste(trimws(code), collapse = " ")))
    }

    setwd(owd)
  }

  if (!checked) {
    stop("check_refusals: found no `error: true` chunks, so the scan is broken ",
         "rather than the book", call. = FALSE)
  }

  if (length(quiet) || length(wrong)) {
    if (length(quiet)) {
      cat("FAIL: shown as refusals, and they did not refuse\n")
      for (line in quiet) cat("  ", line, "\n", sep = "")
      cat("  Either the grammar stopped refusing, or the prose should stop saying it does.\n")
    }
    if (length(wrong)) {
      cat("FAIL: these failed for a reason that is not a refusal\n")
      for (line in wrong) cat("  ", line, "\n", sep = "")
      cat("  That is this guard missing a table, not the book being wrong. Fix the guard.\n")
    }
    stop(sprintf("check_refusals: %d chunk(s) wrong", length(quiet) + length(wrong)),
         call. = FALSE)
  }

  unmarked <- unmarked_python_refusals(qmds)
  if (length(unmarked)) {
    cat("FAIL: Python refusal chunks with no `#| classes: refusal`\n")
    for (line in unmarked) cat("  ", line, "\n", sep = "")
    cat("  Without it the twin renders as an ordinary result while the R tab is\n")
    cat("  shaded as a refusal, so the two tabs of one sentence disagree on the page.\n")
    stop(sprintf("check_refusals: %d unmarked Python refusal(s)", length(unmarked)),
         call. = FALSE)
  }

  cat("PASS: every documented refusal refuses (", checked, "chunks )\n")
  cat("PASS: every Python refusal is marked for the stylesheet\n")
  invisible(TRUE)
}

# The R half of a refusal raises, so Quarto marks its output `cell-output-error`
# and the stylesheet can find it. The Python half catches and prints, which lands
# in `cell-output-stdout` beside every ordinary table in the book. So the Python
# chunk carries `#| classes: refusal`, which Quarto puts on the cell, and the
# stylesheet reaches the output through it.
#
# **Nothing else can catch a missing one.** The chunk still runs, the message
# still prints, the render still exits 0, and the only symptom is a tab that is
# not shaded on a page where its partner is. That is invisible to every other
# guard here, all of which ask whether something *ran*.
unmarked_python_refusals <- function(qmds) {
  out <- character()
  for (f in qmds) {
    lines <- readLines(f, warn = FALSE)
    open <- grep("^\\s*```\\{python\\}\\s*$", lines)
    for (start in open) {
      close <- grep("^\\s*```\\s*$", lines)
      close <- close[close > start]
      if (!length(close)) next
      body <- lines[(start + 1L):(close[1] - 1L)]
      if (!any(grepl("except GodError", body))) next
      if (any(grepl("^\\s*#\\|\\s*classes:.*\\brefusal\\b", body))) next
      out <- c(out, sprintf("%s:%d", sub("^.*book/", "", f), start))
    }
  }
  out
}

# Every `{r}` chunk in a file, with its line, its code, and whether it is marked
# to tolerate an error.
r_chunks <- function(lines) {
  out <- list()
  i <- 1L
  while (i <= length(lines)) {
    if (grepl("^\\s*```\\{r\\}\\s*$", lines[i])) {
      start <- i
      i <- i + 1L
      body <- character()
      while (i <= length(lines) && !grepl("^\\s*```\\s*$", lines[i])) {
        body <- c(body, lines[i])
        i <- i + 1L
      }
      options <- grep("^\\s*#\\|", body, value = TRUE)
      out[[length(out) + 1L]] <- list(
        line = start,
        code = body[!grepl("^\\s*#\\|", body)],
        tolerates = any(grepl("error:\\s*true", options))
      )
    }
    i <- i + 1L
  }
  out
}

if (sys.nframe() == 0L) check_refusals()
