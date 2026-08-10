# check_promises.R — does the book keep the rules its preface states?
#
# The preface says the book is governed by rules a reader can hold it to. The
# sibling book made the same claim, a reader held it to them on 2026-07-28, and
# three of its five were false; the only rule that had held was the one with a
# script behind it. A promise a script cannot check is a promise that drifts,
# so god's rules were worded to be checkable on the day they were stated, and
# this file is the check.
#
# What is checked here, and where the rest live:
#
#   * Both tabs are one sentence — check_tabs.R's job.
#   * Questions first — here. The first prose paragraph of every teaching
#     chapter asks something within its opening two sentences. The rule fixes
#     the order, question then answer; the verb may still arrive in the very
#     next sentence.
#   * A small cast — here, as a share rather than a count. The sibling book
#     promised "eight tables" while 33 were in use, which is what a number in
#     prose does. What the rule cares about is that a reader meets few enough
#     tables to stop noticing them, so the share of pipelines that start from
#     a cast table is re-derived on every run and never asserted in prose.
#   * Read it aloud — here. The first executable tabset of every teaching
#     chapter is followed, within ten lines, by the same pipeline in the
#     grammar's own words, italicized. That gloss is itself a text-form
#     pipeline, so check_grammar.R parses every one: a gloss that rots fails
#     the build, which is more than an English gloss could ever promise.
#   * Refusals on stage, in both languages — check_refusals.R and
#     check_refusals.py's job.
#
# Scope. The opening rules bind the teaching chapters: everything listed in
# `_quarto.yml` between the Part I divider and the Part VII divider, part pages
# and the two capstones excluded. Reading the span from the file means a new
# chapter is under the rules the day it is added, which is the failure this
# whole file exists for.
#
# Run from the repository root; sourced by the R test suite.

check_promises <- function(book = "book") {
  fail <- function(...) stop(..., call. = FALSE)
  problems <- character()

  yml <- readLines(file.path(book, "_quarto.yml"), warn = FALSE)
  # List entries only: chapters are also named in comments, and a trailing
  # comment may follow a part line.
  entries <- grep("^\\s*-\\s*(part:\\s*)?[A-Za-z0-9_/-]+\\.qmd\\s*(#.*)?$",
                  yml, value = TRUE)
  listed <- sub("\\s*#.*$", "", sub("^\\s*-\\s*(part:\\s*)?", "", trimws(entries)))

  start <- match("parts/six-verbs.qmd", listed)
  stop_ <- match("parts/how-it-works.qmd", listed)
  if (is.na(start) || is.na(stop_))
    fail("FAIL: check_promises cannot find the Part I / Part VII dividers in _quarto.yml")
  teaching <- listed[(start + 1):(stop_ - 1)]
  teaching <- teaching[!grepl("^parts/", teaching)]

  read_chapter <- function(f) readLines(file.path(book, f), warn = FALSE)

  # --- Questions first ------------------------------------------------------
  first_prose <- function(ln) {
    ln <- ln[!grepl("^\\s*(#|:::|\\||\\[\\^|\\{\\{)", ln)]
    inchunk <- FALSE
    keep <- character()
    for (l in ln) {
      if (grepl("^```", l)) { inchunk <- !inchunk; next }
      if (grepl("^---\\s*$", l)) next
      if (grepl("^title:", l)) next
      if (!inchunk) keep <- c(keep, l)
    }
    para <- character()
    for (l in keep) {
      if (!nzchar(trimws(l))) { if (length(para)) break else next }
      para <- c(para, trimws(l))
    }
    paste(para, collapse = " ")
  }
  # The question must be the opening, not merely present: 120 characters is
  # about two short sentences, room for "You have a year of sales. Which
  # product earned the most?" without letting a question four sentences down
  # count as an opening.
  for (f in teaching) {
    p <- first_prose(read_chapter(f))
    if (!grepl("\\?", substr(p, 1, 120)))
      problems <- c(problems, sprintf(
        "%s does not open with a question: %s", f, substr(p, 1, 64)))
  }

  # --- Read it aloud --------------------------------------------------------
  #
  # The gloss is an italic inline-code pipeline: *`sales then keep ...`*. Ten
  # lines after the first executable tabset's closing fence is the window:
  # room for the fence, a blank line and a sentence, not enough for a later
  # example's gloss to satisfy an earlier example's rule.
  for (f in teaching) {
    ln <- read_chapter(f)
    fences <- grep("^\\s*```", ln)
    first_example <- NA_integer_
    if (length(fences) >= 2) {
      for (k in seq(1, length(fences) - 1, by = 2)) {
        opener <- ln[fences[k]]
        body <- ln[fences[k]:fences[k + 1]]
        if (grepl("^\\s*```\\{r\\}", opener) &&
            !any(grepl("error:\\s*true|include:\\s*false", body))) {
          first_example <- fences[k + 1]
          break
        }
      }
    }
    if (is.na(first_example)) next               # a chapter may run nothing
    # The gloss follows the *tabset*, not the R chunk: the Python tab and the
    # closing `:::` sit between the first chunk and the gloss, so the window
    # opens at the tabset's closing fence when there is one.
    after <- ln[first_example:min(length(ln), first_example + 25)]
    closes <- which(grepl("^\\s*:::\\s*$", after))
    anchor <- if (length(closes)) first_example + closes[1] - 1 else first_example
    window <- ln[anchor:min(length(ln), anchor + 10)]
    if (!any(grepl("\\*`[^`]+`\\*", window)))
      problems <- c(problems, sprintf(
        "%s:%d first example has no read-aloud gloss within 10 lines of its tabset",
        f, first_example))
  }

  # --- A small cast ---------------------------------------------------------
  #
  # The share of pipelines that start from a cast table, over every pipeline
  # head in every executable chunk of the whole book. Chapter-local tables are
  # deliberately in the denominator; they are what the share is a share of.
  cast <- c("sales", "products", "survey", "answers", "marks", "messy",
            "diary", "gapminder")
  heads <- character()
  for (f in listed) {
    ln <- read_chapter(f)
    inchunk <- FALSE
    for (l in ln) {
      if (grepl("^\\s*```\\{(r|python)\\}", l)) { inchunk <- TRUE; next }
      if (grepl("^\\s*```\\s*$", l)) { inchunk <- FALSE; next }
      if (!inchunk) next
      m <- regmatches(l, regexec("^\\s*\\(?(?:collect\\()?\\s*([A-Za-z_][A-Za-z0-9_]*)\\s*(\\|>|>>)", l,
                                 perl = TRUE))[[1]]
      if (length(m) == 3) heads <- c(heads, m[2])
    }
  }
  if (length(heads)) {
    share <- mean(heads %in% cast)
    if (share < 0.75)
      problems <- c(problems, sprintf(
        "the cast carries %.0f%% of pipelines; the preface claims three in four",
        share * 100))
  } else {
    share <- NA_real_
  }

  if (length(problems)) {
    for (p in problems) message("  ", p)
    fail(sprintf("FAIL: the book breaks %d rule(s) its preface states (listed above)",
                 length(problems)))
  }
  message(sprintf(
    "check_promises: OK (%d teaching chapters open with a question and gloss their first example; the cast carries %.0f%% of pipelines)",
    length(teaching), share * 100))
  invisible(TRUE)
}

if (sys.nframe() == 0L) check_promises()
