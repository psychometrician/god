# How a pipeline prints inside a rendered document.
#
# A pipeline printed at the console is console output, and that is right: it is
# what the rest of an R session looks like. A pipeline printed inside Quarto or R
# Markdown is a table on a page, and console text there is a picture of a
# terminal rather than a table.

# No `@export` tag here, and none may return: roxygen would turn it into an
# `S3method(knit_print, god_pipeline)` line in NAMESPACE, which makes knitr a
# load-time dependency. `.onLoad` below registers the method only where knitr
# actually exists.
knit_print.god_pipeline <- function(x, ...) {
  # **Delegating rather than choosing a format is the whole point.** The
  # document already has an answer for how a table is printed, in `df-print`, and
  # a pipeline that picked its own would be the one table on the page that
  # ignored the setting.
  knitr::knit_print(collect(x), ...)
}

# A drawing inside a rendered document is the picture, not the ladder.
#
# **This is the one place the medium is chosen for the reader**, and it is chosen
# by asking where they are rather than by an argument: a console gets text
# because that is what the rest of a session looks like, and a page gets the
# drawing because a page can hold one. Nobody has to say which.
knit_print.god_steps <- function(x, ...) {
  # `asis_output` rather than a fenced block: the SVG is markup and has to reach
  # the page as markup. It carries its own stylesheet, including the dark half,
  # so nothing about the document's theme reaches inside it.
  knitr::asis_output(format(x, "svg"))
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
    registerS3method(
      "knit_print", "god_steps", knit_print.god_steps,
      envir = asNamespace("knitr")
    )
  }
}
