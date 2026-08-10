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
"""

import pandas as pd

from .run import GodError

GOD_BOOK_DATA_URL = "https://psychometrician.github.io/god-book/data/"

__all__ = ["god_table"]


def god_table(name, text=()):
    """Read one of the book's example tables.

    Fetches a table published beside the manual and returns it ready to pipe.
    The cast is declared in the book's preface: ``sales``, ``products``,
    ``survey``, ``answers``, ``marks``, ``messy``, ``diary`` and
    ``gapminder``.

    Args:
        name: The table's name without the extension, such as ``"sales"``.
        text: Columns that must stay text. A CSV records what a value is and
            never what kind of thing it is, so a column of ``01``, ``02``,
            ``03`` comes back as the numbers 1, 2, 3 unless it is named here.

    Returns:
        A pandas DataFrame.
    """
    if not isinstance(name, str):
        raise GodError(
            "god: god_table() takes one table name, as in "
            'god_table("sales"). The cast is declared in the book\'s preface.'
        )
    return pd.read_csv(
        GOD_BOOK_DATA_URL + name + ".csv",
        dtype={column: str for column in text},
    )
