# Every word the grammar has must be demonstrated by a chunk that runs.
#
# Run from the repository root; sourced by the R test suite.
#
# **The manual was incomplete and nothing said so.** When the book was split into
# chapters, five words turned out to appear nowhere in it at all, and seven more
# appeared only in prose. A sentence can name a verb and still build clean, which
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

check_vocabulary <- function(dirs = "book") {
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

  if (length(missing_run) || length(missing_any)) {
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
    stop(sprintf("check_vocabulary: %d word(s) undocumented",
                 length(missing_run) + length(missing_any)), call. = FALSE)
  }

  cat("PASS: every word the grammar has is demonstrated (",
      nrow(words), "words,", length(qmds), "files )\n")
  invisible(TRUE)
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
