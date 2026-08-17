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

god_book_data_url <- "https://psychometrician.github.io/god-book/data/"

# `data/<name>.csv`, in this directory or any above it. Deliberately not
# `book/data/`: a package that knows the manual's directory layout is a package
# with a second job. A reader keeping their own copies puts them in `data/` and
# gets the same behaviour the book does.
god_walk_up_data <- function(start, name) {
  directory <- normalizePath(start, mustWork = FALSE)
  repeat {
    candidate <- file.path(directory, "data", paste0(name, ".csv"))
    if (file.exists(candidate)) return(candidate)
    parent <- dirname(directory)
    if (identical(parent, directory)) return(NA_character_)
    directory <- parent
  }
}

#' Read one of the book's example tables
#'
#' Returns a table ready to pipe. A `data/<name>.csv` in the working directory
#' or any directory above it is read first; failing that, the copy published
#' beside the manual is fetched. The cast is declared in the book's preface:
#' `sales`, `products`, `survey`, `answers`, `marks`, `messy`, `diary` and
#' `gapminder`.
#'
#' @param name The table's name without the extension, such as `"sales"`.
#' @param text Columns that must stay text. A CSV records what a value is and
#'   never what kind of thing it is, so a column of `01`, `02`, `03` comes
#'   back as the numbers 1, 2, 3 unless it is named here.
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
         "`_` and `-`, as in `god_table(\"sales\")` — not a path and not a ",
         "file name.", call. = FALSE)
  }
  classes <- rep("character", length(text))
  names(classes) <- text
  local <- god_walk_up_data(getwd(), name)
  from <- if (is.na(local)) paste0(god_book_data_url, name, ".csv") else local
  utils::read.csv(from, colClasses = classes, stringsAsFactors = FALSE)
}
