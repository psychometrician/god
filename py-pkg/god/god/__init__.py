"""god — a grammar of data.

One small vocabulary for manipulating tables, spelled the same way in R, in
Python, and on a cluster.

    from god import *

    (sales
      >> keep(col.region == "West")
      >> add(margin = col.revenue - col.cost)
      >> summarize(margin = total(col.margin), orders = row_count(), by = col.product)
      >> sort(descending(col.margin))
      >> take(10))

The same sentence in R is the same sentence, and differs in two things: the pipe
is `|>`, and a column is a bare name rather than `col.name`.

A verb returns a plan rather than a frame, so nothing runs until the answer is
wanted. Printing one runs it; `collect` hands back the frame. What comes back is
a pandas frame; there is no god table type to convert to and no session to open.

Where there is no language to bind into, such as a database cell or a
configuration file, the same grammar is written as text and `run` executes it.
"""

from .columns import Expr, col
from .run import GodError, god_sql, run, show_as, show_steps
from .tables import god_table
from .verbs import (
    add,
    add_combinations,
    add_rows,
    all_but,
    average,
    between,
    characters,
    collect,
    descending,
    drop_duplicates,
    drop_missing,
    fill_missing,
    first,
    first_present,
    following,
    join,
    join_rows,
    join_text,
    keep,
    kind,
    hour,
    largest,
    last,
    lengthen,
    look_up,
    lower,
    matching,
    month,
    name,
    median,
    pick,
    previous,
    rank,
    rename,
    replace_text,
    row_count,
    row_number,
    latest,
    remainder,
    rolling,
    running_total,
    smallest,
    standard_deviation,
    split_text,
    sort,
    summarize,
    take,
    take_last,
    to_date,
    to_number,
    to_text,
    round_below,
    round_above,
    total,
    day,
    trim,
    unique_count,
    upper,
    value,
    weekday,
    when,
    where,
    where_any,
    where_every,
    widen,
    year,
)

__all__ = [
    # The verbs. Every one is an imperative English verb, and the list is
    # closed: a word that is not here is not in the grammar.
    "keep", "pick", "add", "summarize", "sort", "take", "take_last", "join",
    "add_rows", "add_combinations", "drop_duplicates", "rename",
    "drop_missing", "fill_missing",
    # Reshaping. Direction is in the name, because nobody could ever remember
    # which of `melt` and `cast` made data taller.
    "lengthen", "widen",
    # How Python names a column, which is one of the two differences from R.
    "col",
    # Grammar that marks rather than computes.
    "descending", "all_but",
    # A filtering join, which is a condition rather than a verb because it adds
    # no columns.
    "matching",
    # The functions. These are real functions in Python and are names in a
    # syntax tree in R, because Python evaluates an expression and R does not.
    "total", "average", "median", "smallest", "largest", "standard_deviation",
    "first", "last", "unique_count", "row_count",
    # The rank family. Two, where dplyr has six.
    "rank", "row_number",
    # The first value that is there, which SQL calls `coalesce`.
    "first_present",
    # Case, which is what lets a name test ask for either.
    "lower", "upper",
    # Converting, always explicitly. Every one begins `to_`, and nothing else
    # does.
    "to_number", "to_text", "to_date", "round_below", "round_above",
    # The rest of the text functions. The `_text` suffix says what they operate
    # on, where `to_` says what they convert into.
    "trim", "characters", "replace_text", "split_text",
    # Text put together, which is `split_text` read the other way. A
    # separator is written as a value where it goes, not as a setting.
    "join_rows",
    "join_text",
    # Whether a value sits between two ends, counting both.
    "between",
    # The parts of a date. `weekday` counts Monday as 1 wherever it runs, which
    # the engines left to themselves do not agree on.
    "year", "month", "day", "weekday", "hour",
    # Looking along the rows. All of them need a `sort` before them, and
    # `rolling` carries an aggregate over the last few rows.
    "running_total", "previous", "following", "latest", "rolling", "remainder",
    # Choosing columns by the shape of their name.
    "where", "where_any", "where_every", "name", "value", "kind",
    # The conditional. Not Python's own `if`, which would pick a branch
    # while the pipeline is being built and discard the condition.
    "when",
    # The lookup table, which is the conditional specialized to equality on
    # one subject, with its `otherwise` required.
    "look_up",
    # Materializing. Nothing runs until the answer is wanted.
    "collect",
    # The text form, for where there is no host language to bind into.
    "run", "show_as", "show_steps", "god_sql",
    # The book's tables, fetched by name from the published site.
    "god_table",
    "GodError", "Expr",
]
# One grammar, one number. This is the declaration a *user* reads, and the
# release gate compares it against the other three — `pyproject.toml`, the
# workspace `Cargo.toml`, and R's `DESCRIPTION` — because a manifest check alone
# cannot see this one go stale.
__version__ = "0.2.1"
