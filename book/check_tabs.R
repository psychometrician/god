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
#      in that order.
#   2. A tabset that runs R runs Python too: executable `{r}` chunks and
#      `{python}` chunks appear in matched, non-zero counts.
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
      if (!identical(labels, c("### R", "### Python"))) {
        bad <- c(bad, sprintf(
          "  %s:%d tabs are (%s); every tabset is `### R` then `### Python`",
          short, start, paste(labels, collapse = ", ")))
        next
      }

      r_chunks <- sum(grepl("^\\s*```\\{r\\}", body))
      py_chunks <- sum(grepl("^\\s*```\\{python\\}", body))
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
