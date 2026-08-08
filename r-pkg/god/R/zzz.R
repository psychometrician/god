# How a pipeline prints inside a rendered document.
#
# A pipeline printed at the console is console output, and that is right: it is
# what the rest of an R session looks like. A pipeline printed inside Quarto or R
# Markdown is a table on a page, and console text there is a picture of a
# terminal rather than a table.

#' @export
knit_print.god_pipeline <- function(x, ...) {
  # **Delegating rather than choosing a format is the whole point.** The
  # document already has an answer for how a table is printed, in `df-print`, and
  # a pipeline that picked its own would be the one table on the page that
  # ignored the setting.
  knitr::knit_print(collect(x), ...)
}

.onLoad <- function(libname, pkgname) {
  # Registered here rather than declared in `NAMESPACE`, because knitr is a
  # suggested package and not a required one: somebody who has never installed it
  # still has to be able to load god. `NAMESPACE` cannot express that, and an
  # `S3method(knit_print, god_pipeline)` line there would make knitr a hard
  # dependency of a package that does not otherwise need it.
  if (requireNamespace("knitr", quietly = TRUE)) {
    registerS3method(
      "knit_print", "god_pipeline", knit_print.god_pipeline,
      envir = asNamespace("knitr")
    )
  }
}
