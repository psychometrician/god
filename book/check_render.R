# check_render.R — the defects that exist only in the rendered book.
#
# **Every other guard in this directory reads `.qmd`, and all four of the ones
# that name `_book/` name it to skip it.** So a whole class of defect has never
# had a local check: the ones pandoc and Quarto introduce on the way out, which
# are invisible in the source and obvious on the page.
#
# The publishing workflow greps the render for exactly these, and until now it
# was the only thing that did. That put them on the far side of the push that
# publishes: the first person to see a wrongly numbered heading was a reader.
# This is that grep, brought forward to where a preview can catch it.
#
# Five assertions over `book/_book/`:
#   1. `index.html` exists and references `site_libs`, which is where Quarto
#      writes the stylesheets and the search index. A site missing it returns
#      200 on every page and looks broken.
#   2. No heading numbered `N.0.x`. That is a `###` under a chapter with no
#      `##` above it: Quarto numbers it against a section 19.1 that does not
#      exist, and `toc-depth: 2` keeps it out of the sidebar as well. Thirteen
#      shipped in eight chapters.
#   3. No HTML entity inside a code span. Pandoc decodes entities in prose and
#      leaves them alone in code, so a table cell escaping a pipe to `&#124;`
#      reaches the reader as those six characters. Eight did.
#   4. No Python repr with its newlines spelled out in a cell's output, which
#      is what one binding printing as well as returning looked like.
#   5. No PDF, which would make Quarto offer the whole book as a download.
#   6. No virtualenv. `book/.venv` is inside the Quarto project, so Quarto
#      copied it into `_book/` and the publish pushed 237 files of somebody
#      else's Python to the public site.
#
# **A skip is not a pass, and this says so in those words.** Where there is no
# render, or where the render is older than the sources it came from, grading it
# would be grading a different book. Both cases report what they did instead of
# quietly counting as a pass.
#
# Run from the repository root; sourced by the R test suite.

check_render <- function(book = "book") {
  fail <- function(...) stop(..., call. = FALSE)

  out <- file.path(book, "_book")
  index <- file.path(out, "index.html")

  if (!file.exists(index)) {
    cat("SKIP: check_render found no render at", out,
        "- nothing was graded. `cd book && quarto render --to html` makes it real\n")
    return(invisible(TRUE))
  }

  # **A stale render is worse than none**, because it passes. It can carry a
  # defect that is already fixed, or miss one that has just arrived, and either
  # way the answer describes a book nobody has. The engine counts as a source:
  # every table in the book is computed by that binary, so one core change
  # invalidates every page at once.
  sources <- c(
    list.files(book, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE),
    file.path(book, "_quarto.yml"),
    "target/release/god-cli"
  )
  sources <- sources[file.exists(sources)]
  sources <- sources[!grepl("/_book/", sources, fixed = TRUE)]
  newest <- suppressWarnings(max(file.mtime(sources)))
  if (is.finite(newest) && newest > file.mtime(index)) {
    behind <- sources[file.mtime(sources) > file.mtime(index)]
    cat("SKIP: the render is older than", length(behind),
        "of its sources, so it was not graded. Newest:",
        basename(behind[which.max(file.mtime(behind))]), "\n")
    return(invisible(TRUE))
  }

  pages <- list.files(out, pattern = "[.]html$", recursive = TRUE, full.names = TRUE)
  # Read once. Every assertion below is a pattern over the same text, and
  # reading 55 files once per assertion is five times the work for one answer.
  html <- setNames(lapply(pages, function(f) readLines(f, warn = FALSE)), pages)

  bad <- character()

  hit <- function(pattern, ...) {
    found <- character()
    for (page in names(html)) {
      lines <- grep(pattern, html[[page]], value = TRUE, ...)
      if (length(lines)) {
        found <- c(found, sprintf("%s: %s", sub("^.*_book/", "", page),
                                  substr(trimws(lines[1]), 1, 120)))
      }
    }
    found
  }

  if (!any(grepl("site_libs", html[[index]], fixed = TRUE))) {
    bad <- c(bad, paste(
      "index.html does not reference site_libs, so the stylesheets and the",
      "search index are not reaching the page"))
  }

  numbered <- hit('<h[2-6][^>]*>[^<]*<span class="header-section-number">[0-9]+[.]0[.]')
  if (length(numbered)) {
    bad <- c(bad, paste0(
      "a subsection is numbered N.0.x, which means a `###` with no `##` above it:\n    ",
      paste(head(numbered, 5), collapse = "\n    ")))
  }

  entities <- hit("<code>[^<]*&amp;#[0-9]+;")
  if (length(entities)) {
    bad <- c(bad, paste0(
      "an HTML entity is showing inside a code span, so the reader sees its\n  characters:\n    ",
      paste(head(entities, 5), collapse = "\n    ")))
  }

  # **Matched literally, not as a pattern, and that is the whole point.** What
  # is being looked for is the two characters backslash and `n` inside a quoted
  # code span. Written as a regex, the backslash has to survive the R parser and
  # then the regex engine, and getting either layer wrong makes the check match
  # nothing at all while still reporting a pass. The first version of this line
  # did exactly that, and it was caught by breaking the thing it guards.
  reprs <- character()
  for (page in names(html)) {
    quoted <- grep("<code>&#39;", html[[page]], value = TRUE)
    escaped <- quoted[grepl("\\n", quoted, fixed = TRUE)]
    if (length(escaped)) {
      reprs <- c(reprs, sprintf("%s: %s", sub("^.*_book/", "", page),
                                substr(trimws(escaped[1]), 1, 120)))
    }
  }
  if (length(reprs)) {
    bad <- c(bad, paste0(
      "a Python repr with its newlines spelled out is in a cell output:\n    ",
      paste(head(reprs, 5), collapse = "\n    ")))
  }

  pdfs <- list.files(out, pattern = "[.]pdf$", recursive = TRUE)
  if (length(pdfs)) {
    bad <- c(bad, paste0(
      "a PDF is in the render, which makes Quarto offer the book as a download: ",
      paste(head(pdfs, 3), collapse = ", ")))
  }

  # **A virtualenv in the render is published to the public site.**
  # `book/.venv` sits inside the Quarto project, so Quarto's resource discovery
  # copied it into `_book/` and the workflow force-pushed all of `_book/` to
  # `god-book` — 237 files and 2.8 MB of somebody else's Python, on a site whose
  # own README says it holds rendered HTML and nothing else. It was there from
  # the day the book grew a virtualenv until 2026-08-12.
  #
  # `site-packages` is asked for as well as `.venv`, because the name of the
  # directory is a developer's choice and the thing inside it is not.
  strays <- c(
    list.files(out, pattern = "^[.]venv$", include.dirs = TRUE, all.files = TRUE),
    basename(dirname(list.files(out, pattern = "^site-packages$",
                                recursive = TRUE, include.dirs = TRUE)))
  )
  if (length(strays)) {
    bad <- c(bad, paste0(
      "a virtualenv is in the render, and the whole render is published: ",
      paste(unique(head(strays, 3)), collapse = ", ")))
  }

  if (length(bad))
    fail("FAIL: the rendered book has defects its sources do not show:\n  ",
         paste(bad, collapse = "\n  "))

  cat("PASS: the rendered book is clean (", length(pages), "pages )\n")
  invisible(TRUE)
}

if (sys.nframe() == 0L) check_render()
