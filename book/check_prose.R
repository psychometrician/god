# book/check_prose.R
# The book has one voice, and this is the half of it a machine can hold.
#
# It exists from the first chapter rather than being added after one. Every
# convention in a book of this kind drifts, and always the same way: not by
# anyone deciding, but *by chapter*, because a writer holds one file in their
# head for an afternoon and spells things consistently inside it. Nobody reading
# a single file ever sees the split. The sibling project found 386 violations in
# one sweep, all of them written by people who knew the rules, which is the
# argument for having the guard before the prose rather than after it.
#
# What is checked here is only what is **true or false about a line**: a bold run
# is longer than eight words or it is not, an em dash is present or it is not.
# Sentence length is deliberately absent, and belongs to `book/readability.py`,
# which is a report and must stay one. A threshold on words per sentence gets
# satisfied by splitting sentences in half rather than by rewriting them, which
# is the metric improving while the prose gets worse.
#
# Scope is the book's prose and any English the book *shows* a reader: chunk
# comments, table captions and chunk options all reach the page. It is not the
# two packages. A diagnostic string in `r-pkg/god/R/` is the package's voice and
# follows the package's conventions.
#
# Run from the repository root; sourced by the R test suite.

check_prose <- function(dirs = "book") {
  qmds <- unlist(lapply(dirs, function(d)
    list.files(d, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)))

  # One rule for every build directory rather than a list of them: `_book/` and
  # `_freeze/` are output, and a `_`-prefixed file is one the author has
  # deliberately withheld. This is the same test `readability.py` applies.
  qmds <- qmds[!grepl("/_", qmds, fixed = TRUE)]
  if (!length(qmds)) {
    cat("PASS: prose is consistent ( 0 files )\n")
    return(invisible(TRUE))
  }

  # Idiom. The list is a probe rather than a boundary: a phrase meaning something
  # other than the sum of its words is an idiom whether or not it is named here.
  # The translation test is what makes this a rule instead of a preference. An
  # idiom does not survive into another language; it becomes nonsense, or every
  # translator invents a different replacement.
  #
  # Matching is `fixed = TRUE`, so each inflection has to be written out. There
  # is no stem that catches them all, and a gerund walks straight past the
  # present tense: "earning its keep" passed a guard that already listed "earns
  # its keep".
  idioms <- c(
    "earns its keep", "earn their keep", "earning its keep", "earning their keep",
    "earn its keep", "earned its keep", "earns its place", "earn its place",
    "earning its place", "earns a place", "earn a place",
    "pays off", "the giveaway", "under the hood", "out of the box",
    "rule of thumb", "at the end of the day", "boils down", "in a nutshell",
    "hand in hand", "bread and butter", "low-hanging", "heavy lifting",
    "moving parts", "sweet spot", "silver bullet", "best of both worlds",
    "from scratch", "bells and whistles", "apples to apples", "cuts both ways",
    "no free lunch", "elephant in the room", "tip of the iceberg",
    "second nature", "load-bearing", "the expert's convenience",
    "worth reaching for", "reach for it", "reach for this", "reach for that",
    "ships with", "ship with", "consolation prize",
    "needing an apology", "needs an apology", "catches people out", "caught out",
    "takes doing", "getting away with", "on the roadmap",
    "the exception that proves", "part company", "parts company",
    "falls behind", "fall behind", "held back", "the good news", "the bad news",
    "on the fly", "down the line", "across the board", "go a long way",
    "goes a long way", "comes down to", "a far cry", "by and large",
    "hit the ground", "off the shelf", "battle-tested", "no small feat",
    "can of worms", "sharp edges", "rough edges", "win-win", "game-changer",
    "game-changing", "move the needle", "first-class citizen", "nuts and bolts",
    "tells a different story", "tell a different story", "fell back",
    "falls back", "leaves the other alone",
    "tells apart", "tell apart", "told apart", "telling apart",
    "tells them apart", "tell them apart",
    # Not an idiom but a fixed shape, and worth making mechanical: it is always
    # ambiguous (does "three times shorter" mean a third, or three times as
    # long?) and translators split on it. The literal form is "less than a third
    # as long".
    "times shorter", "times smaller", "times lower", "times fewer", "times less"
  )

  # `—` is also a *glyph* in a table legend, where it sits beside other symbols
  # and means "this does not apply". A symbol is not punctuation, so those uses
  # stay, and they are recognized by shape rather than by filename: the whole
  # content of a string in code, or a table cell that is only the glyph.
  # Anything else on the line is still checked.
  ungl <- function(s) {
    s <- gsub("\"—\"", "\"\"", s)                             # "—", the glyph as a value
    s <- gsub("\\*\\*—", "**", s)                               # **— none**, a legend label
    s <- gsub("^\\s*#\\s+—\\s", "# ", s)                      # a comment defining the symbol
    s <- gsub("\\|\\s*—\\s*(?=\\|)", "| ", s, perl = TRUE)    # a cell that is the glyph
    s
  }

  # The laws carry official names, set in the design document. `## Law 5: Derive,
  # don't enumerate` is that name rather than Title Case drift, and rewriting it
  # here would desync the chapter from the law.
  law_heading <- "^#+\\s*Law\\s+[0-9]"

  # Title Case is found by capitalization, so proper nouns have to be named or
  # every one of them is a false positive. Keep this list short and concrete: it
  # is cheaper to add a name here than to weaken the rule into uselessness.
  # `god` is deliberately absent. This list whitelists *capitalized* words, and
  # the package name is lowercase everywhere, so an entry for it could never
  # match the `^[A-Z]` test below.
  #
  # `I` is here and is not a proper noun. English capitalizes the pronoun
  # everywhere it appears, so it can never be evidence of Title Case, and left
  # out it reports any heading containing it. One section of the preface is
  # written in the first person on purpose.
  proper <- c("I",
              "R", "Python", "Rust", "SQL", "CSV", "JSON", "Parquet", "Arrow",
              "Quarto", "Quarto's", "Posit", "CRAN", "PyPI", "ISO", "UTF",
              "Jupyter", "RStudio", "Windows", "macOS", "Linux",
              "Anthropic", "Wilkinson", "Codd", "Wickham",
              "Korean", "English", "American", "Law", "Part", "Nine", "Laws",
              "R's", "Python's", "god's")

  MAX_HEADING <- 8
  MAX_BOLD <- 8

  # Strip inline code before counting anything. A code span is one name however
  # many spaces are inside it. Lowercase on purpose: an uppercase placeholder
  # reads as a capitalized word to the Title Case rule below, and every heading
  # naming a verb becomes a hit.
  strip_code <- function(s) gsub("`[^`]*`", "code", s)

  nwords <- function(s) {
    s <- strip_code(s)
    s <- gsub("[*_]", "", s)
    s <- trimws(gsub("\\s+", " ", s))
    if (!nzchar(s)) return(0L)
    length(strsplit(s, " ", fixed = TRUE)[[1]])
  }

  bad_bold <- character(0); bad_dash <- character(0); bad_head <- character(0)
  bad_case <- character(0); bad_call <- character(0); bad_idiom <- character(0)

  for (f in qmds) {
    lines <- readLines(f, warn = FALSE)
    short <- sub("^.*book/", "", f)
    in_chunk <- FALSE
    in_yaml <- FALSE
    where <- function(i) sprintf("  %s:%d  %s", short, i, trimws(lines[i]))

    # A bold run is tracked across lines, because prose here is hard-wrapped at
    # about 80 characters and a bolded sentence is usually longer than that. A
    # check that reads one line at a time silently misses every wrapped one,
    # which is the same blind spot that hides a quarter of the hits in any
    # line-based grep over this book.
    b_open <- FALSE; b_start <- NA_integer_; b_text <- ""; b_item <- FALSE

    for (i in seq_along(lines)) {
      line <- lines[i]

      # YAML front matter carries the chapter title, which is prose a reader sees.
      if (i == 1L && grepl("^---\\s*$", line)) { in_yaml <- TRUE; next }
      if (in_yaml) {
        if (grepl("^---\\s*$", line)) in_yaml <- FALSE
        else if (grepl("—", line)) bad_dash <- c(bad_dash, where(i))
        next
      }

      if (grepl("^\\s*```", line)) { in_chunk <- !in_chunk; next }

      # Inside a chunk, an em dash still reaches the reader: in a comment they
      # read, or in a `#| tbl-cap` rendered as a caption.
      if (in_chunk) {
        if (grepl("—", ungl(line))) bad_dash <- c(bad_dash, where(i))
        next
      }

      if (grepl("^\\s*:::\\s*\\{?\\.callout", line)) {
        bad_call <- c(bad_call, where(i)); next
      }

      # --- Headings ---------------------------------------------------------
      if (grepl("^#+\\s+\\S", line)) {
        h <- sub("^#+\\s+", "", line)
        h <- gsub("\\{#[^}]*\\}", "", h)   # explicit anchors are not words
        n <- nwords(h)
        # A heading is prose a reader sees, so it is checked for the em dash too.
        if (grepl("—", ungl(strip_code(h)))) bad_dash <- c(bad_dash, where(i))
        # Two sentences in a heading is the same defect as an over-long one: the
        # argument has climbed out of the paragraph and into the label. A capital
        # after the stop is what marks a second sentence; requiring it keeps
        # "Table-level vs. step-level data" out of the report.
        if (n > MAX_HEADING || grepl("[.!?]\\s+[A-Z]", h))
          bad_head <- c(bad_head, sprintf("  %s:%d  [%dw] %s", short, i, n, trimws(h)))
        if (!grepl(law_heading, line)) {
          # A word opening a second sentence is capitalized by grammar rather
          # than by Title Case, so it is neutralized before the count.
          h2 <- gsub("([.!?])\\s+[A-Z]", "\\1 x", strip_code(h))
          words <- strsplit(trimws(gsub("[^A-Za-z' ]", " ", h2)), "\\s+")[[1]]
          words <- words[nzchar(words)]
          if (length(words) > 1L) {
            capd <- words[-1][grepl("^[A-Z]", words[-1]) & !(words[-1] %in% proper)]
            if (length(capd))
              bad_case <- c(bad_case, sprintf("  %s:%d  %s   (%s)", short, i,
                                              trimws(h), paste(capd, collapse = ", ")))
          }
        }
        next
      }

      prose <- strip_code(line)

      # --- Em dash ----------------------------------------------------------
      if (grepl("—", ungl(prose))) bad_dash <- c(bad_dash, where(i))

      # --- Idiom ------------------------------------------------------------
      low <- tolower(line)
      for (p in idioms) {
        if (grepl(p, low, fixed = TRUE))
          bad_idiom <- c(bad_idiom, sprintf("  %s:%d  \"%s\"", short, i, p))
      }

      # --- Bolded sentences -------------------------------------------------
      # A short bold run-in label may open a *list item*, with the terminal
      # period inside the bold. That is a layout device, and it is the only
      # carve-out: a bold label opening an ordinary paragraph is emphasis and
      # goes. Anything longer, or anywhere else, is a sentence wearing bold, and
      # a bolded sentence reads as a box.
      if (!nzchar(trimws(line))) {           # a blank line ends a paragraph, so
        b_open <- FALSE; b_text <- ""        # an unmatched `**` cannot run away
      } else {
        # `strsplit` drops trailing empty fields, so a line *ending* in `**`
        # comes back one segment short and the closing delimiter is never seen.
        # The run then stays open and swallows the next line. Count the
        # delimiters and pad instead of trusting the split.
        ndelim <- if (grepl("**", line, fixed = TRUE))
          length(gregexpr("**", line, fixed = TRUE)[[1]]) else 0L
        segs <- strsplit(line, "**", fixed = TRUE)[[1]]
        if (length(segs) < ndelim + 1L)
          segs <- c(segs, rep("", ndelim + 1L - length(segs)))
        nseg <- length(segs)
        for (j in seq_len(nseg)) {
          if (b_open) b_text <- paste0(b_text, segs[j])
          if (j < nseg) {                    # a `**` delimiter follows
            if (b_open) {
              n <- nwords(b_text)
              punct <- grepl("[.!?]\\s+[A-Z]", b_text) ||
                (grepl("[.!?]$", trimws(b_text)) && n > 3L)
              if ((n > MAX_BOLD || punct) && !(b_item && n <= MAX_BOLD))
                bad_bold <- c(bad_bold, sprintf("  %s:%d  [%dw] **%s**",
                                                short, b_start, n, trimws(b_text)))
              b_open <- FALSE; b_text <- ""
            } else {
              b_open <- TRUE; b_start <- i; b_text <- ""
              # A run-in label is the first bold on a list-item line.
              b_item <- (j == 1L) && grepl("^\\s*([-*+]|[0-9]+\\.)\\s+$", segs[1])
            }
          }
        }
        if (b_open) b_text <- paste0(b_text, " ")   # the run crosses a line break
      }
    }
  }

  total <- length(bad_bold) + length(bad_dash) + length(bad_head) +
    length(bad_case) + length(bad_call) + length(bad_idiom)

  report <- function(items, headline, advice) {
    if (!length(items)) return(invisible(NULL))
    cat(headline, "\n", sep = "")
    cat(paste(items, collapse = "\n"), "\n", sep = "")
    cat("  ", advice, "\n", sep = "")
  }

  if (total) {
    report(bad_bold, "FAIL: a whole sentence is set in bold",
           "Bold introduces a term; it does not emphasize a sentence. Set it plain.")
    report(bad_dash, "FAIL: em dash in text a reader sees",
           "Use a comma, a colon, a semicolon, parentheses, or two sentences.")
    report(bad_head, "FAIL: heading is a sentence, not a label",
           "A short noun phrase, eight words at most. The argument goes in the paragraph.")
    report(bad_case, "FAIL: heading is in Title Case",
           "Sentence case: 'Window expressions', not 'Window Expressions'.")
    report(bad_call, "FAIL: callout box",
           "Weave the point into the prose, or let the output show it.")
    report(bad_idiom, "FAIL: idiom does not translate",
           "Say the literal thing.")
    stop("check_prose: ", total, " prose inconsistency(ies)")
  }

  cat("PASS: prose is consistent (", length(qmds), "files )\n")
  invisible(TRUE)
}

# Run standalone as well as sourced, so the guard is usable before there is a
# test suite to source it from.
if (sys.nframe() == 0L) check_prose()
