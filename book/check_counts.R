# Every prose count of something the engine can count, checked against the engine.
#
# Run from the repository root; sourced by the R test suite.
#
# **This is the defect class that has cost this book the most, and no guard could
# see it.** A sentence says "the ten aggregations" beside a vocabulary the engine
# grew to eleven that morning. The page renders, the pipeline parses, every other
# check passes, and the number is a copy of the vocabulary written in a place
# nothing reads. It has happened four times across this book and its sibling:
# "the ten aggregations" in two files, "four conversions that say what they give"
# after `to_whole` left, and "one conditional" after `look_up` arrived.
#
# **What it checks is narrow on purpose.** Only nouns the engine can be asked
# about — verbs, aggregations, functions, grammar words, windows, scalars, and
# the `to_` conversions, which are derived rather than listed. A count of
# anything else (chapters, refusals, rows in a printed table) is not checked
# here, because nothing can answer it without running the thing that produces it.
#
# **How a real count is told from a passing mention.** The book says "one verb"
# and "two words" constantly, and those are local claims rather than counts of
# the vocabulary. Flagging them would make this check useless. So a claim is
# reported only when it sits *near* the engine's own number without matching it:
# within NEAR of the truth. That is exactly the shape a count goes stale in — a
# word is added or removed and the prose is off by one or two — and it leaves
# "eight verbs now", which is a claim about the reader's progress and is nine
# away from sixteen, alone.
#
# **What that trades away, said plainly.** A count that goes stale by more than
# NEAR is missed, and so is a correct-looking number that was always wrong. This
# check narrows the class rather than closing it; the reading is still the
# reading.
#
# Matching is done with the lines joined, not one at a time. A claim can wrap
# across a line break in hard-wrapped prose, which is how `check_prose.R` came to
# be reporting a clean book while an idiom sat in it.

NEAR <- 3L

check_counts <- function(dirs = "book") {
  engine <- god_engine_for_counts()
  if (is.na(engine)) {
    cat("SKIP: the engine is not built, so no count can be checked\n")
    return(invisible(NA))
  }

  vocab <- system2(engine, "--vocabulary", stdout = TRUE)
  kind <- sub("\t.*$", "", vocab)
  word <- sub("^[^\t]*\t", "", vocab)

  n_verb <- sum(kind == "verb")
  n_agg <- sum(kind == "aggregate")
  n_win <- sum(kind == "window")
  n_sca <- sum(kind == "scalar")
  n_word <- sum(kind == "word")

  # **The nouns, and what answers each.** `conversions` is derived rather than
  # counted from a list, because that is the one that went stale: `to_whole` was
  # removed and became two roundings that do not begin `to_`, and the prose kept
  # saying four.
  truth <- c(
    verb = n_verb, verbs = n_verb,
    aggregation = n_agg, aggregations = n_agg,
    aggregate = n_agg, aggregates = n_agg,
    `function` = n_sca + n_agg + n_win, functions = n_sca + n_agg + n_win,
    word = n_word, words = n_word,
    window = n_win, windows = n_win,
    scalar = n_sca, scalars = n_sca,
    conversion = sum(grepl("^to_", word) & kind == "scalar"),
    conversions = sum(grepl("^to_", word) & kind == "scalar")
  )

  spelled <- c(one = 1, two = 2, three = 3, four = 4, five = 5, six = 6,
               seven = 7, eight = 8, nine = 9, ten = 10, eleven = 11,
               twelve = 12, thirteen = 13, fourteen = 14, fifteen = 15,
               sixteen = 16, seventeen = 17, eighteen = 18, nineteen = 19,
               twenty = 20, thirty = 30, forty = 40, fifty = 50)

  qmds <- unlist(lapply(dirs, function(d)
    list.files(d, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)))
  qmds <- qmds[!grepl("/_", qmds, fixed = TRUE)]

  number <- paste0("(", paste(c(names(spelled), "[0-9]+"), collapse = "|"), ")")
  noun <- paste0("(", paste(names(truth), collapse = "|"), ")")
  pattern <- paste0("\\b", number, "[ -]", noun, "\\b")

  bad <- character()
  checked <- 0L

  for (f in qmds) {
    lines <- readLines(f, warn = FALSE)

    # Chunks, front matter and table rows are not prose a reader reads as a
    # claim. Blanking rather than dropping keeps the line numbers honest.
    in_chunk <- FALSE
    in_yaml <- FALSE
    for (i in seq_along(lines)) {
      line <- lines[i]
      if (i == 1L && grepl("^---\\s*$", line)) { in_yaml <- TRUE; lines[i] <- ""; next }
      if (in_yaml) {
        if (grepl("^---\\s*$", line)) in_yaml <- FALSE
        lines[i] <- ""; next
      }
      if (grepl("^\\s*```", line)) { in_chunk <- !in_chunk; lines[i] <- ""; next }
      if (in_chunk || grepl("^\\s*\\|", line)) lines[i] <- ""
    }

    for (i in seq_along(lines)) {
      if (!nzchar(trimws(lines[i]))) next
      # This line joined to the next, so a claim that wraps is still one claim.
      window <- if (i < length(lines)) paste(lines[i], lines[i + 1L]) else lines[i]
      window <- tolower(gsub("[[:space:]]+", " ", window))
      hits <- gregexpr(pattern, window, perl = TRUE)[[1]]
      if (hits[1] == -1L) next
      for (h in regmatches(window, gregexpr(pattern, window, perl = TRUE))[[1]]) {
        parts <- regmatches(h, regexec(paste0("^", number, "[ -]", noun, "$"), h))[[1]]
        if (length(parts) != 3L) next
        said <- if (parts[2] %in% names(spelled)) spelled[[parts[2]]] else
                  suppressWarnings(as.integer(parts[2]))
        if (is.na(said)) next
        # Report a claim only where it begins on this line, or a wrapped one is
        # reported twice, once from each side of the break.
        nextline <- if (i < length(lines)) tolower(gsub("[[:space:]]+", " ", lines[i + 1L])) else ""
        if (grepl(h, nextline, fixed = TRUE)) next
        want <- truth[[parts[3]]]
        checked <- checked + 1L
        if (said != want && abs(said - want) <= NEAR) {
          bad <- c(bad, sprintf(
            "  %s:%d  says \"%s\", and the engine has %d",
            sub("^book/", "", f), i, h, want))
        }
      }
    }
  }

  if (length(bad)) {
    cat("FAIL: a count in the prose disagrees with the engine\n")
    cat(paste(bad, collapse = "\n"), "\n")
    cat("  A count is a copy of the vocabulary written as a number.\n",
        " Recount it, or write the sentence so it holds no number.\n", sep = "")
    stop(sprintf("check_counts: %d stale count(s)", length(bad)), call. = FALSE)
  }
  cat(sprintf("PASS: every count the engine can check agrees with it ( %d claims, %d files )\n",
              checked, length(qmds)))
  invisible(TRUE)
}

god_engine_for_counts <- function() {
  from_env <- Sys.getenv("GOD_CLI", "")
  if (nzchar(from_env) && file.exists(from_env)) return(from_env)
  directory <- normalizePath(getwd())
  repeat {
    candidate <- file.path(directory, "target", "release", "god-cli")
    if (file.exists(candidate)) return(candidate)
    parent <- dirname(directory)
    if (identical(parent, directory)) return(NA_character_)
    directory <- parent
  }
}

if (sys.nframe() == 0L) check_counts()
