# book/check_grammar.R
# Every pipeline the book shows, handed to the grammar that has to read it.
#
# A live chunk proves the code in it works, and that is most of the book. What it
# cannot reach is the code the book **shows without running**: the text form,
# which has no chunk engine yet, and the shell transcripts. Those are ordinary
# fenced blocks, so they render cleanly whatever they say, and prose that renders
# cleanly is prose nobody checks.
#
# That is not a hypothetical gap. The preface showed `is 'West'` in two places,
# which the grammar refuses and says so with a caret, and documented a
# `god show --as dplyr` subcommand that has never existed. Both survived every
# render the book had ever had, because neither was in a chunk.
#
# So each block is given to the engine instead of being read by a person:
#
#   a pipeline      `--needs`, which parses and checks the vocabulary
#   a transcript    its flags against `--help`, and the pipeline inside it
#
# `--needs` is the right gate because of what it does **not** do. It reads the
# whole sentence and refuses an unknown verb or a host habit, and it says nothing
# about columns, so a pipeline written against a table the book never defines
# passes rather than producing a false alarm. A guard that cried wolf on every
# illustrative example would be turned off within a week.
#
# Run from the repository root; sourced by the R test suite.

check_grammar <- function(dirs = "book", files = "README.md", binary = NULL) {
  if (is.null(binary)) binary <- god_engine()
  if (is.null(binary)) {
    cat("SKIP: the engine is not built, so no pipeline in the book was checked\n")
    return(invisible(NA))
  }

  qmds <- unlist(lapply(dirs, function(d)
    list.files(d, pattern = "[.]qmd$", recursive = TRUE, full.names = TRUE)))
  # The same rule the other guards apply: `_book/` is output, and a
  # `_`-prefixed file is one the author has deliberately withheld.
  qmds <- qmds[!grepl("/_", qmds, fixed = TRUE)]
  # The root README shows the same transcript the preface does, and it sat
  # outside this guard for as long as the guard only read `book/` — which is
  # how the exact defect class named in the header above survived there after
  # it had been killed in the preface. Markdown is read the same way `.qmd` is.
  qmds <- c(qmds, files[file.exists(files)])

  # Every flag the command line actually has, read from the command line rather
  # than written down here. A list restated in a guard is a list that goes stale
  # the first time one is added.
  flags <- god_flags(binary)

  bad <- character(0)
  checked <- 0L

  for (f in qmds) {
    lines <- readLines(f, warn = FALSE)
    short <- sub("^.*book/", "", f)

    for (block in fenced_blocks(lines)) {
      found <- check_block(block, binary, flags, short)
      checked <- checked + found$checked
      bad <- c(bad, found$bad)
    }

    # A pipeline written inline, in the middle of a sentence.
    for (i in seq_along(lines)) {
      for (span in inline_code(lines[i])) {
        if (!looks_like_pipeline(span)) next
        checked <- checked + 1L
        refusal <- god_refusal(binary, span)
        if (!is.null(refusal)) {
          bad <- c(bad, sprintf("  %s:%d  `%s`\n%s", short, i, span, indent(refusal)))
        }
      }
    }
  }

  if (length(bad)) {
    cat("FAIL: the book shows a pipeline the grammar refuses\n")
    cat(paste(bad, collapse = "\n"), "\n", sep = "")
    cat("  Fix the example, or the grammar. A manual is not allowed to be wrong.\n")
    stop("check_grammar: ", length(bad), " refused example(s)")
  }

  cat("PASS: every pipeline the book shows is one the grammar reads (",
      checked, "examples )\n")
  invisible(TRUE)
}

# -- what a block is, and what to do with it ---------------------------------

check_block <- function(block, binary, flags, short) {
  bad <- character(0)
  body <- block$lines
  at <- block$start

  if (!length(body)) return(list(bad = bad, checked = 0L))

  # A shell transcript. The command is checked for flags that exist and for the
  # one argument it is allowed, and then the pipeline inside it is checked like
  # any other.
  prompts <- grep("^\\s*\\$\\s+god\\b", body)
  if (length(prompts)) {
    checked <- 0L
    ends <- c(prompts[-1L] - 1L, length(body))
    for (k in seq_along(prompts)) {
      i <- prompts[[k]]
      command <- sub("^\\s*\\$\\s+", "", body[i])
      complaint <- check_command(command, flags)
      if (!is.null(complaint)) {
        bad <- c(bad, sprintf("  %s:%d  %s\n%s", short, at + i - 1L, command, indent(complaint)))
        next
      }
      # **Then the command is run, and the page's output has to be the
      # output.** Parsing alone let a transcript through that exits 1: every
      # flag was real and the pipeline parsed, and the command still refused
      # to answer, because `--columns` was missing. So a transcript is
      # executed the way every chunk in the book is, and the lines under the
      # prompt are compared against what the command printed.
      #
      # A transcript whose shown output opens `illegal:` means to show a
      # refusal, so the expectation flips whole: exit 2, and the diagnostic
      # on stderr compared byte for byte, which is what keeps a quoted
      # message the message the engine prints. An assumption transcript
      # (exit 0, a note on stderr) has no convention yet; the first one
      # added will fail here, and that is the moment to design one.
      shown <- if (i < ends[[k]]) body[(i + 1L):ends[[k]]] else character(0)
      refusing <- length(shown) > 0L &&
        grepl("^(illegal|unsupported):", trimws(shown[[1]]))
      if (!refusing) {
        pipeline <- command_pipeline(command)
        if (!is.null(pipeline) && looks_like_pipeline(pipeline)) {
          checked <- checked + 1L
          refusal <- god_refusal(binary, pipeline)
          if (!is.null(refusal)) {
            bad <- c(bad, sprintf("  %s:%d  %s\n%s", short, at + i - 1L, command, indent(refusal)))
            next
          }
        }
      } else {
        checked <- checked + 1L
      }
      ran <- run_transcript(binary, command, refusing = refusing)
      if (!is.null(ran$complaint)) {
        bad <- c(bad, sprintf("  %s:%d  %s\n%s", short, at + i - 1L, command, indent(ran$complaint)))
        next
      }
      if (!identical(trimws(shown, "right"), trimws(ran$lines, "right"))) {
        bad <- c(bad, sprintf(
          "  %s:%d  the page shows output the command does not print\n%s",
          short, at + i - 1L,
          indent(paste0("page   | ", paste(shown, collapse = " / "), "\n",
                        "engine | ", paste(ran$lines, collapse = " / ")))))
      }
    }
    return(list(bad = bad, checked = checked))
  }

  # A diagnostic the book is quoting. It contains the word `then` inside a
  # gutter, and reading it as a pipeline would refuse the book for correctly
  # showing a refusal.
  if (grepl("^(illegal|unsupported|assumption):", trimws(body[1]))) {
    return(list(bad = bad, checked = 0L))
  }

  text <- paste(body, collapse = "\n")
  if (!looks_like_pipeline(text)) return(list(bad = bad, checked = 0L))

  refusal <- god_refusal(binary, text)
  if (!is.null(refusal)) {
    bad <- c(bad, sprintf("  %s:%d\n%s", short, at, indent(refusal)))
  }
  list(bad = bad, checked = 1L)
}

# A pipeline is a table name followed by `then` followed by something, and that
# whole shape is the test rather than the word `then` alone.
#
# **The word by itself is not enough, and the book proves it.** One chapter
# discusses `then` as a column name, to show that the grammar reserves nothing;
# keying on the word reported that sentence as a broken pipeline. A guard whose
# first run accuses the manual of the thing the manual is explaining is a guard
# that gets deleted.
looks_like_pipeline <- function(text) {
  # A quoted diagnostic carries a line-number gutter, and the pipeline inside it
  # is the one being refused on purpose.
  if (any(grepl("^\\s*[0-9]*\\s*\\|", strsplit(text, "\n")[[1]]))) return(FALSE)
  flat <- trimws(gsub("\\s+", " ", text))
  grepl("^[A-Za-z_][A-Za-z0-9_]* then \\S", flat)
}

# -- talking to the engine ---------------------------------------------------

# The refusal, or NULL if the grammar read it.
god_refusal <- function(binary, pipeline) {
  err <- tempfile(); on.exit(unlink(err), add = TRUE)
  status <- suppressWarnings(system2(
    binary, "--needs",
    stdout = FALSE, stderr = err, input = pipeline
  ))
  if (status == 0L) return(NULL)
  paste(readLines(err, warn = FALSE), collapse = "\n")
}

god_flags <- function(binary) {
  out <- tempfile(); on.exit(unlink(out), add = TRUE)
  suppressWarnings(system2(binary, "--help", stdout = out, stderr = out))
  help <- paste(readLines(out, warn = FALSE), collapse = "\n")
  found <- regmatches(help, gregexpr("--[a-z][a-z-]*", help))[[1]]
  unique(found)
}

# The transcript, actually run. An answering one must exit 0 and its stdout is
# the page's output; a refusing one must exit 2 and the diagnostic on stderr is
# the page's output. Nothing in between is a transcript the book may show.
run_transcript <- function(binary, command, refusing = FALSE) {
  tokens <- shell_tokens(command)[-1L]
  out <- tempfile(); err <- tempfile()
  on.exit(unlink(c(out, err)), add = TRUE)
  status <- suppressWarnings(system2(binary, shQuote(tokens), stdout = out, stderr = err))
  if (refusing) {
    if (status != 2L) {
      return(list(complaint = sprintf(
        "the page shows a refusal, and the command exits %d rather than refusing", status),
        lines = NULL))
    }
    return(list(complaint = NULL, lines = readLines(err, warn = FALSE)))
  }
  if (status != 0L) {
    return(list(complaint = sprintf(
      "the command exits %d rather than answering:\n%s", status,
      paste(readLines(err, warn = FALSE), collapse = "\n")), lines = NULL))
  }
  list(complaint = NULL, lines = readLines(out, warn = FALSE))
}

# -- reading a command line --------------------------------------------------

# The command line takes options and exactly one pipeline. A second bare word is
# a subcommand the reader will type and the tool does not have, which is how
# `god show --as dplyr` reached the page.
check_command <- function(command, flags) {
  tokens <- shell_tokens(command)
  if (!length(tokens)) return(NULL)
  tokens <- tokens[-1L]                     # drop `god`

  bare <- character(0)
  i <- 1L
  while (i <= length(tokens)) {
    token <- tokens[[i]]
    if (startsWith(token, "--")) {
      name <- sub("=.*$", "", token)
      if (!(name %in% flags)) {
        return(sprintf("`%s` is not an option this command has. It has: %s",
                       name, paste(flags, collapse = ", ")))
      }
      # `--needs` and `--help` stand alone; the rest take the token after them.
      if (!grepl("=", token, fixed = TRUE) && !(name %in% c("--needs", "--help"))) i <- i + 1L
    } else {
      bare <- c(bare, token)
    }
    i <- i + 1L
  }

  if (length(bare) > 1L) {
    return(sprintf(
      "this passes %d arguments where the command takes one pipeline. `%s` reads as a second one, and there are no subcommands",
      length(bare), bare[[1]]
    ))
  }
  NULL
}

command_pipeline <- function(command) {
  tokens <- shell_tokens(command)
  if (length(tokens) < 2L) return(NULL)
  tokens <- tokens[-1L]
  bare <- character(0)
  i <- 1L
  while (i <= length(tokens)) {
    token <- tokens[[i]]
    if (startsWith(token, "--")) {
      if (!grepl("=", token, fixed = TRUE) &&
          !(sub("=.*$", "", token) %in% c("--needs", "--help"))) i <- i + 1L
    } else {
      bare <- c(bare, token)
    }
    i <- i + 1L
  }
  if (!length(bare)) NULL else bare[[length(bare)]]
}

# Enough of a shell to split a command line on spaces while keeping quoted runs
# together, and to take one layer of quoting off. A book's transcripts are one
# line each, so this does not need to be a shell.
shell_tokens <- function(command) {
  command <- gsub("\\\\\\s*$", "", command)
  chars <- strsplit(command, "")[[1]]
  tokens <- character(0)
  current <- ""
  quote <- ""
  started <- FALSE
  for (ch in chars) {
    if (nzchar(quote)) {
      if (ch == quote) quote <- "" else current <- paste0(current, ch)
      next
    }
    if (ch %in% c("'", "\"")) { quote <- ch; started <- TRUE; next }
    if (ch == " ") {
      if (nzchar(current) || started) { tokens <- c(tokens, current); current <- ""; started <- FALSE }
      next
    }
    current <- paste0(current, ch)
  }
  if (nzchar(current) || started) tokens <- c(tokens, current)
  tokens
}

# -- reading the page --------------------------------------------------------

# The fenced blocks that are **not** executed. A block with a language, or a
# `{r}` chunk, is either run at render time or belongs to another language, and
# in both cases it is not this guard's business.
fenced_blocks <- function(lines) {
  blocks <- list()
  open <- FALSE
  bare <- FALSE
  start <- 0L
  body <- character(0)
  for (i in seq_along(lines)) {
    if (grepl("^\\s*```", lines[i])) {
      if (!open) {
        open <- TRUE
        bare <- grepl("^\\s*```\\s*$", lines[i])
        start <- i + 1L
        body <- character(0)
      } else {
        if (bare && length(body)) blocks[[length(blocks) + 1L]] <- list(start = start, lines = body)
        open <- FALSE
        bare <- FALSE
      }
      next
    }
    if (open && bare) body <- c(body, lines[i])
  }
  blocks
}

inline_code <- function(line) {
  found <- regmatches(line, gregexpr("`[^`]+`", line))[[1]]
  gsub("^`|`$", "", found)
}

indent <- function(text) {
  paste(sprintf("        %s", strsplit(text, "\n")[[1]]), collapse = "\n")
}

# Where the engine is, by the same walk the packages do.
god_engine <- function() {
  named <- Sys.getenv("GOD_CLI", "")
  if (nzchar(named) && file.exists(named)) return(named)
  directory <- normalizePath(getwd(), mustWork = FALSE)
  repeat {
    candidate <- file.path(directory, "target", "release", "god-cli")
    if (file.exists(candidate)) return(candidate)
    parent <- dirname(directory)
    if (identical(parent, directory)) break
    directory <- parent
  }
  NULL
}

# Run standalone as well as sourced, so the guard is usable before there is a
# test suite to source it from.
if (sys.nframe() == 0L) check_grammar()
