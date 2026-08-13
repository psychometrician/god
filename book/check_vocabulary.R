# Every word the grammar has must be demonstrated by a chunk that runs.
#
# Run from the repository root; sourced by the R test suite.
#
# **The manual was incomplete and nothing said so.** When the book was split into
# chapters, a handful of words turned out to appear nowhere in it at all, and
# more appeared only in prose. A sentence can name a verb and still build clean, which
# is the defect no live chunk catches, and the inverse holds too: a grammar can
# grow a word and no page has to mention it.
#
# So the engine is asked what words it has, and each one has to turn up inside an
# executable chunk. Prose does not count. A word a reader has never seen *used*
# is a word this book has not taught, whatever it says about it.
#
# Verbs and functions are the ones required to run, because each of them is
# something a reader writes. The grammar words are checked more loosely: several
# of them are also R or Python keywords, so `in` and `is` and `not` would match
# host syntax rather than the grammar and the check would pass without meaning
# anything. Those have to appear somewhere in the book, which is what catches a
# word nobody documented at all.

check_vocabulary <- function(dirs = "book", readme = "README.md") {
  engine <- god_engine_for_vocabulary()
  if (is.na(engine)) {
    cat("SKIP: the engine is not built, so the vocabulary is unchecked\n")
    return(invisible(NA))
  }

  qmds <- unlist(lapply(dirs, function(d)
    list.files(d, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)))
  # The same rule the other guards apply: `_book/` is output, and a
  # `_`-prefixed file is one the author has deliberately withheld.
  qmds <- qmds[!grepl("/_", qmds, fixed = TRUE)]
  if (!length(qmds)) {
    cat("PASS: no chapters to check\n")
    return(invisible(TRUE))
  }

  words <- read.delim(
    text = system2(engine, "--vocabulary", stdout = TRUE),
    header = FALSE, col.names = c("kind", "word"), stringsAsFactors = FALSE
  )

  running <- character()
  anywhere <- character()
  for (f in qmds) {
    lines <- readLines(f, warn = FALSE)
    in_chunk <- FALSE
    for (line in lines) {
      # An executable chunk opens with a brace: ```{r} or ```{python}. A bare
      # fence or a language-tagged one is prose showing syntax, and that is
      # exactly what this guard refuses to accept as documentation.
      if (grepl("^\\s*```\\{", line)) {
        in_chunk <- TRUE
        next
      }
      if (grepl("^\\s*```\\s*$", line)) {
        in_chunk <- FALSE
        next
      }
      if (in_chunk) running <- c(running, line)
      anywhere <- c(anywhere, line)
    }
  }
  running <- paste(running, collapse = "\n")
  anywhere <- paste(anywhere, collapse = "\n")

  # `\b` on both sides, so `first` does not match inside `first_present`: an
  # underscore is a word character, so there is no boundary between them.
  mentions <- function(word, text) {
    grepl(paste0("\\b", word, "\\b"), text, perl = TRUE)
  }

  missing_run <- character()
  missing_any <- character()
  for (i in seq_len(nrow(words))) {
    word <- words$word[i]
    kind <- words$kind[i]
    if (kind == "word") {
      if (!mentions(word, anywhere)) missing_any <- c(missing_any, word)
    } else {
      if (!mentions(word, running)) missing_run <- c(missing_run, word)
    }
  }

  # The front page keeps its own copy of the vocabulary, and a hand copy that
  # no check reads is the copy that rots: the README claimed "nothing has to
  # keep a second copy in step" while being exactly that copy, and it was
  # missing seventeen of the grammar words when this was first measured. The
  # README runs nothing, so appearing anywhere in it is the whole bar.
  missing_front <- character()
  wrong_count <- character()
  if (length(readme) && file.exists(readme)) {
    front <- paste(readLines(readme, warn = FALSE), collapse = "\n")
    for (word in words$word) {
      if (!mentions(word, front)) missing_front <- c(missing_front, word)
    }
    wrong_count <- counts_that_disagree(front, words, readme)
  }

  if (length(wrong_count)) {
    cat("FAIL: the front page counts the vocabulary wrong\n")
    for (line in wrong_count) cat("  ", line, "\n", sep = "")
    cat("  A count is a copy of the vocabulary written as a number.\n")
  }

  if (length(missing_front)) {
    cat("FAIL: the front page never shows a word the grammar has\n")
    for (word in missing_front) {
      cat("  ", word, " is in the vocabulary and appears nowhere in ", readme, "\n", sep = "")
    }
  }

  if (length(missing_run) || length(missing_any) || length(missing_front) ||
      length(wrong_count)) {
    if (length(missing_run)) {
      cat("FAIL: the grammar has words this book never runs\n")
      for (word in missing_run) {
        cat("  ", word, " is in the vocabulary and appears in no executable chunk\n", sep = "")
      }
      cat("  Write an example that uses it. Naming it in prose is not documenting it.\n")
    }
    if (length(missing_any)) {
      cat("FAIL: the grammar has words this book never mentions\n")
      for (word in missing_any) {
        cat("  ", word, " is in the vocabulary and appears nowhere in the book\n", sep = "")
      }
    }
    stop(sprintf("check_vocabulary: %d undocumented word(s), %d wrong count(s)",
                 length(missing_run) + length(missing_any) + length(missing_front),
                 length(wrong_count)),
         call. = FALSE)
  }

  cat("PASS: every word the grammar has is demonstrated (",
      nrow(words), "words,", length(qmds),
      "files, and the README names them all and counts them right )\n")
  invisible(TRUE)
}

# A count is a copy of the vocabulary written as a number, and it rots more
# quietly than a list does, because every word is still named on the page. The
# README said "thirty-one functions" from the day `join_text` landed, and the
# check above passed the whole time — `join_text` *is* named there, and nothing
# was counting.
#
# Only a numeral standing in front of the noun is read as a claim. "the verbs"
# and "same verbs" are prose, convert to nothing, and are passed over, so the
# page can go on saying those without owing a number.
counts_that_disagree <- function(front, words, readme) {
  ones <- c(one = 1, two = 2, three = 3, four = 4, five = 5, six = 6, seven = 7,
            eight = 8, nine = 9, ten = 10, eleven = 11, twelve = 12,
            thirteen = 13, fourteen = 14, fifteen = 15, sixteen = 16,
            seventeen = 17, eighteen = 18, nineteen = 19)
  tens <- c(twenty = 20, thirty = 30, forty = 40, fifty = 50, sixty = 60,
            seventy = 70, eighty = 80, ninety = 90)

  as_count <- function(token) {
    token <- tolower(token)
    if (grepl("^[0-9]+$", token)) return(as.integer(token))
    if (!is.na(ones[token])) return(unname(ones[token]))
    if (!is.na(tens[token])) return(unname(tens[token]))
    parts <- strsplit(token, "-", fixed = TRUE)[[1]]
    if (length(parts) == 2L && !is.na(tens[parts[1]]) && !is.na(ones[parts[2]])) {
      return(unname(tens[parts[1]] + ones[parts[2]]))
    }
    NA_integer_
  }

  # What the engine calls a verb, and everything it calls a function: the three
  # kinds a reader writes with brackets after them.
  expected <- c(verbs = sum(words$kind == "verb"),
                functions = sum(words$kind %in% c("scalar", "aggregate", "window")))

  wrong <- character()
  for (noun in names(expected)) {
    phrases <- regmatches(
      front, gregexpr(paste0("[[:alnum:]-]+ ", noun), front, perl = TRUE))[[1]]
    for (phrase in phrases) {
      said <- as_count(sub(paste0(" ", noun, "$"), "", phrase))
      if (is.na(said)) next
      if (said != expected[[noun]]) {
        wrong <- c(wrong, sprintf('%s says "%s" and the grammar has %d',
                                  readme, phrase, expected[[noun]]))
      }
    }
  }
  wrong
}

# `$GOD_CLI` if it is set, else walk up looking for the built binary. The same
# thing the other guard does, and the same thing both packages do.
god_engine_for_vocabulary <- function() {
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

if (sys.nframe() == 0L) check_vocabulary()
