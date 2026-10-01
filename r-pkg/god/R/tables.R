# tables.R — the book's example tables, fetched by name.
#
# Not a word of the grammar, and deliberately so: this is the same category as
# `run` and `god_sql`, something the binding offers and the vocabulary does
# not. It exists because every example in the manual begins with a table, and
# a reader who wants to run one should not have to retype fifteen rows first.
#
# The tables are not shipped in the package. They are published beside the
# book, so one copy serves both languages and nothing goes stale inside a
# tarball — the sibling project keeps its own tables the same way, under its
# own helper's name, and the two names differ on purpose: both packages are
# meant to be loaded together, and one of them masking the other's tables
# would be a collision the pair exists to avoid.
#
# **A local `data/` wins, and that is what makes this usable by the manual that
# needs it.** Reading only from the network meant a new table did not resolve
# until the book was published, and a render calling this thirty times needed a
# connection to build a page about a grammar that has nothing to do with one.
# So the walk-up comes first and the published copy is the fallback, which is
# the same order and the same reason as the engine's own resolution.
#
# **And the file is read by DuckDB, with options the engine pins**, in both
# languages. R's `read.csv` and pandas' `read_csv` made one file into two
# different tables, in a dozen small ways the engine's `read_csv_query` lists,
# so the reader is now the one both bindings already run every query on, and
# the query that reads the file comes from the engine like every other query.

god_book_data_url <- "https://psychometrician.github.io/god-book/data/"

# `data/<name>.csv`, in this directory or any above it. Deliberately not
# `book/data/`: a package that knows the manual's directory layout is a package
# with a second job. A reader keeping their own copies puts them in `data/` and
# gets the same behaviour the book does.
#
# **The name as written, letter for letter.** `file.exists` follows the
# filesystem, which ignores case on a Mac and on Windows and not on Linux, so
# with only `data/Sales.csv` beside it, `god_table("sales")` read that file on
# one machine and fetched the book's `sales` on another. The listing is
# compared exactly, so the answer is the same everywhere.
god_walk_up_data <- function(start, name) {
  file <- paste0(name, ".csv")
  directory <- normalizePath(start, mustWork = FALSE)
  repeat {
    folder <- file.path(directory, "data")
    if (file %in% list.files(folder)) return(file.path(folder, file))
    parent <- dirname(directory)
    if (identical(parent, directory)) return(NA_character_)
    directory <- parent
  }
}

# **A table that could not be found or read has a class of its own.** It is
# still an error a caller catches like any other, and the class is what lets the
# book's refusal guard tell "this page never got its table" from "the grammar
# said no", which before this were the same red text.
god_table_stop <- function(message) {
  stop(structure(class = c("god_table_error", "error", "condition"),
                 list(message = message, call = NULL)))
}

# The published copy, fetched to a temporary file. R gives up after
# `getOption("timeout")` seconds, 60 unless changed, and the Python half waits
# the same. A failure is god's message rather than the download's, the same
# words in both languages, and says which of the two things went wrong.
god_fetch_table <- function(name) {
  file <- tempfile(fileext = ".csv")
  said <- character()
  fetched <- tryCatch(
    withCallingHandlers(
      utils::download.file(paste0(god_book_data_url, name, ".csv"), file,
                           quiet = TRUE, mode = "wb"),
      warning = function(w) {
        said <<- c(said, conditionMessage(w))
        invokeRestart("muffleWarning")
      }
    ),
    error = function(e) NULL
  )
  if (is.null(fetched) || !file.exists(file)) {
    unlink(file)
    god_table_stop(god_table_absent(name, any(grepl("404", said, fixed = TRUE))))
  }
  file
}

god_table_absent <- function(name, unpublished) {
  if (unpublished) {
    paste0("god: there is no table called `", name, "`. No `data/", name,
           ".csv` was found here or in a folder above, and the book publishes ",
           "no table by that name. Check the name, or put your own `", name,
           ".csv` in a `data/` folder.")
  } else {
    paste0("god: no `data/", name, ".csv` was found here or in a folder above, ",
           "and the copy published beside the book could not be fetched. Put `",
           name, ".csv` in a `data/` folder to read it without a connection.")
  }
}

# The file, read on the binding's own DuckDB with the query the engine writes.
god_read_csv <- function(name, path, text) {
  con <- god_connection()
  query <- function(keep) {
    god_call(c("--read-csv", path, unlist(lapply(keep, function(k) c("--text", k)))), "")
  }
  frame <- function(sql) {
    tryCatch(DBI::dbGetQuery(con, sql), error = function(e) {
      god_table_stop(paste0(
        "god: `", name, "` could not be read as a table. Every row of a CSV ",
        "needs as many fields as its header, and a value holding a comma ",
        "needs quotes around it. DuckDB said: ",
        trimws(strsplit(conditionMessage(e), "\n")[[1L]][1L])))
    })
  }
  # **A `text` column the file does not have is refused**, in the grammar's
  # words. A misspelt name would otherwise mean the column the caller wanted
  # kept as text was not, and the two readers this replaced disagreed even
  # about that: R warned, and pandas said nothing.
  if (length(text)) {
    have <- names(frame(sprintf("SELECT * FROM (%s) LIMIT 0", query(character()))))
    absent <- setdiff(text, have)
    if (length(absent)) {
      stop("god: `", name, "` has no column called `", absent[[1L]], "` to keep ",
           "as text. The table has: ", paste(have, collapse = ", "), call. = FALSE)
    }
  }
  frame(query(text))
}

#' Read one of the book's example tables
#'
#' Returns a table ready to pipe. A `data/<name>.csv` in the working directory
#' or any directory above it is read first; failing that, the copy published
#' beside the manual is fetched. So it needs no connection once you have a
#' `data/` folder, and **your own tables work the same way**: put
#' `invoices.csv` in `data/` and `god_table("invoices")` reads it.
#'
#' Every table the book uses is published beside it, the cast the preface
#' declares and the smaller ones a single chapter needs. A name is letters,
#' digits, `_` and `-`; it is not a path, and a `/` or a `..` is refused rather
#' than followed. The file's name has to match the name given, letter for
#' letter, on every operating system.
#'
#' The file is read the same way in R and in Python, so one file is one table
#' in either: a column name is kept exactly as the header writes it, an empty
#' field and `NA` are missing, a date stays the text it was written as until
#' `to_date` reads it, and a row with the wrong number of fields is refused
#' rather than guessed at.
#'
#' @param name The table's name without the extension, such as `"sales"`.
#' @param text Columns that must stay text. A CSV records what a value is and
#'   never what kind of thing it is, so a column of `01`, `02`, `03` can come
#'   back as the numbers 1, 2, 3 unless it is named here. A name the file does
#'   not have is refused.
#' @return A data frame.
#' @examples
#' \dontrun{
#' sales <- god_table("sales")
#' sales |> keep(region == "West") |> take(3)
#' }
#' @export
god_table <- function(name, text = character()) {
  if (!is.character(name) || length(name) != 1L || is.na(name)) {
    stop("god: `god_table()` takes one table name, as in ",
         "`god_table(\"sales\")`. The cast is declared in the book's ",
         "preface.", call. = FALSE)
  }
  # A name is a name, not a path. This mattered less when the only thing a name
  # could do was make a bad URL; now that it also names a file on disk, a `/`
  # or a `..` would reach outside `data/` entirely.
  if (!grepl("^[A-Za-z0-9_-]+$", name)) {
    stop("god: `", name, "` is not a table name. A name is letters, digits, ",
         "`_` and `-`, as in `god_table(\"sales\")`. It is not a path and not ",
         "a file name.", call. = FALSE)
  }
  path <- god_walk_up_data(getwd(), name)
  if (is.na(path)) {
    path <- god_fetch_table(name)
    on.exit(unlink(path), add = TRUE)
  }
  god_read_csv(name, path, text)
}
