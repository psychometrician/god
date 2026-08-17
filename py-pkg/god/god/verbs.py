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

from .columns import Expr, GodExpressionError, is_frame as _is_frame, name_of, _value

__all__ = [
    "keep", "pick", "add", "summarize", "sort", "take", "take_last", "join",
    "add_rows", "add_combinations", "drop_duplicates", "rename",
    "drop_missing", "fill_missing",
    "descending", "all_but", "collect",
    "total", "average", "median", "smallest", "largest",
    "first", "last", "unique_count", "row_count",
    "rank", "row_number", "first_present", "lower", "upper",
    "where", "where_any", "where_every", "name", "value", "kind",
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> keep(col.region == "West"))
      region  revenue
    0   West      100
    1   West      150
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West", "North"],
    ...                       "product": ["Widget", "Gadget", "Doohickey", "Widget"],
    ...                       "revenue": [100, 120, 120, 150],
    ...                       "cost": [40, 80, 75, 60]})
    >>> collect(sales >> pick(col.region, col.revenue))
      region  revenue
    0   West      100
    1   East      120
    2   West      120
    3  North      150
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(doubled=col.revenue * 2))
      region  revenue  doubled
    0   West      100      200
    1   East      120      240
    2   West      150      300
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(sold=total(col.revenue), by=col.region))
      region   sold
    0   East  120.0
    1   West  250.0
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(descending(col.revenue)))
      region  revenue
    0   West      150
    1   East      120
    2   West      100
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


def take(n, *, by=None, ties=False):
    """The first n rows, or the first n of each group.

    `by` needs a `sort` before it, because "the first rows" means nothing until
    something says first by what.

    An ordinary Python value, so a threshold held in a variable works. This is
    the one position where no column could appear, which is why a bare name here
    is a value rather than a column.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> take(2))
      region  revenue
    0   West      100
    1   East      120
    """
    if isinstance(n, Expr) or isinstance(n, bool) or not isinstance(n, int):
        raise GodExpressionError("`take` needs a whole number of rows: take(10)")
    grouping = _grouping(by)
    tied = " with ties" if ties else ""
    return _Verb("take", lambda: f"take {n}{grouping}{tied}")


def take_last(n, *, by=None, ties=False):
    """The last n rows, or the last n of each group.

    The other end of ``take``. It always needs a ``sort`` before it, where a bare
    ``take`` does not: "the first rows" of an unsorted table is at least the rows
    the pipeline reached first, and "the last rows" is a claim about an end that a
    table does not have until something says which way it runs.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(col.revenue) >> take_last(2))
      region  revenue
    0   East      120
    1   West      150
    """
    if isinstance(n, Expr) or isinstance(n, bool) or not isinstance(n, int):
        raise GodExpressionError("`take_last` needs a whole number of rows: take_last(10)")
    grouping = _grouping(by)
    tied = " with ties" if ties else ""
    return _Verb("take_last", lambda: f"take_last {n}{grouping}{tied}")


def join(other, *, by=None, unmatched="this"):
    """Add another table's columns.

    `by` names the columns that say which rows correspond. Left out, the columns
    both tables share are used and god says which it chose.

    Where the two tables name a key differently, write both with ``==`` between
    them and this table's first: ``by=col.customer_id == col.id``. The answer
    keeps this table's name. Several keys go in a list, and the two forms mix:
    ``by=[col.region, col.customer_id == col.id]``.

    `unmatched` says whose unmatched rows survive: ``"this"`` keeps this table's
    and is the default, ``"none"`` keeps neither, ``"both"`` keeps both. There is
    no ``"other"``, because that is this join with the tables the other way
    round.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West", "North"],
    ...                       "product": ["Widget", "Gadget", "Doohickey", "Widget"],
    ...                       "revenue": [100, 120, 120, 150],
    ...                       "cost": [40, 80, 75, 60]})
    >>> products = pd.DataFrame({"product": ["Widget", "Gadget"],
    ...                          "maker": ["Acme", "Globex"]})
    >>> collect(sales >> pick(col.product, col.revenue) >> join(products, by=col.product) >> sort(col.product))
         product  revenue   maker
    0  Doohickey      120     NaN
    1     Gadget      120  Globex
    2     Widget      150    Acme
    3     Widget      100    Acme
    """
    if not _is_frame(other):
        raise GodExpressionError(
            f"`join` needs another table, and this is {type(other).__name__}"
        )
    name = _name_in_caller(other)
    keys = _join_keys(by)

    def write():
        matched = f" by {keys}" if keys else ""
        survivors = "" if unmatched == "this" else f' unmatched "{unmatched}"'
        return f"join {name}{matched}{survivors}"

    return _Verb("join", write, brings=(name, other))


def _join_keys(by):
    """The columns that say which rows of two tables correspond, as text.

    Four shapes, and the last two are what arrived on 2026-08-16::

        by=col.id                            the same word on both sides
        by=[col.region, col.product]         several, all the same word
        by=col.customer_id == col.id         named differently on each side
        by=[col.region, col.customer_id == col.id]        the two mixed

    `==` is what Python writes where the grammar writes `is`, which is the same
    trade every condition already makes — the vocabulary is identical and only
    the idiom moves.

    A run of shared names collapses into one bracket group, so the sentence that
    goes to the engine is the one the caller would have written by hand.
    """
    if by is None:
        return ""
    given = by if isinstance(by, (list, tuple)) else [by]

    written = []
    shared = []

    def flush():
        if shared:
            written.append(f"[{', '.join(shared)}]")
            shared.clear()

    for one in given:
        pair = getattr(one, "_pair", None)
        if pair is not None:
            flush()
            written.append(f"[{pair[0]}] is [{pair[1]}]")
        else:
            shared.append(name_of(one, "by"))
    flush()
    return ", ".join(written)


def matching(other, *, by=None):
    """Whether a row has a partner in another table.

    A condition rather than a verb, because a semi join and an anti join add no
    columns — they only decide which rows survive::

        sales >> keep(matching(products, by="id"))       # semi join
        sales >> keep(~matching(products, by="id"))      # anti join

    `by` works exactly as it does on `join`, down to the form for a key the two
    tables name differently: ``by=col.product == col.item``. Left out, the
    columns both tables share are used and god says which it chose.

    It cannot multiply rows, which is the one thing `join` cannot promise: a row
    either has a partner or it does not, however many it has.

    It is the whole question `keep` asks rather than one part of one, so it does
    not combine with `&` or `|`. Ask it in its own step.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"product": ["Widget", "Gadget", "Widget"],
    ...                       "revenue": [100, 120, 150]})
    >>> stocked = pd.DataFrame({"product": ["Widget"]})
    >>> collect(sales >> keep(matching(stocked, by=col.product)))
      product  revenue
    0  Widget      100
    1  Widget      150
    """
    if not _is_frame(other):
        raise GodExpressionError(
            f"`matching` needs another table, and this is {type(other).__name__}"
        )
    name = _name_in_caller(other)
    keys = _join_keys(by)
    written = f", by {keys}" if keys else ""
    return Expr(f"matching({name}{written})", brings=(name, other))


def add_rows(other):
    """Add another table's rows.

    Both tables need the same columns. A column on one side only is refused
    rather than filled in with missing values, because a column that is half
    empty and says nothing is how a mistake survives.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> late = pd.DataFrame({"region": ["North"], "revenue": [80]})
    >>> collect(sales >> add_rows(late))
      region  revenue
    0   West      100
    1   East      120
    2   West      150
    3  North       80
    """
    if not _is_frame(other):
        raise GodExpressionError(
            f"`add_rows` needs another table, and this is {type(other).__name__}"
        )
    name = _name_in_caller(other)
    return _Verb("add_rows", lambda: f"add_rows {name}", brings=(name, other))


def add_combinations(*names, by=None):
    """Make the absent combinations appear: ``add_combinations(col.region, col.product)``.

    Every combination of the values these columns already hold, as rows. The rows
    that were there are handed on untouched; the ones that were not arrive with
    every other column missing, and ``fill_missing`` is what says otherwise.

    The values come from the table and nowhere else, so a month with no row
    anywhere is never invented. A missing value is not a category and makes no
    combinations, and no row is lost by that: nothing already in the table is
    touched at all.

    ``by`` makes the combinations inside each group. Without it the whole table
    is one group. With it, a new row keeps those columns filled in rather than
    going missing, which is the reason to write one.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "product": ["Widget", "Widget", "Gadget"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add_combinations(col.region, col.product) >> fill_missing(revenue=0))
      region product  revenue
    0   West  Widget      100
    1   East  Widget      120
    2   West  Gadget      150
    3   East  Gadget        0
    """
    if not names:
        raise GodExpressionError(
            "`add_combinations` needs the columns whose combinations to make: "
            "add_combinations(col.region, col.product)"
        )
    listed = ", ".join(name_of(n, "add_combinations") for n in names)
    grouping = _grouping(by)
    return _Verb("add_combinations", lambda: f"add_combinations [{listed}]{grouping}")


def drop_duplicates():
    """Drop rows that are identical across every column.

    The answer comes back in a settled order, because dropping duplicates says
    nothing about order and an answer that reorders itself is not predictable.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "product": ["Widget", "Gadget", "Widget"]})
    >>> collect(sales >> drop_duplicates())
      region product
    0   East  Gadget
    1   West  Widget
    """
    return _Verb("drop_duplicates", lambda: "drop_duplicates")


def rename(**pairs):
    """Rename a column: ``rename(margin = col.profit)``.

    The new name goes first, the way it does everywhere else in the grammar and
    the way assignment reads. Note that pandas writes the pair the other way
    round, and both spellings are legal here, so this is worth reading twice.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> rename(area=col.region))
       area  revenue
    0  West      100
    1  East      120
    2  West      150
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
    """Drop rows with missing values. With no columns named, every column.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> patchy = pd.DataFrame({"product": ["Widget", "Gadget"],
    ...                        "revenue": [100.0, None]})
    >>> collect(patchy >> drop_missing(col.revenue))
      product  revenue
    0  Widget    100.0
    """
    if not names:
        return _Verb("drop_missing", lambda: "drop_missing")
    listed = ", ".join(name_of(n, "drop_missing") for n in names)
    return _Verb("drop_missing", lambda: f"drop_missing [{listed}]")


def fill_missing(**values):
    """Replace missing values: ``fill_missing(price = 0)``.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> patchy = pd.DataFrame({"product": ["Widget", "Gadget"],
    ...                        "revenue": [100.0, None]})
    >>> collect(patchy >> fill_missing(revenue=0))
      product  revenue
    0  Widget    100.0
    1  Gadget      0.0
    """
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(descending(col.revenue)))
      region  revenue
    0   West      150
    1   East      120
    2   West      100
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


def where_any(condition, test):
    """Keep the rows where **any** of the matched columns answers yes.

    dplyr spells this `when_any`, pandas and polars `.any(axis=1)`. The columns
    are chosen the same way `where` chooses them, and ``value`` stands for the
    column being asked about::

        survey >> keep(where_any(name.starts("q"), value > 3))

    **The name is a compound rather than the grammar's own word**, and both
    bindings use the same one. The text form says `any`, which it can, because
    nothing there is evaluated; Python evaluates, and `any` is a builtin the
    vocabulary has avoided shadowing since `total` was chosen over `sum`. One
    spelling in R and Python is worth more than each matching the text form on
    its own.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> survey = pd.DataFrame({"who": ["ana", "ben"],
    ...                        "q1": [4, 1], "q2": [1, 2]})
    >>> collect(survey >> keep(where_any(name.starts("q"), value > 3)))
       who  q1  q2
    0  ana   4   1
    """
    return _quantified(condition, test, "any")


def where_every(condition, test):
    """Keep the rows where **every** matched column answers yes.

    The other half of `where_any`, and the same shape::

        survey >> keep(where_every(name.starts("q"), value > 3))

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> survey = pd.DataFrame({"who": ["ana", "ben"],
    ...                        "q1": [4, 1], "q2": [5, 2]})
    >>> collect(survey >> keep(where_every(name.starts("q"), value > 3)))
       who  q1  q2
    0  ana   4   5
    """
    return _quantified(condition, test, "every")


def _quantified(condition, test, word):
    if not isinstance(condition, Expr):
        raise GodExpressionError(
            f"`where_{word}` takes a question about a column's name first, "
            'like name.starts("q")'
        )
    if not isinstance(test, Expr):
        raise GodExpressionError(
            f"`where_{word}` takes the question to ask of each column second, "
            f'with `value` standing for it: where_{word}(name.starts("q"), value > 3)'
        )
    return Expr(f"{word} {condition} as {test}")


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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> survey = pd.DataFrame({"name": ["ana", "ben"],
    ...                        "q1_score": [4, 5], "q2_score": [2, 5]})
    >>> collect(survey >> pick(where(name.starts("q"))))
       q1_score  q2_score
    0         4         2
    1         5         5

    >>> survey = pd.DataFrame({"name": ["ana", "ben"],
    ...                        "q1_score": [4, 5], "q2_score": [2, 5]})
    >>> collect(survey >> add(where(name.starts("q"), value * 10)))
      name  q1_score  q2_score
    0  ana        40        20
    1  ben        50        50

    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> pick(where(kind == "number")))
       revenue
    0      100
    1      120
    2      150
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"product": ["Widget", "Gadget", "Widget"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(quiet=lower(col.product)) >> pick(col.product, col.quiet))
      product   quiet
    0  Widget  widget
    1  Gadget  gadget
    2  Widget  widget
    """
    return Expr(f"lower({_column_or_word(column, 'lower')})")


def upper(column):
    """Text with its case folded up: `upper(col.region)`.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"product": ["Widget", "Gadget", "Widget"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(shout=upper(col.product)) >> pick(col.product, col.shout))
      product   shout
    0  Widget  WIDGET
    1  Gadget  GADGET
    2  Widget  WIDGET
    """
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
    """This, as a number: `to_number(col.age_text)`.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> messy = pd.DataFrame({"raw": ["  ann marie  ", "  bob  "], "n": [7, 99]})
    >>> collect(messy >> add(size=to_number(to_text(col.n))) >> pick(col.size))
       size
    0   7.0
    1  99.0
    """
    return Expr(f"to_number({_written(column, 'to_number')})")


def round_below(column):
    """The whole number below: `round_below(col.score)`.

    Always toward the smaller number, so `round_below(-5.5)` is -6 rather than
    -5. A value that is already whole does not move.

    **It is `below` and not `down` on purpose.** Spreadsheets round "down"
    toward zero, which would make -5.5 into -5, so the plainer-looking word
    would have meant two things to two readers with nothing to warn either.

    For the nearest whole number rather than the one below, add a half first:
    ``round_below(col.x + 0.5)``.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(each=round_below(col.revenue / 7)) >> pick(col.each))
       each
    0    14
    1    17
    2    21
    """
    return Expr(f"round_below({_written(column, 'round_below')})")


def round_above(column):
    """The whole number above: `round_above(col.score)`.

    Always toward the larger number, so `round_above(-5.5)` is -5 and
    `round_above(5.1)` is 6. A value that is already whole does not move.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(pages=round_above(col.revenue / 7)) >> pick(col.pages))
       pages
    0     15
    1     18
    2     22
    """
    return Expr(f"round_above({_written(column, 'round_above')})")


def to_text(column):
    """This, as text: `to_text(col.id)`.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(written=to_text(col.revenue)) >> pick(col.written))
      written
    0     100
    1     120
    2     150
    """
    return Expr(f"to_text({_written(column, 'to_text')})")


def to_date(column):
    """This, as a date: `to_date(col.ordered_on)`.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> diary = pd.DataFrame({"on_": ["2026-01-02", "2026-01-05"],
    ...                       "x": [10, 20]})
    >>> collect(diary >> add(day=to_date(col.on_)) >> pick(col.day))
             day
    0 2026-01-02
    1 2026-01-05
    """
    return Expr(f"to_date({_written(column, 'to_date')})")


# -- text ---------------------------------------------------------------------


def trim(column):
    """Text with the spaces taken off both ends: `trim(col.name)`.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> messy = pd.DataFrame({"raw": ["  ann marie  ", "  bob  "], "n": [7, 99]})
    >>> collect(messy >> add(name=trim(col.raw)) >> pick(col.name))
            name
    0  ann marie
    1        bob
    """
    return Expr(f"trim({_written(column, 'trim')})")


def characters(column):
    """How many characters the text has: `characters(col.name)`.

    Not `length`, because R's `length` counts the elements of a vector, and a
    word that reads as one thing and does another is the one case masking cannot
    be made honest.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> messy = pd.DataFrame({"raw": ["  ann marie  ", "  bob  "], "n": [7, 99]})
    >>> collect(messy >> add(width=characters(trim(col.raw))) >> pick(col.width))
       width
    0      9
    1      3
    """
    return Expr(f"characters({_written(column, 'characters')})")


def replace_text(column, look_for, put_there):
    """Text with one thing swapped for another.

    ``replace_text(col.name, "-", " ")`` looks for the text itself rather than
    for a pattern, so nothing in it is special.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"product": ["Widget", "Gadget", "Widget"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(kind=replace_text(col.product, "Widget", "Sprocket")) >> pick(col.kind))
           kind
    0  Sprocket
    1    Gadget
    2  Sprocket
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> messy = pd.DataFrame({"raw": ["  ann marie  ", "  bob  "], "n": [7, 99]})
    >>> collect(messy >> add(first=split_text(trim(col.raw), " ", 1)) >> pick(col.first))
      first
    0   ann
    1   bob
    """
    return Expr(
        "split_text({}, {}, {})".format(
            _written(column, "split_text"),
            _written(cut_on, "split_text"),
            _written(piece, "split_text"),
        )
    )


def join_text(*parts):
    """Text values joined into one: ``join_text(col.first, " ", col.last)``.

    This is ``split_text`` read the other way. A separator is written where it
    goes, as a value, rather than being a setting somewhere else, so the call
    says aloud what comes out.

    **Missing anywhere makes the answer missing**, which is the rule arithmetic
    already follows. To fill a hole instead of losing the row, say what to fill
    it with: ``join_text(col.first, " ", first_present(col.last, ""))``.

    Numbers are refused rather than converted, because how a number should look
    is a decision: use ``to_text`` and make it.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(label=join_text(col.region, " ", to_text(col.revenue))) >> pick(col.label))
          label
    0  West 100
    1  East 120
    2  West 150
    """
    if len(parts) < 2:
        raise GodExpressionError(
            "`join_text` joins at least two things: join_text(col.first, col.last)"
        )
    written = ", ".join(_written(p, "join_text") for p in parts)
    return Expr(f"join_text({written})")


def between(column, low, high):
    """Whether this sits between two ends, counting both: ``between(col.n, 1, 10)``.

    Inclusive at each end, the way SQL's `BETWEEN` and dplyr's `between` both
    are, so nobody arriving from either has to check.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> keep(between(col.revenue, 110, 140)))
      region  revenue
    0   East      120
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> patchy = pd.DataFrame({"product": ["Widget", "Gadget"],
    ...                        "revenue": [100.0, None],
    ...                        "listed": [90.0, 60.0]})
    >>> collect(patchy >> add(price=first_present(col.revenue, col.listed)))
      product  revenue  listed  price
    0  Widget    100.0    90.0  100.0
    1  Gadget      NaN    60.0   60.0
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(size=when(col.revenue > 120, "big", otherwise="small")))
      region  revenue   size
    0   West      100  small
    1   East      120  small
    2   West      150    big
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
    """The year of a date: `year(col.ordered_on)`.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> diary = pd.DataFrame({"on_": ["2026-01-02", "2026-01-05"],
    ...                       "x": [10, 20]})
    >>> collect(diary >> add(y=year(to_date(col.on_))) >> pick(col.y))
          y
    0  2026
    1  2026
    """
    return Expr(f"year({_written(column, 'year')})")


def month(column):
    """The month of a date, 1 to 12.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> diary = pd.DataFrame({"on_": ["2026-01-02", "2026-01-05"],
    ...                       "x": [10, 20]})
    >>> collect(diary >> add(m=month(to_date(col.on_))) >> pick(col.m))
       m
    0  1
    1  1
    """
    return Expr(f"month({_written(column, 'month')})")


def day(column):
    """The day of the month, 1 to 31.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> diary = pd.DataFrame({"on_": ["2026-01-02", "2026-01-05"],
    ...                       "x": [10, 20]})
    >>> collect(diary >> add(d=day(to_date(col.on_))) >> pick(col.d))
       d
    0  2
    1  5
    """
    return Expr(f"day({_written(column, 'day')})")


def weekday(column):
    """Which day of the week, **counting Monday as 1**.

    The numbering is the grammar's rather than the engine's, and it has to be:
    asked plainly, DuckDB calls a Friday 5 and Spark calls it 4, and neither
    complains. Here it is 5 wherever you run it.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> diary = pd.DataFrame({"on_": ["2026-01-02", "2026-01-05"],
    ...                       "x": [10, 20]})
    >>> collect(diary >> add(w=weekday(to_date(col.on_))) >> pick(col.w))
       w
    0  5
    1  1
    """
    return Expr(f"weekday({_written(column, 'weekday')})")


def hour(column):
    """The hour of a time, 0 to 23. A date with no time in it is 0.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> diary = pd.DataFrame({"on_": ["2026-01-02", "2026-01-05"],
    ...                       "x": [10, 20]})
    >>> collect(diary >> add(h=hour(to_date(col.on_))) >> pick(col.h))
       h
    0  0
    1  0
    """
    return Expr(f"hour({_written(column, 'hour')})")


# -- looking along the rows ---------------------------------------------------
#
# **All three have to be told the order**, the way `row_number` does: a total
# *so far* means nothing until a `sort` has said so far in what.


def remainder(column, divisor):
    """What is left over after dividing: `remainder(col.n, 3)`.

    The one arithmetic operator with no composition in the grammar. Integer
    division has one once this exists — ``(col.n - remainder(col.n, 3)) / 3`` —
    and a square is ``col.x * col.x``, so neither gets a word of its own.

    **The sign is the grammar's, not the engine's.** R, Python, pandas and
    polars all answer 1 for ``-7 % 2``; DuckDB and Spark both answer -1, and
    neither raises. god names the first — the answer takes the divisor's sign,
    which is what makes bucketing work — and gives each engine the spelling
    that produces it.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> rows = pd.DataFrame({"n": [1, 2, 3, 4]})
    >>> collect(rows >> keep(remainder(col.n, 2) == 0))
       n
    0  2
    1  4
    """
    return Expr(f"remainder({_written(column, 'remainder')}, {_value(divisor)})")


def latest(column):
    """The last value that was there, reading down: `latest(col.reading)`.

    Fills a hole with the most recent value above it, which tidyr spells
    ``fill``, pandas ``ffill`` and polars ``forward_fill``. Needs a `sort`
    before it, like every window, and `by` restarts it for each group.

    A row with nothing above it and nothing of its own stays missing. To put a
    value there instead, `fill_missing` after it.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> readings = pd.DataFrame({"at": [1, 2, 3, 4],
    ...                          "reading": [10.0, None, None, 40.0]})
    >>> collect(readings >> sort(col.at) >> add(reading=latest(col.reading)))
       at  reading
    0   1     10.0
    1   2     10.0
    2   3     10.0
    3   4     40.0
    """
    return Expr(f"latest({_written(column, 'latest')})")


def running_total(column):
    """The total so far, down the rows: `running_total(col.amount)`.

    Needs a `sort` before it, and `by` restarts it for each group.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(col.revenue) >> add(so_far=running_total(col.revenue)))
      region  revenue  so_far
    0   West      100   100.0
    1   East      120   220.0
    2   West      150   370.0
    """
    return Expr(f"running_total({_written(column, 'running_total')})")


def previous(column, how_far=1):
    """This column's value in the row before: `previous(col.price)`.

    The first row of each group has nothing before it, so it is missing.
    Everywhere else this is called `lag`, which nobody can read aloud.

    `how_far` says how many rows back, and one is the default. A year-over-year
    comparison on monthly rows is `previous(col.revenue, 12)`. It is a plain
    whole number and cannot be worked out per row.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(col.revenue) >> add(before=previous(col.revenue)))
      region  revenue  before
    0   West      100    <NA>
    1   East      120     100
    2   West      150     120
    """
    return Expr(f"previous({_written(column, 'previous')}{_how_far(how_far)})")


def following(column, how_far=1):
    """This column's value in the row after: `following(col.price)`.

    The last row of each group has nothing after it, so it is missing.

    `how_far` says how many rows on, and one is the default. It is a plain whole
    number and cannot be worked out per row.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(col.revenue) >> add(after=following(col.revenue)))
      region  revenue  after
    0   West      100    120
    1   East      120    150
    2   West      150   <NA>
    """
    return Expr(f"following({_written(column, 'following')}{_how_far(how_far)})")


def _how_far(how_far):
    """How far `previous` or `following` looks, written only when it was asked for.

    **The default renders as nothing**, so the common sentence stays the short
    one it always was and the round trip hands back what the caller wrote.

    Everything about what a legal offset *is* — a whole number, at least one,
    never a column — is the engine's question and is answered there, once, for
    both languages (Law 7). This passes the value along and does not judge it.
    """
    return "" if how_far == 1 else f", {_value(how_far)}"


def rank(column):
    """A place, with ties sharing one and the next value skipping it.

    `add(place = rank(descending(col.revenue)), by = col.region)` gives each row
    its place within its region, largest first. Ties share a place and the next
    value skips: 1, 2, 2, 4, the way a race is scored.

    `descending` marks the column exactly as it does in `sort`, because a column
    in an ordering position is the same idea in both places.

    dplyr calls this `min_rank`, which names the implementation. This is the one
    a person means when they say rank.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> add(place=rank(descending(col.revenue))))
      region  revenue  place
    0   West      150      1
    1   East      120      2
    2   West      100      3
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(col.revenue) >> add(n=row_number()))
      region  revenue  n
    0   West      100  1
    1   East      120  2
    2   West      150  3
    """
    return Expr("row_number()")


class _AllBut:
    __slots__ = ("names",)

    def __init__(self, names):
        self.names = names


def all_but(*columns):
    """Invert a `pick`: `pick(all_but(col.cost))`.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> patchy = pd.DataFrame({"product": ["Widget", "Gadget"],
    ...                        "revenue": [100.0, None],
    ...                        "listed": [90.0, 60.0]})
    >>> collect(patchy >> pick(all_but(col.listed)))
      product  revenue
    0  Widget    100.0
    1  Gadget      NaN
    """
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
    """Add up a column, over a group or the whole table.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(sold=total(col.revenue), by=col.region))
      region   sold
    0   East  120.0
    1   West  250.0
    """
    return _function("total", column)


def average(column):
    """The mean of a column.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(typical=average(col.revenue)))
          typical
    0  123.333333
    """
    return _function("average", column)


def median(column):
    """The middle value of a column.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(middle=median(col.revenue)))
       middle
    0   120.0
    """
    return _function("median", column)


def smallest(column):
    """The lowest value in a column.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(low=smallest(col.revenue), by=col.region))
      region  low
    0   East  120
    1   West  100
    """
    return _function("smallest", column)


def largest(column):
    """The highest value in a column.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(high=largest(col.revenue), by=col.region))
      region  high
    0   East   120
    1   West   150
    """
    return _function("largest", column)


def first(column):
    """The value in the first row of a group.

    A group's rows have no order of their own, so this means the first row
    as the pipeline reached it. Put a ``sort`` before it to say which.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(col.revenue) >> summarize(cheapest=first(col.revenue), by=col.region))
      region  cheapest
    0   East       120
    1   West       100
    """
    return _function("first", column)


def last(column):
    """The value in the last row of a group.

    Wants a ``sort`` before it for the same reason ``first`` does.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> sort(col.revenue) >> summarize(dearest=last(col.revenue), by=col.region))
      region  dearest
    0   East      120
    1   West      150
    """
    return _function("last", column)


def unique_count(column):
    """How many different values a column holds.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"product": ["Widget", "Gadget", "Widget"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(kinds=unique_count(col.product)))
       kinds
    0      2
    """
    return _function("unique_count", column)


def row_count():
    """How many rows. It asks about rows rather than about a column, so it takes
    no argument, and it is named for the value it returns.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> summarize(orders=row_count(), by=col.region))
      region  orders
    0   East       1
    1   West       2
    """
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> answers = pd.DataFrame({"student": ["ann", "bob"],
    ...                         "q1": [1, 4], "q2": [2, 5]})
    >>> collect(answers >> lengthen(col.q1, col.q2))
      student name  value
    0     ann   q1      1
    1     ann   q2      2
    2     bob   q1      4
    3     bob   q2      5
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> marks = pd.DataFrame({"student": ["ann", "ann", "bob", "bob"],
    ...                       "question": ["q1", "q2", "q1", "q2"],
    ...                       "mark": [1, 2, 4, 5]})
    >>> collect(marks >> widen(name=col.question, value=col.mark, by=col.student, giving=[col.q1, col.q2]))
      student  q1  q2
    0     ann   1   2
    1     bob   4   5
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

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> keep(col.region == "West"))
      region  revenue
    0   West      100
    1   West      150
    """
    if not isinstance(pipeline, Pipeline):
        raise GodExpressionError("`collect` runs a god pipeline, and this is not one")
    from .run import _query

    return _query(pipeline.written(), pipeline.tables, pipeline.source)
