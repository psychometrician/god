"""The verbs, as Python.

**Every function here is a syntax builder**, exactly as its opposite number in R
is. It arranges words into the grammar's own text and hands that over; it decides
nothing about what the words mean. Whether a column exists, whether an
aggregation may appear where it was written, what an empty group comes out as:
none of that is answerable here, and answering it here would answer it twice,
once per language, with two answers that drift.

This module was written by reading the R one and copying its decisions rather
than by making them again. Six of those decisions are recorded in the design
notes precisely so that the second binding would not re-open them, and the places
where Python genuinely had to differ are marked below with the reason.

**A verb returns a pipeline, not a frame.** Nothing runs until the answer is
wanted, which is what lets the grammar see a whole sentence before any of it
executes and report a bad column at step two rather than failing at step seven.
"""

from __future__ import annotations

import inspect

from .columns import Expr, GodExpressionError, name_of, _value

__all__ = [
    "keep", "pick", "add", "summarize", "sort", "take", "join",
    "add_rows", "drop_duplicates", "rename", "drop_missing", "fill_missing",
    "descending", "all_but", "collect",
    "total", "average", "median", "smallest", "largest",
    "first", "last", "unique_count", "row_count",
    "rank", "row_number", "first_present", "lower", "upper",
    "where", "name", "value", "kind",
]


class Pipeline:
    """A plan for a table, not a table.

    Printing one runs it, which is what makes a session feel ordinary. Ask for
    the frame itself with ``collect``.
    """

    __slots__ = ("source", "tables", "steps")

    def __init__(self, source: str, tables: dict, steps: tuple[str, ...] = ()):
        self.source = source
        # Every table the sentence reads, not one: `join` names a second, and
        # the grammar has to be told about all of them.
        self.tables = tables
        self.steps = steps

    @property
    def table(self):
        """The table at the head, which is the only one most pipelines have."""
        return self.tables[self.source]

    def __rshift__(self, verb):
        if not isinstance(verb, _Verb):
            raise TypeError(
                "the right side of `>>` has to be a god verb, "
                f"and this is {type(verb).__name__}"
            )
        return verb._apply(self)

    def _then(self, step: str) -> "Pipeline":
        return Pipeline(self.source, dict(self.tables), self.steps + (step,))

    def written(self) -> str:
        """The pipeline as the grammar's own text, which is what gets handed over."""
        return "\n".join([self.source, *(f"  then {s}" for s in self.steps)])

    # Materialize on first use, which is printing or converting. A person at a
    # prompt should never have to think about this.
    def __repr__(self) -> str:
        return repr(collect(self))

    def __str__(self) -> str:
        return str(collect(self))

    def _repr_html_(self) -> str:
        """The table as HTML, for a notebook or a rendered document.

        Jupyter and Quarto both look for this before falling back to ``repr``,
        so a pipeline shown in either renders as a table rather than as a
        picture of a terminal. At an ordinary prompt nothing looks for it and
        ``repr`` still answers, which is right: console output is what the rest
        of a session looks like.

        The row numbers are left off, because they are pandas' bookkeeping
        rather than anything the table says, and R's side of the same example
        does not show them either.
        """
        return collect(self).to_html(index=False, border=0, classes="table")


class _Verb:
    """A verb waiting for its table."""

    __slots__ = ("_write", "_name", "_brings")

    def __init__(self, name: str, write, brings=None):
        self._name = name
        self._write = write
        # `join` is the one verb that arrives holding a table of its own.
        self._brings = brings

    def _apply(self, pipeline: Pipeline) -> Pipeline:
        out = pipeline._then(self._write())
        if self._brings is not None:
            name, frame = self._brings
            existing = out.tables.get(name)
            if existing is not None and existing is not frame:
                raise GodExpressionError(
                    f"this pipeline already reads a different table called `{name}`"
                )
            out.tables[name] = frame
        return out

    def __rrshift__(self, other):
        """`frame >> verb`, which is how a pipeline opens.

        Reached because no frame library defines ``__rshift__``: pandas, polars
        and pyspark were all checked, and Python falls through to the right
        operand's ``__rrshift__``. That is why the connector is `>>` and not `|`,
        which pandas and Spark both define and would silently answer with an
        elementwise or.
        """
        return self._apply(_open(other, self._name))

    def __repr__(self) -> str:
        return f"god verb: {self._name}"


def _open(table, verb: str) -> Pipeline:
    """Start a pipeline from a frame, and work out what the frame is called.

    **This is the one thing Python had to solve differently from R.** R's `|>`
    hands the verbs an unevaluated call, so the table's name is sitting in it.
    `>>` hands over the frame itself and no name at all, and the grammar's
    sentence names its table.

    So the caller's scope is read backwards: which name is bound to this exact
    object? That recovers `sales` from `sales >> keep(...)` and makes Python emit
    the same sentence R does, which is what the parity check compares. A frame
    with no name of its own falls back to `table`, which is the same fallback R
    uses when the head of the pipe is not a plain name.
    """
    if isinstance(table, Pipeline):
        return table

    if not _is_frame(table):
        raise GodExpressionError(
            f"`{verb}` works on a table, and this is {type(table).__name__}"
        )

    name = _name_in_caller(table)
    return Pipeline(name, {name: table})


def _is_frame(table) -> bool:
    # Asked of the object rather than of pandas, so a polars frame or anything
    # else shaped like a table is not turned away for having the wrong parentage.
    #
    # **A Spark frame has no length and cannot be asked for one**, because
    # counting its rows is a job rather than an attribute. So the question is
    # whether it can say what its columns are, which every table can, and then
    # whether it can be measured *or* describe its own types.
    if not hasattr(table, "columns"):
        return False
    return hasattr(table, "__len__") or hasattr(table, "dtypes")


def _name_in_caller(table) -> str:
    frame = inspect.currentframe()
    try:
        # Walk out of this module before looking, rather than counting frames.
        # A count is wrong the moment a helper moves, and it was: the first
        # version stopped inside `__rrshift__` and named every table `other`,
        # after the parameter holding it.
        caller = frame.f_back
        while caller is not None and caller.f_code.co_filename == __file__:
            caller = caller.f_back
        if caller is None:
            return "table"

        for scope in (caller.f_locals, caller.f_globals):
            for name, value in scope.items():
                if value is table and not name.startswith("_"):
                    return name
        return "table"
    finally:
        del frame


# -- the six verbs -----------------------------------------------------------


def keep(condition):
    """Keep the rows where a condition holds.

    `col.region == "West"`. Comparisons joined by `&` need parentheses, because
    `&` binds more tightly than `==` in Python, and negation is `~`.
    """
    if not isinstance(condition, Expr):
        raise GodExpressionError(
            "`keep` takes a question about a row, like col.region == \"West\""
        )
    # A condition can name a table, which no other expression does:
    # `keep(matching(products, by="id"))` reads `products` without any verb
    # mentioning it, so the table travels with the verb the same way `join`'s
    # does.
    return _Verb("keep", lambda: f"keep where {condition}", brings=condition._brings)


def pick(*names):
    """Just these columns, or all but these.

    Wrap them in ``all_but`` to name the ones to drop instead. It is the same
    word in R and in the text form, which it was not until 2026-08-07: the marker
    was `except`, and Python had to spell it `except_` because `except` is a
    keyword here. `all_but` reads as what it means and is legal in both.
    """
    if not names:
        raise GodExpressionError("`pick` needs at least one column")

    if len(names) == 1 and isinstance(names[0], _Where):
        condition = names[0].condition
        return _Verb("pick", lambda: f"pick where {condition}")
    if any(isinstance(n, _Where) for n in names):
        raise GodExpressionError(
            "`where` chooses the columns on its own, so nothing goes beside it: "
            'pick(where(name.starts("q")))'
        )

    inverted = len(names) == 1 and isinstance(names[0], _AllBut)
    if inverted:
        chosen = names[0].names
    else:
        if any(isinstance(n, _AllBut) for n in names):
            raise GodExpressionError(
                "write the columns inside `all_but()`, all of them: "
                "pick(all_but(col.cost, col.region))"
            )
        chosen = [name_of(n, "pick") for n in names]

    written = ", ".join(chosen)
    return _Verb("pick", lambda: f"pick {'all_but ' if inverted else ''}[{written}]")


def add(*across, by=None, **values):
    """Add or replace a column.

    A column made in one step is not there yet for the rest of that same step;
    every value is worked out from the table as it arrives. Use two steps.

    One value can be applied to every column whose name matches, which is
    dplyr's `across`::

        survey >> add(where(name.starts("q"), value * 2))
    """
    rule = _one_across(across, "add")
    grouping = _grouping(by)
    if rule is not None:
        return _Verb(
            "add", lambda: f"add where {rule.condition} as {rule.value}{grouping}"
        )
    written = _assignments(values, "add")
    return _Verb("add", lambda: f"add {written}{grouping}")


def summarize(*across, by=None, **values):
    """Collapse to one row per group.

    `by` names the columns that say which rows go together. Without it the whole
    table is one group.

    One aggregation can be applied to every column whose name matches::

        survey >> summarize(where(name.ends("_score"), average(value)), by = col.region)
    """
    rule = _one_across(across, "summarize")
    grouping = _grouping(by)
    if rule is not None:
        return _Verb(
            "summarize",
            lambda: f"summarize where {rule.condition} as {rule.value}{grouping}",
        )
    written = _assignments(values, "summarize")
    return _Verb("summarize", lambda: f"summarize {written}{grouping}")


def _one_across(across, verb: str):
    """The `where(...)` rule a verb was given, if it was given one."""
    if not across:
        return None
    if len(across) > 1 or not isinstance(across[0], _Where):
        raise GodExpressionError(
            f"`{verb}` takes named columns, or one `where(...)` naming a pattern "
            "and what to make of each column that matches"
        )
    rule = across[0]
    if rule.value is None:
        raise GodExpressionError(
            f"`{verb} where ...` has to say what to make of each column: "
            'where(name.starts("q"), value * 2)'
        )
    return rule


def sort(*keys):
    """Order the rows.

    Wrap a key in ``descending`` to reverse it. There is deliberately no
    ``ascending``: ascending is what happens when you do not ask for anything.
    """
    if not keys:
        raise GodExpressionError("`sort` needs at least one column to order by")

    written = []
    for key in keys:
        if isinstance(key, _Descending):
            written.append(f"[{key.name}] descending")
        else:
            written.append(f"[{name_of(key, 'sort')}]")
    line = ", ".join(written)
    return _Verb("sort", lambda: f"sort {line}")


def take(n, *, by=None):
    """The first n rows, or the first n of each group.

    `by` needs a `sort` before it, because "the first rows" means nothing until
    something says first by what.

    An ordinary Python value, so a threshold held in a variable works. This is
    the one position where no column could appear, which is why a bare name here
    is a value rather than a column.
    """
    if isinstance(n, Expr) or isinstance(n, bool) or not isinstance(n, int):
        raise GodExpressionError("`take` needs a whole number of rows: take(10)")
    grouping = _grouping(by)
    return _Verb("take", lambda: f"take {n}{grouping}")


def join(other, *, by=None, unmatched="this"):
    """Add another table's columns.

    `by` names the columns that say which rows correspond. Left out, the columns
    both tables share are used and god says which it chose.

    `unmatched` says whose unmatched rows survive: ``"this"`` keeps this table's
    and is the default, ``"none"`` keeps neither, ``"both"`` keeps both. There is
    no ``"other"``, because that is this join with the tables the other way
    round.
    """
    if not _is_frame(other):
        raise GodExpressionError(
            f"`join` needs another table, and this is {type(other).__name__}"
        )
    name = _name_in_caller(other)
    keys = [] if by is None else [name_of(c, "by") for c in (by if isinstance(by, (list, tuple)) else [by])]

    def write():
        matched = f" by [{', '.join(keys)}]" if keys else ""
        survivors = "" if unmatched == "this" else f' unmatched "{unmatched}"'
        return f"join {name}{matched}{survivors}"

    return _Verb("join", write, brings=(name, other))


def matching(other, *, by=None):
    """Whether a row has a partner in another table.

    A condition rather than a verb, because a semi join and an anti join add no
    columns — they only decide which rows survive::

        sales >> keep(matching(products, by="id"))       # semi join
        sales >> keep(~matching(products, by="id"))      # anti join

    `by` names the columns that say which rows correspond. Left out, the columns
    both tables share are used and god says which it chose.

    It cannot multiply rows, which is the one thing `join` cannot promise: a row
    either has a partner or it does not, however many it has.

    It is the whole question `keep` asks rather than one part of one, so it does
    not combine with `&` or `|`. Ask it in its own step.
    """
    if not _is_frame(other):
        raise GodExpressionError(
            f"`matching` needs another table, and this is {type(other).__name__}"
        )
    name = _name_in_caller(other)
    keys = (
        []
        if by is None
        else [name_of(c, "by") for c in (by if isinstance(by, (list, tuple)) else [by])]
    )
    written = f", by [{', '.join(keys)}]" if keys else ""
    return Expr(f"matching({name}{written})", brings=(name, other))


def add_rows(other):
    """Add another table's rows.

    Both tables need the same columns. A column on one side only is refused
    rather than filled in with missing values, because a column that is half
    empty and says nothing is how a mistake survives.
    """
    if not _is_frame(other):
        raise GodExpressionError(
            f"`add_rows` needs another table, and this is {type(other).__name__}"
        )
    name = _name_in_caller(other)
    return _Verb("add_rows", lambda: f"add_rows {name}", brings=(name, other))


def drop_duplicates():
    """Drop rows that are identical across every column.

    The answer comes back in a settled order, because dropping duplicates says
    nothing about order and an answer that reorders itself is not predictable.
    """
    return _Verb("drop_duplicates", lambda: "drop_duplicates")


def rename(**pairs):
    """Rename a column: ``rename(margin = col.profit)``.

    The new name goes first, the way it does everywhere else in the grammar and
    the way assignment reads. Note that pandas writes the pair the other way
    round, and both spellings are legal here, so this is worth reading twice.
    """
    if not pairs:
        raise GodExpressionError(
            "`rename` takes `new = old` pairs: rename(margin = col.profit)"
        )
    written = ", ".join(
        f"[{new}] as [{name_of(old, 'rename')}]" for new, old in pairs.items()
    )
    return _Verb("rename", lambda: f"rename {written}")


def drop_missing(*names):
    """Drop rows with missing values. With no columns named, every column."""
    if not names:
        return _Verb("drop_missing", lambda: "drop_missing")
    listed = ", ".join(name_of(n, "drop_missing") for n in names)
    return _Verb("drop_missing", lambda: f"drop_missing [{listed}]")


def fill_missing(**values):
    """Replace missing values: ``fill_missing(price = 0)``."""
    written = _assignments(values, "fill_missing")
    return _Verb("fill_missing", lambda: f"fill_missing {written}")


# -- the markers -------------------------------------------------------------


class _Descending:
    __slots__ = ("name",)

    def __init__(self, name: str):
        self.name = name


def descending(column):
    """Reverse one sort key: `sort(descending(col.revenue), col.cost)`.

    It marks a column rather than taking a word for the whole sort, because
    direction belongs to a key: `sort(col.product, descending(col.revenue))`
    orders one way by product and the other way by revenue, and a positional word
    could not say which key it modified.
    """
    return _Descending(name_of(column, "sort"))


#: The column being worked on, for `add(where(..., value * 2))`.
#:
#: `name` and `value` are the pair the reshaping verbs already use for what a
#: column is called and what it holds, so neither is new to the vocabulary.
value = Expr("value")


#: What the column being considered holds, for `pick(where(kind == "number"))`.
#:
#: One of ``"text"``, ``"number"``, ``"truth"`` or ``"date"``, written as text
#: the way `unmatched` is. This is dplyr's `where(is.numeric)` and pandas'
#: `select_dtypes`, and it joins with a name test: ``(kind == "number") &
#: name.starts("q")``.
kind = Expr("kind")


#: The name of the column being considered, for `pick(where(...))`.
#:
#: The one place a column's own name is a value. Writing it means `starts` never
#: quietly changes what it is testing: `col.region.starts("W")` asks about what
#: is in the column and `name.starts("q")` asks about what the column is called.
name = Expr("name")


class _Where:
    __slots__ = ("condition", "value")

    def __init__(self, condition, value=None):
        self.condition = condition
        self.value = value


def where(condition, value=None):
    """Columns chosen by the shape of their name.

    In `pick` it takes the question alone::

        survey >> pick(where(name.starts("q")))

    In `add` and `summarize` it also takes what to make of each one, with
    ``value`` standing for the column being worked on::

        survey >> add(where(name.starts("q"), value * 2))
        survey >> summarize(where(name.ends("_score"), average(value))))

    The matched columns keep their names, because `add` already means make or
    replace. Joins with `&`, `|` and `~` like any other condition.
    """
    if not isinstance(condition, Expr):
        raise GodExpressionError(
            "`where` takes a question about a column's name, "
            'like name.starts("q")'
        )
    if value is not None and not isinstance(value, Expr):
        raise GodExpressionError(
            "`where` takes what to make of each column second, "
            "with `value` standing for it: where(name.starts(\"q\"), value * 2)"
        )
    return _Where(condition, value)


def lower(column):
    """Text with its case folded down: `lower(col.region)`.

    Also folds a column's *name*, which is how a name test asks for either
    case: ``pick(where(lower(name).starts("q")))`` matches `Q1_score` where
    ``name.starts("q")`` does not.
    """
    return Expr(f"lower({_column_or_word(column, 'lower')})")


def upper(column):
    """Text with its case folded up: `upper(col.region)`."""
    return Expr(f"upper({_column_or_word(column, 'upper')})")


def _written(value, where: str) -> str:
    """One argument, whether it is a column or an ordinary value."""
    return value._text if isinstance(value, Expr) else _value(value)


# -- converting ---------------------------------------------------------------
#
# **Every conversion begins `to_`, and nothing else does.** Conversion is always
# explicit here and never happens on your behalf, because a column that quietly
# changes what it holds is the defect this grammar is most against.


def to_number(column):
    """This, as a number: `to_number(col.age_text)`."""
    return Expr(f"to_number({_written(column, 'to_number')})")


def to_whole(column):
    """This, as a whole number: `to_whole(col.score)`."""
    return Expr(f"to_whole({_written(column, 'to_whole')})")


def to_text(column):
    """This, as text: `to_text(col.id)`."""
    return Expr(f"to_text({_written(column, 'to_text')})")


def to_date(column):
    """This, as a date: `to_date(col.ordered_on)`."""
    return Expr(f"to_date({_written(column, 'to_date')})")


# -- text ---------------------------------------------------------------------


def trim(column):
    """Text with the spaces taken off both ends: `trim(col.name)`."""
    return Expr(f"trim({_written(column, 'trim')})")


def characters(column):
    """How many characters the text has: `characters(col.name)`.

    Not `length`, because R's `length` counts the elements of a vector, and a
    word that reads as one thing and does another is the one case masking cannot
    be made honest.
    """
    return Expr(f"characters({_written(column, 'characters')})")


def replace_text(column, look_for, put_there):
    """Text with one thing swapped for another.

    ``replace_text(col.name, "-", " ")`` looks for the text itself rather than
    for a pattern, so nothing in it is special.
    """
    return Expr(
        "replace_text({}, {}, {})".format(
            _written(column, "replace_text"),
            _written(look_for, "replace_text"),
            _written(put_there, "replace_text"),
        )
    )


def split_text(column, cut_on, piece):
    """One piece of text cut apart: ``split_text(col.name, " ", 1)``.

    The pieces are counted from 1, and it says which one because every value in
    the grammar is one value. There is no list here to hand back.
    """
    return Expr(
        "split_text({}, {}, {})".format(
            _written(column, "split_text"),
            _written(cut_on, "split_text"),
            _written(piece, "split_text"),
        )
    )


def between(column, low, high):
    """Whether this sits between two ends, counting both: ``between(col.n, 1, 10)``.

    Inclusive at each end, the way SQL's `BETWEEN` and dplyr's `between` both
    are, so nobody arriving from either has to check.
    """
    return Expr(
        "between({}, {}, {})".format(
            _written(column, "between"),
            _written(low, "between"),
            _written(high, "between"),
        )
    )


def _column_or_word(column, verb: str) -> str:
    """A value, or a grammar word left bare.

    `name` and `kind` are the two things that are not columns and can still have
    their case folded, which is what lets a name test ask for either case.

    **Anything else is an ordinary value, including a nested call.**
    `upper(trim(col.raw))` is a sentence the grammar reads and R writes, and this
    refused it: it demanded a bare column and reported `trim([raw])` as not being
    a column name. Two spellings of one grammar disagreeing is the one thing the
    bindings may not do.
    """
    if isinstance(column, Expr) and str(column) in ("name", "kind"):
        return str(column)
    if isinstance(column, Expr) and column._column is None:
        return column._text
    return f"[{name_of(column, verb)}]"


def first_present(*columns):
    """The first of these columns that has a value, reading left to right.

    `add(contact = first_present(col.mobile, col.landline, col.email))` takes the
    mobile where there is one, otherwise the landline, otherwise the email.

    The order matters: these are places to look, in priority order, not a set.
    And the only thing it skips is a missing value, so a `0`, an empty text and a
    `no` are all present and come back. If every column is missing for a row, the
    answer is missing for that row.

    SQL and dplyr both call this `coalesce`.
    """
    if len(columns) < 2:
        raise GodExpressionError(
            "`first_present` looks in at least two columns: "
            "first_present(col.mobile, col.landline)"
        )
    written = ", ".join(f"[{name_of(c, 'first_present')}]" for c in columns)
    return Expr(f"first_present({written})")


def when(*pairs, otherwise=None):
    """The first question that is true, and the answer beside it.

    ``add(band = when(col.score >= 90, "A", col.score >= 70, "B", otherwise = "C"))``

    The arguments come in pairs, a question and then what it gives, and the
    order is the meaning: the first one that is true wins. That is the same
    reading `first_present` asks for, a list in priority order rather than a set.

    Left without an `otherwise`, a row that matched nothing is missing, which is
    what SQL and dplyr both do.

    **It is not spelled with Python's own conditional**, and the reason is
    mechanical rather than stylistic. ``"A" if col.score >= 90 else "B"`` calls
    `__bool__` on the expression, picks a branch while the pipeline is still
    being built, and throws the condition away without raising anything.
    """
    if not pairs:
        raise GodExpressionError(
            "`when` needs at least one question and the answer that goes with "
            'it: when(col.score >= 90, "A", otherwise = "C")'
        )
    if len(pairs) % 2 != 0:
        raise GodExpressionError(
            "each question `when` asks needs the answer that goes with it, right "
            'after it: when(col.score >= 90, "A", otherwise = "C")'
        )

    written = []
    for i in range(0, len(pairs), 2):
        test, value = pairs[i], pairs[i + 1]
        if not isinstance(test, Expr):
            raise GodExpressionError(
                f"`when` asks a question first and gives an answer second, and "
                f"`{test!r}` is not a question. Compare something: "
                "when(col.score >= 90, \"A\")"
            )
        written.append(test._text)
        written.append(value._text if isinstance(value, Expr) else _value(value))
    if otherwise is not None:
        written.append(
            f"otherwise {otherwise._text if isinstance(otherwise, Expr) else _value(otherwise)}"
        )
    return Expr("when({})".format(", ".join(written)))


# -- the parts of a date ------------------------------------------------------


def year(column):
    """The year of a date: `year(col.ordered_on)`."""
    return Expr(f"year({_written(column, 'year')})")


def month(column):
    """The month of a date, 1 to 12."""
    return Expr(f"month({_written(column, 'month')})")


def day(column):
    """The day of the month, 1 to 31."""
    return Expr(f"day({_written(column, 'day')})")


def weekday(column):
    """Which day of the week, **counting Monday as 1**.

    The numbering is the grammar's rather than the engine's, and it has to be:
    asked plainly, DuckDB calls a Friday 5 and Spark calls it 4, and neither
    complains. Here it is 5 wherever you run it.
    """
    return Expr(f"weekday({_written(column, 'weekday')})")


def hour(column):
    """The hour of a time, 0 to 23. A date with no time in it is 0."""
    return Expr(f"hour({_written(column, 'hour')})")


# -- looking along the rows ---------------------------------------------------
#
# **All three have to be told the order**, the way `row_number` does: a total
# *so far* means nothing until a `sort` has said so far in what.


def running_total(column):
    """The total so far, down the rows: `running_total(col.amount)`.

    Needs a `sort` before it, and `by` restarts it for each group.
    """
    return Expr(f"running_total({_written(column, 'running_total')})")


def previous(column):
    """This column's value in the row before: `previous(col.price)`.

    The first row of each group has nothing before it, so it is missing.
    Everywhere else this is called `lag`, which nobody can read aloud.
    """
    return Expr(f"previous({_written(column, 'previous')})")


def following(column):
    """This column's value in the row after: `following(col.price)`.

    The last row of each group has nothing after it, so it is missing.
    """
    return Expr(f"following({_written(column, 'following')})")


def rank(column):
    """A place, with ties sharing one and the next value skipping it.

    `add(place = rank(descending(col.revenue)), by = col.region)` gives each row
    its place within its region, largest first. Ties share a place and the next
    value skips: 1, 2, 2, 4, the way a race is scored.

    `descending` marks the column exactly as it does in `sort`, because a column
    in an ordering position is the same idea in both places.

    dplyr calls this `min_rank`, which names the implementation. This is the one
    a person means when they say rank.
    """
    if isinstance(column, _Descending):
        return Expr(f"rank([{column.name}] descending)")
    return Expr(f"rank([{name_of(column, 'rank')}])")


def row_number():
    """Which row this is, in the order the rows are already in: 1, 2, 3, 4.

    It never ties, which is the difference from `rank`. It takes nothing, so it
    needs a `sort` before it to say what the order is, and is refused without
    one. To number by a column without sorting the table, `rank` says what it
    goes by.
    """
    return Expr("row_number()")


class _AllBut:
    __slots__ = ("names",)

    def __init__(self, names):
        self.names = names


def all_but(*columns):
    """Invert a `pick`: `pick(all_but(col.cost))`."""
    if not columns:
        raise GodExpressionError("`all_but` needs at least one column")
    return _AllBut([name_of(c, "pick") for c in columns])


# -- the functions -----------------------------------------------------------
#
# **These are real functions in Python and are not in R**, and the difference is
# mechanism rather than disagreement. R captures an expression without evaluating
# it, so `total` there is a name in a syntax tree and no such function exists.
# Python evaluates as it goes, so the word has to be something that can be
# called. The vocabulary is identical; only what the host does with it differs.


def _function(name: str, *args) -> Expr:
    written = ", ".join(a._text if isinstance(a, Expr) else _value(a) for a in args)
    return Expr(f"{name}({written})")


def total(column):
    return _function("total", column)


def average(column):
    return _function("average", column)


def median(column):
    return _function("median", column)


def smallest(column):
    return _function("smallest", column)


def largest(column):
    return _function("largest", column)


def first(column):
    return _function("first", column)


def last(column):
    return _function("last", column)


def unique_count(column):
    return _function("unique_count", column)


def row_count():
    """How many rows. It asks about rows rather than about a column, so it takes
    no argument, and it is named for the value it returns."""
    return _function("row_count")


# -- building the sentence ---------------------------------------------------


def _assignments(values: dict, verb: str) -> str:
    if not values:
        raise GodExpressionError(f"`{verb}` needs at least one column")
    parts = []
    for name, value in values.items():
        written = value._text if isinstance(value, Expr) else _value(value)
        parts.append(f"[{name}] as {written}")
    return ", ".join(parts)


def lengthen(*names, name=None, value=None):
    """Turn columns into rows. The table grows taller, which is what the name says.

    The columns are chosen the three ways `pick` chooses them::

        answers >> lengthen(col.q1, col.q2, col.q3)
        answers >> lengthen(all_but(col.id))
        answers >> lengthen(where(name.starts("q")))

    Left unnamed, the two new columns are called `name` and `value`, which are
    the grammar's own words for them — so `lengthen(...) >> widen()` is the round
    trip written with nothing at all.

    Where the old names hold more than one thing, say what they look like::

        answers >> lengthen(all_but(col.id), name = "{question}_{year}", value = col.answer)

    and `{value}` for a piece says that piece picks which value column a row
    belongs to, which is what tidyr spells `.value`.
    """
    if not names:
        raise GodExpressionError(
            "`lengthen` needs the columns that become rows: "
            "lengthen(col.q1, col.q2, col.q3). all_but(col.id) names the ones to "
            "leave instead"
        )

    if len(names) == 1 and isinstance(names[0], _Where):
        if names[0].value is not None:
            raise GodExpressionError(
                "`where` here only chooses the columns, so it takes the question "
                'alone: lengthen(where(name.starts("q")))'
            )
        selector = f"where {names[0].condition}"
    elif any(isinstance(n, _Where) for n in names):
        raise GodExpressionError(
            "`where` chooses the columns on its own, so nothing goes beside it: "
            'lengthen(where(name.starts("q")))'
        )
    else:
        inverted = len(names) == 1 and isinstance(names[0], _AllBut)
        if inverted:
            chosen = names[0].names
        else:
            if any(isinstance(n, _AllBut) for n in names):
                raise GodExpressionError(
                    "write the columns inside `all_but()`, all of them: "
                    "lengthen(all_but(col.id, col.region))"
                )
            chosen = [name_of(n, "lengthen") for n in names]
        selector = f"{'all_but ' if inverted else ''}[{', '.join(chosen)}]"

    said = _naming(name, value, value_is_expression=False)
    written = f"lengthen {selector}" + (f" as {said}" if said else "")
    return _Verb("lengthen", lambda: written)


def widen(name=None, value=None, by=None, missing=None, giving=None):
    """Turn rows into columns. The inverse of `lengthen`, read the other way.

    `name` points at the column holding column names in both verbs; here it is
    being read rather than made, and the verb is what says so::

        answers >> widen(name = col.question, value = col.answer)

    A bare column in `value` means one row per cell, and the query stops and
    names the cell if two rows want one. An aggregation says what to do about
    that instead, which is what tidyr spends `values_fn` on::

        answers >> widen(name = col.question, value = average(col.answer))

    `giving` says which columns this makes. Without it the column names come
    from the data, which nothing can know before the query runs, so nothing may
    follow.
    """
    said = _naming(name, value, value_is_expression=True)
    written = "widen" + (f" {said}" if said else "")
    written += _grouping(by)
    if missing is not None:
        filler = missing._text if isinstance(missing, Expr) else _value(missing)
        written += f" missing {filler}"
    if giving is not None:
        columns = giving if isinstance(giving, (list, tuple)) else [giving]
        if not columns:
            raise GodExpressionError(
                "`giving` needs the columns this makes: giving = [col.q1, col.q2]"
            )
        listed = ", ".join(name_of(c, "giving") for c in columns)
        written += f" giving [{listed}]"
    return _Verb("widen", lambda: written)


def _naming(name, value, *, value_is_expression: bool) -> str:
    """`name ...` and `value ...`, the pair both reshaping verbs take.

    One writer, because the two verbs take the same two words and differ only in
    which way they flow. A quoted string in `name` is the shape of the names
    rather than a column called that, which is the rule the text form has too:
    brackets are a column and quotes are a pattern.
    """
    said = []
    if name is not None:
        written = f'"{name}"' if isinstance(name, str) else f"[{name_of(name, 'name')}]"
        said.append(f"name {written}")
    if value is not None:
        if value_is_expression:
            written = value._text if isinstance(value, Expr) else _value(value)
        else:
            written = f"[{name_of(value, 'value')}]"
        said.append(f"value {written}")
    return ", ".join(said)


def _grouping(by) -> str:
    if by is None:
        return ""
    columns = by if isinstance(by, (list, tuple)) else [by]
    if not columns:
        return ""
    names = ", ".join(name_of(c, "by") for c in columns)
    return f" by [{names}]"


# -- materializing -----------------------------------------------------------


def collect(pipeline: Pipeline):
    """Run a pipeline and return the frame.

    Nothing runs until the answer is wanted. Printing a pipeline runs it; this is
    the explicit form, for when you want the frame rather than the look of it.
    """
    if not isinstance(pipeline, Pipeline):
        raise GodExpressionError("`collect` runs a god pipeline, and this is not one")
    from .run import _query

    return _query(pipeline.written(), pipeline.tables, pipeline.source)
