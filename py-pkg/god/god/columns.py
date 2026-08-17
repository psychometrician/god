"""How Python names a column, and what you can say about one.

**This is the Python half of the one piece of real logic that lives in a host.**
Its opposite number is the R translator, and the two exist to do the same job by
different means: turn what someone wrote in their own language into the grammar's
own text.

The means differ because the languages do. R captures an expression without
evaluating it and reads its syntax tree, so `total` and `descending` there are
names in a tree and no function called `total` exists. Python evaluates as it
goes, so the same words have to be real objects that build a sentence as they are
called. **Same decisions, different mechanism**, and that difference is exactly
why the parity harness compares the two: a translator can only drift from its
twin, never from itself.

The one rule that shapes everything here: an operator on a column returns a piece
of the grammar's text rather than a value. `col.region == "West"` is not a
comparison, it is the sentence `[region] is "West"` being written down.
"""

from __future__ import annotations

__all__ = ["col", "Expr"]


class Expr:
    """A piece of a sentence, in the grammar's words.

    Operators build bigger pieces rather than computing anything. Nothing here
    knows what is in the table, and nothing here checks: whether `[revenue]`
    exists and whether `total` may appear where it was written are the grammar's
    questions, answered once, in one place, for both languages.

    **This is also what `help()` reaches for `name`, `value` and `kind`**, which
    are instances rather than functions. All three stand inside `where(...)`
    and `help(god.where)` shows each one working.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> col.revenue > 110
    god expression: ([revenue] > 110)
    >>> sales = pd.DataFrame({"region": ["West", "East"], "revenue": [100, 120]})
    >>> collect(sales >> keep(col.revenue > 110))
      region  revenue
    0   East      120
    """

    __slots__ = ("_text", "_negation", "_column", "_brings", "_pair")

    def __init__(
        self,
        text: str,
        negation: str | None = None,
        column: str | None = None,
        brings=None,
        pair=None,
    ):
        self._text = text
        # The table this expression reads, when it reads one. `matching` is the
        # only expression that names a table, and `keep` has to hand that table
        # over with the pipeline; nothing else in a sentence reaches outside the
        # one table at its head.
        self._brings = brings
        # Two forms have a better spelling for their own negation than wrapping
        # them in `not` would give, and reaching it matters because it is what a
        # person would have written by hand.
        self._negation = negation
        # The bare name, when this expression is nothing but a column. `pick`,
        # `by` and `sort` take names rather than values, and this is how they
        # tell the difference without parsing the text back.
        self._column = column
        # The two column names, when this expression is one column `is`
        # another. A join key names both sides and needs them apart; the same
        # reason `_column` exists, one level up.
        self._pair = pair

    def __str__(self) -> str:
        return self._text

    def __repr__(self) -> str:
        return f"god expression: {self._text}"

    def __bool__(self):
        # A condition is a column expression, not a yes or no, so Python's
        # `and`, `or`, `in` and `if` have nothing true or false to read here.
        # Refusing beats the alternative: an object is truthy by default, so
        # every one of those would quietly answer yes.
        from .run import GodError

        raise GodError(
            "this is a column expression, not a yes or no. Combine conditions "
            "with `&`, `|` and `~`, and hand the whole expression to a verb"
        )

    # -- the words where the hosts disagree ---------------------------------

    def __eq__(self, other):
        return _infix(self, "is", other)

    def __ne__(self, other):
        return _infix(self, "is not", other)

    def __lt__(self, other):
        return _infix(self, "<", other)

    def __le__(self, other):
        return _infix(self, "<=", other)

    def __gt__(self, other):
        return _infix(self, ">", other)

    def __ge__(self, other):
        return _infix(self, ">=", other)

    def __and__(self, other):
        return _infix(self, "and", other)

    def __or__(self, other):
        return _infix(self, "or", other)

    def __rand__(self, other):
        return _infix(other, "and", self)

    def __ror__(self, other):
        return _infix(other, "or", self)

    # -- the three text tests ------------------------------------------------
    #
    # Methods, for the same reason `is_in` and `is_missing` are: these sit
    # between their operands in the grammar, and Python has no way to add an
    # infix word. The subject is written either way, which is the point.

    def starts(self, value):
        """`col.product.starts("W")`, or `name.starts("q")` for a column's name."""
        return Expr(f"({self._text} starts {_value(value)})")

    def ends(self, value):
        """`col.file.ends(".csv")`, or `name.ends("_id")` for a column's name."""
        return Expr(f"({self._text} ends {_value(value)})")

    def contains(self, value):
        """`col.note.contains("urgent")`, or `name.contains("rev")` for a name."""
        return Expr(f"({self._text} contains {_value(value)})")

    def __invert__(self):
        """`~`, because Python cannot overload `not`.

        `not` is a keyword whose result Python coerces to a bool, so a class
        cannot see it at all. `~` is the only prefix operator available, which
        makes it the spelling rather than a choice between spellings.
        """
        if self._negation is not None:
            return Expr(self._negation, brings=self._brings)
        # The table travels through the negation, because an anti join reads the
        # other table exactly as much as a semi join does.
        return Expr(f"(not {self._text})", brings=self._brings)

    # -- arithmetic ----------------------------------------------------------

    def __add__(self, other):
        return _infix(self, "+", other)

    def __sub__(self, other):
        return _infix(self, "-", other)

    def __mul__(self, other):
        return _infix(self, "*", other)

    def __truediv__(self, other):
        return _infix(self, "/", other)

    def __radd__(self, other):
        return _infix(other, "+", self)

    def __rsub__(self, other):
        return _infix(other, "-", self)

    def __rmul__(self, other):
        return _infix(other, "*", self)

    def __rtruediv__(self, other):
        return _infix(other, "/", self)

    def __neg__(self):
        return Expr(f"-{self._text}")

    # -- the two the grammar spells with words -------------------------------

    def is_in(self, values):
        """`[region] in {"West", "East"}`.

        Python's `in` cannot be reached: it calls `__contains__` on the thing on
        the right and coerces the answer to a bool, so an expression object never
        sees it. A method is the only route, and `{ }` around the values is a set
        literal in Python as well as in the grammar, so the two lines look alike.

        **A set is sorted before it is written, and it has to be.** Python
        randomizes string hashing per process, so iterating the same set literal
        gives a different order on every run. Written out as they came, the same
        pipeline would emit different text each time it was executed, which
        breaks the one promise this project makes. A list or a tuple keeps the
        order it was written in, because that order was chosen by a person.
        """
        items = ", ".join(_value(v) for v in _each(values))
        if not items:
            raise GodExpressionError("a set needs at least one value")
        return Expr(
            f"({self._text} in {{{items}}})",
            negation=f"({self._text} not in {{{items}}})",
        )

    def is_missing(self):
        """`[cost] is missing`, and `~col.cost.is_missing()` for the other way."""
        return Expr(
            f"({self._text} is missing)",
            negation=f"({self._text} is not missing)",
        )

    # An expression is not a value, so it has no meaningful hash. Defining
    # `__eq__` already removed the inherited one; saying so here is for the
    # reader rather than for Python.
    __hash__ = None


class GodError(Exception):
    """A pipeline god refused. The one exception a caller has to know.

    **Every refusal is one of these, wherever it was raised**, and that is what
    lets a reader write one `except` around a pipeline. The grammar's own
    refusals arrive from the engine; a few are raised here instead, before a
    sentence is even built, because a whole table where a column belongs is a
    mistake worth naming at the point it is made rather than letting it reach
    the engine as something stranger.

    It lives in this module rather than beside `run` because this is the lower
    one: a binding-level refusal cannot subclass an error defined above it
    without a cycle. `run` re-exports it, so `from god.run import GodError`
    still means what it always did.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West"], "revenue": [100]})
    >>> products = pd.DataFrame({"product": ["Widget"]})

    >>> try:
    ...     collect(sales >> keep(col.reveune > 1))
    ... except GodError as refusal:
    ...     print(refusal)
    <BLANKLINE>
    illegal: there is no column called `reveune`. Did you mean `revenue`? The table has: region, revenue
      |
    2 |   then keep where ([reveune] > 1)
      |                     ^^^^^^^

    >>> try:
    ...     collect(sales >> pick(products))
    ... except GodError as refusal:
    ...     print(refusal)
    `pick` names a column, and this is a whole table. A column of it is written `col.name`
    """


class GodExpressionError(GodError):
    """Something in an expression the grammar cannot be given.

    **A `GodError`, deliberately.** It was a bare `Exception` until 2026-08-13,
    which meant the `except GodError` every chapter of the manual teaches did
    not catch it: a reader who followed the book and wrote `pick(products)` got
    an uncaught crash rather than a refusal, and could not catch it by name
    either, since this class is not exported. One idea, one exception to catch.
    """


class _Columns:
    """`col`, the one name Python adds to the grammar.

    R writes a bare `revenue` because it can look a name up in the data before
    the scope. Python has no such hook, so a column says it is one. That is the
    second of the two differences between the languages, and the whole of it.

    `col["order date"]` is for a name that is not a Python identifier. The
    grammar's own words `name`, `value` and `kind` are not columns and are
    written bare; they stand inside `where(...)` and are shown there.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> collect(sales >> keep(col.revenue > 110))
      region  revenue
    0   East      120
    1   West      150
    """

    __slots__ = ()

    def __getattr__(self, name: str) -> Expr:
        # Dunder probes, and the single-underscore ones notebook frontends
        # send (`_ipython_canary_...`, `_repr_html_`), have to miss: an Expr
        # answered there convinces a frontend this object is something it is
        # not. A real column whose name starts with one underscore still
        # works.
        if name.startswith("__") or name.startswith(("_ipython_", "_repr_")):
            raise AttributeError(name)
        return Expr(f"[{name}]", column=name)

    def __getitem__(self, name: str) -> Expr:
        """For a column whose name is not a Python identifier: `col["order date"]`."""
        return Expr(f"[{name}]", column=str(name))

    def __repr__(self) -> str:
        return "col: write col.name for a column"


col = _Columns()


# -- writing a value out -----------------------------------------------------


def _infix(left, word: str, right) -> Expr:
    # **One column `is` another is remembered as a pair**, because `join` and
    # `matching` need the two names rather than the finished text: a join key is
    # written `by [customer_id] is [id]` with no parentheses, and reading them
    # back out of `"([customer_id] is [id])"` would be parsing this file's own
    # output. Everywhere else the pair is ignored and this is an ordinary
    # condition.
    pair = None
    if (
        word == "is"
        and isinstance(left, Expr)
        and isinstance(right, Expr)
        and left._column is not None
        and right._column is not None
    ):
        pair = (left._column, right._column)
    return Expr(f"({_operand(left)} {word} {_operand(right)})", pair=pair)


def _operand(value) -> str:
    return value._text if isinstance(value, Expr) else _value(value)


def _value(value) -> str:
    """A Python value, as the grammar writes it."""
    if isinstance(value, Expr):
        return value._text

    # `bool` before `int`, because in Python a bool **is** an int and the wrong
    # order would write `yes` as `1`.
    if isinstance(value, bool):
        return "yes" if value else "no"

    if value is None:
        return "missing"

    if isinstance(value, str):
        # The grammar closes a text value at the first `"` and has no escape, so
        # a value containing one cannot be written at all. Refusing here names
        # the problem; passing it through would end the sentence somewhere the
        # caller did not intend.
        if '"' in value:
            raise GodExpressionError(
                "god cannot yet write a text value containing a double quote, "
                "and will not guess where it ends"
            )
        return f'"{value}"'

    if isinstance(value, (int, float)):
        return _number(value)

    raise GodExpressionError(
        f"god does not know how to write `{value!r}` in a pipeline"
    )


def _number(value) -> str:
    """A number, never in scientific notation.

    Python writes large and small numbers as `1e+05`, and the grammar reads
    digits and at most one point. Formatting is fixed here so the two hosts write
    the same number the same way.
    """
    if isinstance(value, int):
        return str(value)
    if value != value or value in (float("inf"), float("-inf")):
        raise GodExpressionError(f"god cannot write `{value}` as a number")
    written = f"{value:.10f}".rstrip("0")
    return written + "0" if written.endswith(".") else written


def _each(values):
    """The values in a set, in an order that does not change between runs.

    An ordered collection keeps the order it was written in. An unordered one is
    sorted, because Python's per-process hash randomization means a `set` yields
    its members differently on every run, and a pipeline whose text depends on
    which run wrote it is not one pipeline.
    """
    if isinstance(values, (str, bytes)) or not hasattr(values, "__iter__"):
        return [values]
    if isinstance(values, (list, tuple)):
        return list(values)
    try:
        return sorted(values)
    except TypeError:
        # Values that cannot be ordered against each other. Nothing here can make
        # them deterministic, so say so rather than emit a different sentence
        # each run.
        raise GodExpressionError(
            "god cannot put these values in a settled order, and the same "
            "pipeline has to read the same way every time. Write them as a "
            "list: [\"West\", \"East\"]"
        ) from None


def is_frame(table) -> bool:
    """Whether this looks like a table.

    Asked of the object rather than of pandas, so a polars frame or anything
    else shaped like a table is not turned away for having the wrong parentage.

    **A Spark frame has no length and cannot be asked for one**, because counting
    its rows is a job rather than an attribute. So the question is whether it can
    say what its columns are, which every table can, and then whether it can be
    measured *or* describe its own types.

    It lives here rather than beside the verbs because two things need it now:
    a verb deciding whether it was handed a table, and a column position saying
    so when it was.
    """
    if not hasattr(table, "columns"):
        return False
    return hasattr(table, "__len__") or hasattr(table, "dtypes")


def name_of(value, where: str) -> str:
    """The column a name-taking position was given.

    `pick`, `by` and `sort` take columns rather than values, so being handed an
    expression is a mistake worth naming at the point it is made rather than
    letting it arrive at the grammar as something stranger.
    """
    if isinstance(value, Expr) and value._column is not None:
        return value._column
    if isinstance(value, str):
        return value
    # **What it was handed is named rather than printed**, and the case that
    # forced this is a whole table: `repr` of a DataFrame is the frame, so the
    # message arrived with rows and columns in the middle of it and the reader
    # had to find the sentence around them. Every verb that takes columns could
    # produce it.
    if is_frame(value):
        raise GodExpressionError(
            f"`{where}` names a column, and this is a whole table. "
            f"A column of it is written `col.name`"
        )
    if isinstance(value, (list, tuple, set)):
        raise GodExpressionError(
            f"`{where}` takes its columns one at a time rather than in a list: "
            f"`{where}(col.a, col.b)`"
        )
    if isinstance(value, Expr):
        raise GodExpressionError(
            f"`{where}` names a column, and `{value._text}` is a computed value. "
            f"Make it a column first with `add`, then name it here"
        )
    # Anything else is small enough to show, and shown with its kind, because
    # `3.14` alone does not say why it is wrong.
    written = repr(value)
    if len(written) > 60:
        written = written[:57] + "..."
    raise GodExpressionError(
        f"`{where}` names a column, and `{written}` is "
        f"{type(value).__name__}, not a column name"
    )
