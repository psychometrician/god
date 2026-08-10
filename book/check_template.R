# check_template.R — do the verb chapters follow the template they claim?
#
# `parts/six-verbs.qmd` states the template as an invariant, in the book's own
# voice: a verb chapter opens with a question, glosses its first pipeline, and
# closes with `## What travels with it` then `## What it refuses`, always last,
# always under those names. The sibling book stated the same kind of promise
# and let it drift: only four of its twelve mark chapters had a refusals
# section, spelled three different ways, until a script started checking. A
# claim stated in prose that nothing in the toolchain can check is the species
# of defect every guard in this directory exists for, so the claim is checked
# here from the day it is made.
#
# Three assertions per verb chapter:
#   1. The last two `##` sections are `## What travels with it` then
#      `## What it refuses`, in that order, spelled exactly.
#   2. The refusals section shows a refusal in both languages: at least one
#      `#| error: true` chunk (R) and at least one `except GodError` (Python).
#      check_refusals.R and check_refusals.py then prove each side really does
#      refuse, so together a verb chapter cannot claim a refusal it lacks.
#   3. The chapter ends on prose. A file whose last non-blank line is a fence
#      or a `:::` drops the reader mid-demo, and seventeen chapters once did.
#
# The list below is explicit and grows session by session as parts are brought
# up to depth; a chapter enters the list in the same change that gives it the
# template. The full census this converges on is the twelve verb-owning
# chapters.
#
# Run from the repository root; sourced by the R test suite.

check_template <- function(book = "book") {
  fail <- function(...) stop(..., call. = FALSE)

  TRAVELS <- "## What travels with it"
  REFUSES <- "## What it refuses"

  chapters <- file.path(book, "chapters", c(
    "keeping-rows.qmd",
    "choosing-columns.qmd",
    "adding-a-column.qmd",
    "sorting-and-taking.qmd",
    "summarizing-by-group.qmd",
    "renaming-and-repeats.qmd"
  ))
  missing <- chapters[!file.exists(chapters)]
  if (length(missing))
    fail("FAIL: check_template names chapters that do not exist: ",
         paste(missing, collapse = ", "))

  bad <- character()
  for (f in chapters) {
    ln <- readLines(f, warn = FALSE)

    # Headings only outside code fences: a chunk comment can start `## `.
    fence <- cumsum(grepl("^```", ln)) %% 2 == 1
    h2_at <- which(!fence & grepl("^## ", ln))

    # A `## ` with no blank line above it is not a heading: pandoc reads it as
    # lazy continuation and renders the hashes as text. The sibling book
    # shipped exactly that once, and its guard was matching the source shape
    # while the reader saw a paragraph, so the reader's shape is what is
    # asserted.
    invisible_h2 <- h2_at[h2_at > 1 & nzchar(trimws(ln[pmax(h2_at - 1, 1)]))]
    if (length(invisible_h2)) {
      bad <- c(bad, sprintf(
        "%s:%d: `%s` has no blank line before it, so pandoc renders the `##` as text",
        basename(f), invisible_h2, trimws(ln[invisible_h2])))
      next
    }
    h2 <- trimws(ln[h2_at])

    if (length(h2) < 2) {
      bad <- c(bad, sprintf("%s: only %d section(s)", basename(f), length(h2)))
      next
    }
    last_two <- tail(h2, 2)
    if (!identical(last_two, c(TRAVELS, REFUSES))) {
      bad <- c(bad, sprintf("%s: ends on %s, expected `%s` then `%s`",
                            basename(f),
                            paste(sprintf("`%s`", last_two), collapse = " then "),
                            TRAVELS, REFUSES))
      next
    }

    # The refusals section is everything from its heading to the end, it being
    # last by the check above. It must show the refusal twice, once per
    # language, because a tab that cannot fail is a tab that proves nothing.
    body <- ln[tail(h2_at, 1):length(ln)]
    if (!any(grepl("error:\\s*true", grep("^#\\|", body, value = TRUE))))
      bad <- c(bad, sprintf(
        "%s: `%s` has no `error: true` chunk; the section must show the R refusal",
        basename(f), REFUSES))
    if (!any(grepl("except GodError", body, fixed = TRUE)))
      bad <- c(bad, sprintf(
        "%s: `%s` has no `except GodError`; the section must show the Python refusal",
        basename(f), REFUSES))

    # The ending rule: the last thing on the page is a sentence, not a fence.
    last_line <- trimws(tail(ln[nzchar(trimws(ln))], 1))
    if (grepl("^(```|:::)", last_line))
      bad <- c(bad, sprintf(
        "%s: ends on `%s`; a chapter ends on prose, not mid-demo",
        basename(f), last_line))
  }

  if (length(bad))
    fail("FAIL: verb chapters that break the template `parts/six-verbs.qmd` promises:\n  ",
         paste(bad, collapse = "\n  "),
         "\n  Either fix the chapter, or stop claiming the template there.")

  # --- The ending rule, for every page --------------------------------------
  # The verb chapters carry the whole template; every other page still owes
  # the reader its last sentence. A file whose final non-blank line closes a
  # fence or a tabset drops them mid-demo, and nine pages did when this was
  # first measured. Two shapes are endings rather than defects: the
  # bibliography stub in `references.qmd`, which pandoc fills, and a styled
  # div holding a line of prose, which is how the afterword hands the motto
  # back. A tabset is neither: it is a demo with no sentence after it.
  every <- list.files(book, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)
  every <- every[!grepl("/_", every, fixed = TRUE)]
  every <- every[basename(every) != "references.qmd"]
  ends <- character()
  for (f in every) {
    ln <- readLines(f, warn = FALSE)
    last_line <- trimws(tail(ln[nzchar(trimws(ln))], 1))
    mid_demo <- grepl("^```", last_line)
    if (!mid_demo && grepl("^:::", last_line)) {
      opener <- tail(grep("^:::+\\s*\\{", ln, value = TRUE), 1)
      mid_demo <- length(opener) && grepl("panel-tabset", opener, fixed = TRUE)
    }
    if (mid_demo)
      ends <- c(ends, sprintf("%s ends on `%s`", sub("^.*book/", "", f), last_line))
  }
  if (length(ends))
    fail("FAIL: a page ends mid-demo; the last thing on a page is a sentence:\n  ",
         paste(ends, collapse = "\n  "))

  cat("PASS: every verb chapter follows the template, and every page ends on prose (",
      length(chapters), "verb chapters,", length(every), "pages )\n")
  invisible(TRUE)
}

if (sys.nframe() == 0L) check_template()
