# check_tabs.R — a tabset holds one sentence in two languages, always both.
#
# The book's central claim is that the R spelling and the Python spelling are
# one sentence, and the tabsets are where a reader checks it. A tab that
# silently does not appear is a page one language's readers get and the other's
# do not; a Python tab that catches `Exception` instead of `GodError` is a
# refusal demonstration that would also swallow a harness bug and call it a
# refusal. The sibling project found twelve silently missing tabs on a day
# somebody happened to grep a render log; this file is why nobody here has to
# grep.
#
# Four assertions, over every tabset in every chapter:
#   1. A `.panel-tabset` holds exactly the labels `### R` then `### Python`,
#      in that order, with one optional third label, `### run`: the text form
#      of the same sentence, in the one call both languages spell identically.
#      A run tab holds exactly one `{r}` chunk and that chunk calls `run('`,
#      so the third tab cannot quietly become a second R tab.
#   2. A tabset that runs R runs Python too: executable `{r}` chunks and
#      `{python}` chunks appear in matched, non-zero counts, the run tab's
#      own chunk set aside.
#   3. A refusal shown in R (`#| error: true`) is shown in Python in the same
#      tabset (`except GodError`), and the Python side prints what it caught.
#   4. No Python tab anywhere catches bare `Exception`. `GodError` is the one
#      surface a refusal arrives on, and a demonstration that catches more
#      than that would pass while catching this guard's own mistakes.
#
# Run from the repository root; sourced by the R test suite.

check_tabs <- function(book = "book") {
  fail <- function(...) stop(..., call. = FALSE)

  qmds <- list.files(book, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)
  qmds <- qmds[!grepl("/_", qmds, fixed = TRUE)]

  bad <- character()
  tabsets <- 0L

  for (f in qmds) {
    ln <- readLines(f, warn = FALSE)
    short <- sub("^.*book/", "", f)

    open_at <- grep("^\\s*::: \\{\\.panel-tabset\\}", ln)
    close_at <- grep("^\\s*:::\\s*$", ln)

    for (start in open_at) {
      end <- close_at[close_at > start][1]
      if (is.na(end)) {
        bad <- c(bad, sprintf("  %s:%d tabset never closes", short, start))
        next
      }
      tabsets <- tabsets + 1L
      body <- ln[start:end]

      labels <- trimws(grep("^\\s*### ", body, value = TRUE))
      # `run` leads, ruled 2026-08-11 with the text form. It is the surface a
      # new reader is pointed at, so it is the tab already open when the page
      # loads; Quarto shows the first one. The pipe spellings follow in the
      # book's canonical order, R then Python.
      # **A second shape, and only one page has it.** The rule below is about
      # one sentence in two languages. The appendix that compares five
      # libraries is about one task in five of them, so the R-against-Python
      # count cannot apply. It holds four `{r}` chunks (god's run and R
      # spellings, dplyr or tidyr, data.table) against three `{python}` (god's
      # Python spelling, pandas, polars). god's three spellings lead, in the
      # book's canonical order, and the four neighbours follow.
      #
      # What carries over is the part that earns this file its place. The
      # labels are named exactly, in order, so a tab that silently fails to
      # appear is caught here rather than by somebody grepping a render log —
      # and this page has 125 tabs, which is the largest place such a tab
      # could hide.
      five <- c("### god: run", "### god: R", "### god: Python",
                "### dplyr", "### pandas", "### polars", "### data.table")
      if (identical(labels, five) ||
          identical(labels, sub("### dplyr", "### tidyr", five, fixed = TRUE))) {
        if (sum(grepl("^\\s*```\\{r\\}", body)) != 4L ||
            sum(grepl("^\\s*```\\{python\\}", body)) != 3L) {
          bad <- c(bad, sprintf(
            "  %s:%d a five-library tabset holds four `{r}` chunks and three `{python}`, and this one holds %d and %d",
            short, start, sum(grepl("^\\s*```\\{r\\}", body)),
            sum(grepl("^\\s*```\\{python\\}", body))))
        }
        next
      }

      has_run <- identical(labels, c("### run", "### R", "### Python"))
      if (!identical(labels, c("### R", "### Python")) && !has_run) {
        bad <- c(bad, sprintf(
          paste("  %s:%d tabs are (%s); a tabset is `### R` then `### Python`,",
                "with `### run` allowed first, or the five-library shape",
                "(god: run, god: R, god: Python, dplyr or tidyr, pandas, polars, data.table)"),
          short, start, paste(labels, collapse = ", ")))
        next
      }

      main <- body
      if (has_run) {
        at <- grep("^\\s*### run\\s*$", body)[1]
        r_at <- grep("^\\s*### R\\s*$", body)[1]
        run_part <- body[at:(r_at - 1L)]
        main <- c(body[1:(at - 1L)], body[r_at:length(body)])
        run_chunks <- sum(grepl("^\\s*```\\{r\\}", run_part))
        if (run_chunks != 1L ||
            sum(grepl("^\\s*```\\{python\\}", run_part)) != 0L ||
            !any(grepl("run('", run_part, fixed = TRUE))) {
          bad <- c(bad, sprintf(
            "  %s:%d the run tab holds one `{r}` chunk calling `run('...')`, and this one does not",
            short, start))
          next
        }
      }

      r_chunks <- sum(grepl("^\\s*```\\{r\\}", main))
      py_chunks <- sum(grepl("^\\s*```\\{python\\}", main))
      if (r_chunks == 0 || r_chunks != py_chunks) {
        bad <- c(bad, sprintf(
          "  %s:%d %d R chunk(s) against %d Python; a sentence appears in both or in neither",
          short, start, r_chunks, py_chunks))
        next
      }

      refuses_r <- any(grepl("error:\\s*true", grep("^\\s*#\\|", body, value = TRUE)))
      refuses_py <- any(grepl("except GodError", body, fixed = TRUE))
      if (refuses_r && !refuses_py) {
        bad <- c(bad, sprintf(
          "  %s:%d the R tab refuses and the Python tab does not; a refusal is shown twice or not at all",
          short, start))
      }
      if (refuses_py && !any(grepl("print(refusal)", body, fixed = TRUE))) {
        bad <- c(bad, sprintf(
          "  %s:%d the Python tab catches a refusal and never prints it; the reader sees an empty cell",
          short, start))
      }
    }

    # Assertion 4 is file-wide, tabset or not.
    naked <- grep("except Exception", ln, fixed = TRUE)
    for (i in naked) {
      bad <- c(bad, sprintf(
        "  %s:%d catches bare `Exception`; a refusal arrives as `GodError`, so catch that",
        short, i))
    }
  }

  if (!tabsets)
    fail("FAIL: check_tabs found no tabsets, so the scan is broken rather than the book")

  if (length(bad)) {
    cat("FAIL: a sentence lost a language, or a refusal lost its shape\n")
    cat(paste(unique(bad), collapse = "\n"), "\n", sep = "")
    stop("check_tabs: ", length(unique(bad)), " tabset defect(s)", call. = FALSE)
  }

  cat("PASS: every tabset holds one sentence in both languages (", tabsets,
      "tabsets )\n")
  invisible(TRUE)
}

if (sys.nframe() == 0L) check_tabs()
