# Translating R's own expressions into the grammar's text.
#
# **This is the one piece of real logic that lives in a host language**, and it is
# worth saying why it is allowed to when nothing else is. The verbs above it are a
# syntax builder: they arrange words and invent no meaning. This file is different
# — it reads R's syntax tree and decides what each node says in the grammar — and
# a second copy of it will be written for Python.
#
# Two copies of a decision can disagree, which is the thing this project exists to
# prevent, so the translator is deliberately kept to a shape where disagreement is
# **visible rather than latent**: it is a table of one-to-one rewrites with no
# defaults and no cleverness. A node this file does not recognize is refused by
# name. It never guesses, because a guess here is a sentence that means one thing
# in R and another in Python, and neither host would report a problem.
#
# What it does not do is check anything. Whether `[revenue]` exists, whether
# `total` may appear where it was written, whether the types line up — all of that
# is the grammar's, and asking it here would be asking it twice, in two languages,
# with two answers.

# R's spelling on the left, the grammar's on the right (§2.4).
#
# These are the words where the hosts genuinely disagree, so each one is a token
# that had to be replaced rather than passed through.
god_comparisons <- c(
  "==" = "is",
  "!=" = "is not",
  "<" = "<",
  "<=" = "<=",
  ">" = ">",
  ">=" = ">="
)

god_arithmetic <- c("+" = "+", "-" = "-", "*" = "*", "/" = "/")

god_logic <- c("&" = "and", "&&" = "and", "|" = "or", "||" = "or")

# An R expression, as the grammar would write it.
god_expr <- function(e) {
  if (is.symbol(e)) {
    return(god_column(as.character(e)))
  }

  if (is.character(e)) {
    return(god_text(e))
  }

  if (is.numeric(e)) {
    return(god_number(e))
  }

  if (is.logical(e) && length(e) == 1L) {
    # `NA` is logical in R, which is why the absent value is tested before the
    # truth values rather than after.
    if (is.na(e)) {
      return("missing")
    }
    return(if (e) "yes" else "no")
  }

  if (is.call(e)) {
    return(god_call_expr(e))
  }

  god_refuse_node(e)
}

# `value`, inside an `add(where(...))`, is the column being worked on.
#
# **It is marked before translation rather than special-cased inside it**, for
# the same reason `where()` has its own reader: a bare symbol is a column
# everywhere else in R, and a table with a column called `value` has to stay
# reachable. So the symbol is rewritten to a call this file recognizes, and
# `god_expr` goes on treating every other bare symbol as a column.
god_mark_value <- function(e) {
  if (is.symbol(e) && identical(as.character(e), "value")) {
    return(quote(.god_value()))
  }
  if (is.call(e)) {
    for (i in seq_along(e)[-1L]) {
      e[[i]] <- god_mark_value(e[[i]])
    }
  }
  e
}

# A condition inside `pick(where(...))`, which asks about a column's *name*.
#
# **It has its own reader rather than going through `god_expr`**, because there
# the bare symbol `name` is a column called `name`, and here it is the word for
# whichever column is being considered. One symbol cannot mean both in one
# function, and a flag threaded through every expression to say which would be
# the context rule this spelling exists to avoid.
#
# The shapes it takes are the ones the grammar allows about a name, and the list
# is short on purpose: the three text tests, `==` and `!=`, and `&`, `|`, `!` to
# join them.
god_pick_condition <- function(e) {
  shape <- "`where` asks about a column's name or what it holds: pick(where(startsWith(name, \"q\"))), or pick(where(kind == \"number\"))"

  if (!is.call(e)) {
    stop(shape, call. = FALSE)
  }
  op <- as.character(e[[1L]])
  args <- as.list(e)[-1L]

  if (identical(op, "(")) {
    return(god_pick_condition(args[[1L]]))
  }
  if (op %in% names(god_logic) && length(args) == 2L) {
    return(sprintf(
      "(%s %s %s)",
      god_pick_condition(args[[1L]]),
      god_logic[[op]],
      god_pick_condition(args[[2L]])
    ))
  }
  if (identical(op, "!") && length(args) == 1L) {
    return(sprintf("(not %s)", god_pick_condition(args[[1L]])))
  }

  # The subject of every test below is written out: `name` for what a column is
  # called, `kind` for what it holds.
  is_name <- function(x) is.symbol(x) && identical(as.character(x), "name")
  is_kind <- function(x) is.symbol(x) && identical(as.character(x), "kind")

  # `tolower(name)` folds the case of the name being tested, which is how a name
  # test asks for either case. The subject stays written either way.
  folded <- function(x, word) {
    if (!is.call(x) || length(as.list(x)) != 2L) return(NULL)
    fold <- as.character(x[[1L]])
    if (!fold %in% c("tolower", "toupper")) return(NULL)
    inner <- x[[2L]]
    ok <- if (identical(word, "name")) is_name(inner) else is_kind(inner)
    if (!ok) return(NULL)
    sprintf("%s(%s)", if (identical(fold, "tolower")) "lower" else "upper", word)
  }
  subject <- function(x) {
    if (is_name(x)) return("name")
    if (is_kind(x)) return("kind")
    for (w in c("name", "kind")) {
      got <- folded(x, w)
      if (!is.null(got)) return(got)
    }
    NULL
  }

  if (op %in% c("startsWith", "endsWith") && length(args) == 2L &&
      !is.null(subject(args[[1L]]))) {
    word <- if (identical(op, "startsWith")) "starts" else "ends"
    return(sprintf("(%s %s %s)", subject(args[[1L]]), word, god_text(args[[2L]])))
  }
  if (identical(op, "grepl") && length(args) >= 2L && !is.null(subject(args[[2L]]))) {
    return(sprintf("(%s contains %s)", subject(args[[2L]]), god_text(args[[1L]])))
  }
  if (op %in% c("==", "!=") && length(args) == 2L && !is.null(subject(args[[1L]]))) {
    return(sprintf(
      "(%s %s %s)", subject(args[[1L]]), god_comparisons[[op]], god_text(args[[2L]])
    ))
  }

  stop(shape, call. = FALSE)
}

god_call_expr <- function(e) {
  head <- e[[1L]]
  if (!is.symbol(head)) {
    stop(
      "god does not know how to read this expression: only a plain function name can be called here",
      call. = FALSE
    )
  }
  op <- as.character(head)
  args <- as.list(e)[-1L]

  # Parentheses the caller wrote carry no meaning the grammar does not already
  # get from the shape of the tree, and every operator below emits its own, so
  # the group is dropped rather than doubled.
  if (identical(op, "(")) {
    return(god_expr(args[[1L]]))
  }

  # Left by `god_mark_value`, and reachable no other way: `.god_value` is not a
  # name anybody can type, since a leading dot makes it invalid as a bare symbol
  # in the position this would have to appear in.
  if (identical(op, ".god_value")) {
    return("value")
  }

  if (op %in% names(god_comparisons) && length(args) == 2L) {
    return(god_infix(god_comparisons[[op]], args[[1L]], args[[2L]]))
  }

  if (op %in% names(god_arithmetic)) {
    # `-x` and `x - y` are the same symbol in R and are told apart by how many
    # operands it was given.
    if (length(args) == 1L) {
      return(paste0(op, god_expr(args[[1L]])))
    }
    return(god_infix(god_arithmetic[[op]], args[[1L]], args[[2L]]))
  }

  if (op %in% names(god_logic) && length(args) == 2L) {
    return(god_infix(god_logic[[op]], args[[1L]], args[[2L]]))
  }

  if (identical(op, "!") && length(args) == 1L) {
    return(god_negate(args[[1L]]))
  }

  if (identical(op, "%in%") && length(args) == 2L) {
    return(god_membership(args[[1L]], args[[2L]], negated = FALSE))
  }

  if (identical(op, "is.na") && length(args) == 1L) {
    return(sprintf("(%s is missing)", god_expr(args[[1L]])))
  }

  # R's own spellings for the three text tests. `startsWith` and `endsWith` are
  # base R; `grepl(pattern, x, fixed = TRUE)` is how base R asks the third, and
  # its arguments are the other way round, which is why it is not in the table
  # of two-argument translations.
  if (op %in% c("startsWith", "endsWith") && length(args) == 2L) {
    word <- if (identical(op, "startsWith")) "starts" else "ends"
    return(sprintf("(%s %s %s)", god_expr(args[[1L]]), word, god_expr(args[[2L]])))
  }
  # R spells these `tolower` and `toupper`; the grammar spells them without the
  # `to`, since it reserves that prefix for converting between kinds of value.
  if (op %in% c("tolower", "toupper") && length(args) == 1L) {
    word <- if (identical(op, "tolower")) "lower" else "upper"
    return(sprintf("%s(%s)", word, god_expr(args[[1L]])))
  }

  if (identical(op, "grepl") && length(args) >= 2L) {
    return(sprintf("(%s contains %s)", god_expr(args[[2L]]), god_expr(args[[1L]])))
  }

  # `matching` is the one expression whose first argument is a table rather than
  # a value. The general rule below reads every argument as a value, which would
  # turn the table's name into a column reference and fail much further in.
  if (identical(op, "matching")) {
    return(god_matching(args))
  }

  # `when` carries its catch-all as a named argument here and as a word in the
  # text form, because the text form has no `=`. That is the only difference,
  # and it is the same one `by` and `unmatched` already have.
  if (identical(op, "when")) {
    return(god_when(e))
  }

  # `look_up` reads the same way: pairs, and a named `otherwise`. The pairs'
  # shape is the engine's question — a lopsided list gets the parser's own
  # sentence about the value with nothing beside it.
  if (identical(op, "look_up")) {
    return(god_lookup(e))
  }

  # `rank` takes a column in an ordering position, so it may carry `descending`
  # exactly as a `sort` key does. The general rule below reads every argument as
  # a value, which would turn `descending(revenue)` into a function call the
  # grammar does not have.
  if (identical(op, "rank")) {
    return(god_rank(args))
  }

  # Anything else is a function in the grammar's own vocabulary. Whether it is
  # one is the grammar's question, not this file's: guessing here would mean a
  # list of function names maintained in three places that could drift apart.
  sprintf("%s(%s)", op, paste(vapply(args, god_expr, character(1)), collapse = ", "))
}

god_lookup <- function(e) {
  parts <- as.list(e)[-1L]
  named <- names(parts)
  if (is.null(named)) named <- rep("", length(parts))

  extra <- setdiff(named[nzchar(named)], "otherwise")
  if (length(extra)) {
    stop(
      sprintf(
        "`look_up` takes the column, pairs of written values, and `otherwise` for the rest. It does not take `%s`",
        extra[[1L]]
      ),
      call. = FALSE
    )
  }

  fallback <- parts[named == "otherwise"]
  positional <- parts[named != "otherwise"]
  if (!length(positional)) {
    stop(
      "`look_up` needs the value being looked up: look_up(code, \"W\", \"West\", otherwise = code)",
      call. = FALSE
    )
  }

  written <- vapply(positional, god_expr, character(1))
  if (length(fallback)) {
    written <- c(written, sprintf("otherwise %s", god_expr(fallback[[1L]])))
  }
  sprintf("look_up(%s)", paste(written, collapse = ", "))
}

god_infix <- function(word, left, right) {
  sprintf("(%s %s %s)", god_expr(left), word, god_expr(right))
}

# `!` is the one operator where the grammar has a better word than a wrapper.
#
# `!is.na(x)` and `!(x %in% y)` have their own spellings — `is not missing` and
# `not in` — and reaching them here rather than emitting `not (… is missing)`
# matters because it is what a person would have written by hand.
god_negate <- function(inner) {
  # `!(x %in% y)` is how anyone would write it, and the parentheses are a node in
  # R's tree rather than punctuation. Looking through them is what lets the
  # spelling below be reached by the sentence people actually type.
  inner <- god_ungroup(inner)

  if (is.call(inner)) {
    op <- as.character(inner[[1L]])
    args <- as.list(inner)[-1L]

    if (identical(op, "is.na") && length(args) == 1L) {
      return(sprintf("(%s is not missing)", god_expr(args[[1L]])))
    }
    if (identical(op, "%in%") && length(args) == 2L) {
      return(god_membership(args[[1L]], args[[2L]], negated = TRUE))
    }
  }
  sprintf("(not %s)", god_expr(inner))
}

god_membership <- function(left, right, negated) {
  right <- god_ungroup(right)

  set <- if (is.call(right) && identical(as.character(right[[1L]]), "c")) {
    as.list(right)[-1L]
  } else if (is.call(right) || is.symbol(right)) {
    # **A name here is neither a column nor a value the grammar can see.** A set
    # holds values written out, and the grammar has no variables yet, so reading
    # `region %in% wanted` would mean running the caller's code to find out what
    # their pipeline says. Refusing names the one spelling that works; guessing
    # would put a column reference inside a set and let it fail further in.
    stop(
      sprintf(
        "god cannot read `%s` as a set of values. Write them out: c(\"West\", \"East\")",
        paste(deparse(right), collapse = " ")
      ),
      call. = FALSE
    )
  } else {
    # A literal vector, which is what `c("West", "East")` becomes if R evaluated
    # it before this saw it.
    as.list(right)
  }

  sprintf(
    "(%s %sin {%s})",
    god_expr(left),
    if (negated) "not " else "",
    paste(vapply(set, god_expr, character(1)), collapse = ", ")
  )
}

# Look through the parentheses a caller wrote.
#
# They are a node in R's syntax tree, not punctuation, so a rule that matches on
# the shape of an expression has to see past them or it silently stops applying
# the moment someone adds a pair.
god_ungroup <- function(e) {
  while (is.call(e) && identical(as.character(e[[1L]]), "(")) {
    e <- e[[2L]]
  }
  e
}

# Inside brackets it is a column, always (§2.3).
god_column <- function(name) sprintf("[%s]", name)

god_text <- function(value) {
  if (length(value) != 1L) {
    stop("god expected one text value here, not several", call. = FALSE)
  }
  # The grammar closes a text value at the first `"` and has no escape, so a
  # value containing one cannot be written at all. Refusing here names the
  # problem; passing it through would produce a sentence that ends somewhere
  # the caller did not intend.
  if (grepl('"', value, fixed = TRUE)) {
    stop(
      "god cannot yet write a text value containing a double quote, and will not guess where it ends",
      call. = FALSE
    )
  }
  sprintf('"%s"', value)
}

god_number <- function(value) {
  if (length(value) != 1L) {
    stop("god expected one number here, not several", call. = FALSE)
  }
  if (is.na(value)) {
    return("missing")
  }
  # `format` rather than `as.character`, because R writes large and small numbers
  # in scientific notation and the grammar reads digits and at most one point.
  # `1e+05` would not be a number to it.
  written <- format(value, scientific = FALSE, trim = TRUE)
  if (!grepl("^-?[0-9]+(\\.[0-9]+)?$", written)) {
    stop(sprintf("god cannot write `%s` as a number", written), call. = FALSE)
  }
  written
}

god_refuse_node <- function(e) {
  stop(
    sprintf("god does not know how to read `%s` in a pipeline", paste(deparse(e), collapse = " ")),
    call. = FALSE
  )
}

# A bare name, where the grammar wants a column rather than an expression.
#
# `pick`, `by =` and `sort` take names and not values, so a caller writing
# `pick(revenue + cost)` is told what those positions accept rather than handed
# an error from further in.
god_name <- function(e, where) {
  if (is.symbol(e)) {
    return(as.character(e))
  }
  if (is.character(e) && length(e) == 1L) {
    return(e)
  }
  # **The verb was named twice**: `sort` names a column in sort. `where` is
  # already the verb, so saying it once is the whole fix.
  written <- paste(deparse(e), collapse = " ")
  if (nchar(written) > 60) written <- paste0(substr(written, 1, 57), "...")
  stop(
    sprintf("`%s` names a column, and `%s` is not a column name", where, written),
    call. = FALSE
  )
}

# `rank(revenue)` and `rank(descending(revenue))`.
#
# One column, and `descending` marks it the same way it marks a sort key,
# because a column in an ordering position is one idea however it is reached.
god_rank <- function(args) {
  shape <- "`rank` ranks by one column: rank(revenue), or rank(descending(revenue))"
  if (length(args) != 1L) {
    stop(shape, call. = FALSE)
  }

  key <- args[[1L]]
  if (is.call(key) && identical(as.character(key[[1L]]), "descending")) {
    inner <- as.list(key)[-1L]
    if (length(inner) != 1L) {
      stop("`descending` takes one column: rank(descending(revenue))", call. = FALSE)
    }
    return(sprintf("rank([%s] descending)", god_name(inner[[1L]], "rank")))
  }
  sprintf("rank([%s])", god_name(key, "rank"))
}

# `matching(products, by = id)`, which becomes `matching(products, by [id])`.
#
# `by` is spelled the same here as on `join`, because working out which columns
# say that two rows correspond is one idea whichever verb asks it.
god_matching <- function(args) {
  shape <- "`matching` needs the other table by name: matching(products, by = id)"

  named <- names(args)
  if (is.null(named)) named <- rep("", length(args))

  positional <- args[named == ""]
  if (length(positional) != 1L || !is.symbol(positional[[1L]])) {
    stop(shape, call. = FALSE)
  }

  extra <- setdiff(named[named != ""], "by")
  if (length(extra)) {
    stop(
      sprintf(
        "`matching` takes the table and `by`, and does not take `%s`",
        extra[[1L]]
      ),
      call. = FALSE
    )
  }

  other <- as.character(positional[[1L]])
  by <- args[named == "by"]
  if (!length(by)) {
    return(sprintf("matching(%s)", other))
  }
  sprintf("matching(%s, by %s)", other, god_join_keys(by[[1L]], "by"))
}

# The columns that say which rows of two tables correspond, as the text form
# writes them.
#
# **R cannot spell the grammar's `is`**, which has no infix form here, so the
# binding writes `==` and this turns it back. That is the same trade §2.4
# records everywhere else: the vocabulary is identical and only the idiom moves,
# and `==` is already what the translator turns into `is` inside a condition.
#
#   by = id                        one key, the same word on both sides
#   by = c(region, product)        several, all the same word
#   by = customer_id == id         one key, named differently on each side
#   by = c(region, customer_id == id)     and the two mixed
#
# The pair is emitted as its own bracket group; a run of shared names collapses
# into one, which is what the caller wrote and what the engine hands back.
god_join_keys <- function(e, where) {
  parts <- if (is.call(e) && identical(as.character(e[[1L]]), "c")) {
    as.list(e)[-1L]
  } else {
    list(e)
  }

  written <- character()
  shared <- character()
  flush <- function() {
    if (length(shared)) {
      written <<- c(written, sprintf("[%s]", paste(shared, collapse = ", ")))
      shared <<- character()
    }
  }

  for (part in parts) {
    if (is.call(part) && identical(as.character(part[[1L]]), "==")) {
      flush()
      written <- c(written, sprintf(
        "[%s] is [%s]",
        god_name(part[[2L]], where),
        god_name(part[[3L]], where)
      ))
    } else {
      shared <- c(shared, god_name(part, where))
    }
  }
  flush()
  paste(written, collapse = ", ")
}

# The columns in a `by =`, which is one name or several written as a list.
god_names <- function(e, where) {
  if (is.call(e) && identical(as.character(e[[1L]]), "c")) {
    return(vapply(as.list(e)[-1L], god_name, character(1), where = where))
  }
  god_name(e, where)
}

# `when(score >= 90, "A", score >= 70, "B", otherwise = "C")`.
#
# The pairs are positional and the catch-all is named, which is how both hosts
# write it. The text form spells the catch-all as a word instead, because it has
# no `=`, and that is the only thing that moves.
god_when <- function(e) {
  parts <- as.list(e)[-1L]
  named <- names(parts)
  if (is.null(named)) named <- rep("", length(parts))

  extra <- setdiff(named[nzchar(named)], "otherwise")
  if (length(extra)) {
    stop(
      sprintf(
        "`when` takes its questions and answers in pairs, and `otherwise` for the rest. It does not take `%s`",
        extra[[1L]]
      ),
      call. = FALSE
    )
  }

  fallback <- parts[named == "otherwise"]
  pairs <- parts[named != "otherwise"]

  if (!length(pairs)) {
    stop(
      "`when` needs at least one question and the answer that goes with it: when(score >= 90, \"A\", otherwise = \"C\")",
      call. = FALSE
    )
  }
  if (length(pairs) %% 2L != 0L) {
    stop(
      "each question `when` asks needs the answer that goes with it, right after it: when(score >= 90, \"A\", otherwise = \"C\")",
      call. = FALSE
    )
  }

  written <- vapply(pairs, god_expr, character(1))
  if (length(fallback)) {
    written <- c(written, sprintf("otherwise %s", god_expr(fallback[[1L]])))
  }
  sprintf("when(%s)", paste(written, collapse = ", "))
}
