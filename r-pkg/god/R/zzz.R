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

# The other half of masking, which god owed and had not paid.
#
# god shadows other people's names on purpose, and `verbs.R` writes the message
# that costs: call god's `sort` on something that is not a table and it names
# `base::sort` and how to reach it. That covers one direction only — god masking
# somebody else. **The other direction is somebody else masking god, and it is
# the common one**, because dplyr is the package most likely to sit beside this
# one and `library(dplyr)` is usually typed second.
#
# Four names collide: `collect`, `pick`, `rename` and `summarize`. Attached last,
# dplyr owns all four, and a god pipeline handed to one reached a generic with no
# method for it:
#
#     no applicable method for 'collect' applied to an object of class "god_pipeline"
#     no applicable method for 'summarise' applied to an object of class "god_pipeline"
#
# The second answers about a spelling nobody typed. The user wrote `summarize`;
# `summarise` is the generic both spellings dispatch through, so that is the name
# R reports and the one a method has to be registered under. Registering the
# American spelling registers a method nothing will ever dispatch to.
#
# **`pick` is the fourth and no method can reach it**, because dplyr's `pick` is
# not a generic at all: it reads the data mask of the verb surrounding it and
# stops with dplyr's own error whatever it is handed. `god::pick` is the repair,
# and it is the only one.

# Each of these is the god verb, reached through dplyr's generic. The bar is not
# "does something reasonable" — it is **exactly what the unmasked call does**,
# because the fix is finished when the attach order stops being observable.
god_dplyr_collect <- function(x, ...) collect(x)

god_dplyr_rename <- function(.data, ...) rename(.data, ...)

god_dplyr_summarise <- function(.data, ..., by) {
  if (missing(by)) {
    return(summarize(.data, ...))
  }
  # **`by` cannot be forwarded, because the grammar reads it unevaluated.**
  # `summarize(.data, ..., by = by)` would hand it the symbol `by` rather than
  # the column the caller named. So the call is rebuilt from the expressions the
  # caller wrote, with god's own `summarize` as its head — the name would resolve
  # back to dplyr's out here — and with the pipeline spliced in as the object
  # dplyr's generic already evaluated in order to dispatch on it, so nothing the
  # caller wrote is evaluated a second time.
  call <- as.call(c(
    list(summarize, .data),
    as.list(substitute(list(...)))[-1L],
    list(by = substitute(by))
  ))
  eval(call, parent.frame())
}

god_register_with_dplyr <- function(...) {
  if (!isNamespaceLoaded("dplyr")) {
    return(invisible(NULL))
  }
  dplyr_ns <- asNamespace("dplyr")
  methods <- list(
    collect   = god_dplyr_collect,
    rename    = god_dplyr_rename,
    summarise = god_dplyr_summarise
  )
  for (generic in names(methods)) {
    # A dplyr old enough to be missing one of these is not worth erroring over
    # during a package load, and a load that fails takes god with it.
    if (exists(generic, envir = dplyr_ns, inherits = FALSE)) {
      registerS3method(generic, "god_pipeline", methods[[generic]], envir = dplyr_ns)
    }
  }
  invisible(NULL)
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

  # **Deliberately not `requireNamespace`, which is what the lines above use.**
  # Asking for knitr is fair: a document that prints a pipeline has loaded knitr
  # already. Asking for dplyr would *load* dplyr — for every user who happens to
  # have it installed, in every session that never mentions it, at the cost of
  # loading its dependency tree. So: register now if dplyr is already here, and
  # otherwise leave a hook that fires if it ever arrives. Either order works, and
  # neither pulls dplyr in.
  god_register_with_dplyr()
  setHook(packageEvent("dplyr", "onLoad"), god_register_with_dplyr)
}
