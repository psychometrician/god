"""The book's example tables, fetched by name.

Not a word of the grammar, and deliberately so: this is the same category as
``run`` and ``god_sql``, something the binding offers and the vocabulary does
not. It exists because every example in the manual begins with a table, and a
reader who wants to run one should not have to retype fifteen rows first.

The tables are not shipped in the package. They are published beside the
book, so one copy serves both languages and nothing goes stale inside a
wheel. The sibling project keeps its own tables the same way, under its own
helper's name, and the two names differ on purpose: both packages are meant
to be loaded together, and one of them masking the other's tables would be a
collision the pair exists to avoid.

**A local ``data/`` wins, and that is what makes this usable by the manual that
needs it.** Reading only from the network meant a new table did not resolve
until the book was published, and a render calling this thirty times needed a
connection to build a page about a grammar that has nothing to do with one. So
the walk-up comes first and the published copy is the fallback, which is the
same order and the same reason as the engine's own resolution.
"""

import re
from pathlib import Path

import pandas as pd

from .run import GodError

GOD_BOOK_DATA_URL = "https://psychometrician.github.io/god-book/data/"

_NAME = re.compile(r"^[A-Za-z0-9_-]+$")

__all__ = ["god_table"]


def _walk_up_data(start: Path, name: str) -> Path | None:
    """``data/<name>.csv``, in this directory or any above it.

    Deliberately not ``book/data/``: a package that knows the manual's
    directory layout is a package with a second job. A reader keeping their own
    copies puts them in ``data/`` and gets the same behaviour the book does.
    """
    for directory in (start, *start.parents):
        candidate = directory / "data" / f"{name}.csv"
        if candidate.exists():
            return candidate
    return None


def god_table(name, text=()):
    """Read one of the book's example tables.

    Returns a table ready to pipe. A ``data/<name>.csv`` in the working
    directory or any directory above it is read first; failing that, the copy
    published beside the manual is fetched. The cast is declared in the book's
    preface: ``sales``, ``products``, ``survey``, ``answers``, ``marks``,
    ``messy``, ``diary`` and ``gapminder``.

    Args:
        name: The table's name without the extension, such as ``"sales"``.
        text: Columns that must stay text. A CSV records what a value is and
            never what kind of thing it is, so a column of ``01``, ``02``,
            ``03`` comes back as the numbers 1, 2, 3 unless it is named here.

    Returns:
        A pandas DataFrame.

    Examples
    --------
    **This one may reach the network**, which is why it is shown rather than
    run: the suites stay offline, and an example that needs a connection fails
    on a train rather than reporting a defect. With a ``data/sales.csv`` beside
    it or above it, no connection is used at all.

    >>> import god                                          # doctest: +SKIP
    >>> sales = god.god_table("sales")                      # doctest: +SKIP
    >>> len(sales), list(sales.columns)                     # doctest: +SKIP
    (15, ['date', 'region', 'product', 'quantity', 'revenue', 'cost'])

    A column of leading-zero codes comes back as numbers unless it is named,
    because a CSV records what a value is and never what kind of thing it is:

    >>> god.god_table("survey", text=["respondent"])        # doctest: +SKIP
    """
    if not isinstance(name, str):
        raise GodError(
            "god: god_table() takes one table name, as in "
            'god_table("sales"). The cast is declared in the book\'s preface.'
        )
    # A name is a name, not a path. This mattered less when the only thing a
    # name could do was make a bad URL; now that it also names a file on disk, a
    # `/` or a `..` would reach outside `data/` entirely.
    if not _NAME.match(name):
        raise GodError(
            f"god: {name!r} is not a table name. A name is letters, digits, "
            '`_` and `-`, as in god_table("sales"). It is not a path and not '
            "a file name."
        )
    local = _walk_up_data(Path.cwd(), name)
    source = str(local) if local else GOD_BOOK_DATA_URL + name + ".csv"
    return pd.read_csv(source, dtype={column: str for column in text})
