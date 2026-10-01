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

**And the file is read by DuckDB, with options the engine pins**, in both
languages. pandas' ``read_csv`` and R's ``read.csv`` made one file into two
different tables, in a dozen small ways the engine's ``read_csv_query`` lists,
so the reader is now the one both bindings already run every query on, and
the query that reads the file comes from the engine like every other query.
"""

import os
import re
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

from .run import GodError, _call, _connection

GOD_BOOK_DATA_URL = "https://psychometrician.github.io/god-book/data/"

# How long a fetch of the published copy may wait, in seconds. R's download
# gives up after `getOption("timeout")`, 60 unless changed, and this waits the
# same. With no limit, a connection that accepts and never answers held the
# caller's session forever.
_TIMEOUT = 60

# Matched whole, with `fullmatch`. A `$` also matches before a final newline,
# so `"sales\n"` passed the check and failed later inside urllib, in its words
# rather than god's.
_NAME = re.compile(r"[A-Za-z0-9_-]+")

__all__ = ["god_table"]


class GodTableError(GodError):
    """A table that could not be found or read.

    **A ``GodError``, so one ``except`` still catches everything god names**,
    and a class of its own, so the book's refusal guard can tell "this page
    never got its table" from "the grammar said no". Before ``god_table``
    spoke in god's words, its failures were urllib's and could not be
    mistaken for a refusal; once they were god's, they could.
    """


def _walk_up_data(start: Path, name: str) -> Path | None:
    """``data/<name>.csv``, in this directory or any above it.

    Deliberately not ``book/data/``: a package that knows the manual's
    directory layout is a package with a second job. A reader keeping their own
    copies puts them in ``data/`` and gets the same behaviour the book does.

    **The name as written, letter for letter.** ``Path.exists`` follows the
    filesystem, which ignores case on a Mac and on Windows and not on Linux, so
    with only ``data/Sales.csv`` beside it, ``god_table("sales")`` read that
    file on one machine and fetched the book's ``sales`` on another. The
    listing is compared exactly, so the answer is the same everywhere.
    """
    file = f"{name}.csv"
    for directory in (start, *start.parents):
        folder = directory / "data"
        if folder.is_dir() and file in os.listdir(folder):
            return folder / file
    return None


def _absent(name: str, unpublished: bool) -> str:
    """The same words R's half gives, for the two ways a table is not there."""
    if unpublished:
        return (
            f"god: there is no table called `{name}`. No `data/{name}.csv` was "
            "found here or in a folder above, and the book publishes no table by "
            f"that name. Check the name, or put your own `{name}.csv` in a "
            "`data/` folder."
        )
    return (
        f"god: no `data/{name}.csv` was found here or in a folder above, and the "
        "copy published beside the book could not be fetched. Put "
        f"`{name}.csv` in a `data/` folder to read it without a connection."
    )


def _fetch(name: str) -> Path:
    """The published copy, fetched to a temporary file within ``_TIMEOUT``."""
    try:
        with urllib.request.urlopen(GOD_BOOK_DATA_URL + name + ".csv",
                                    timeout=_TIMEOUT) as response:
            data = response.read()
    except urllib.error.HTTPError as e:
        raise GodTableError(_absent(name, unpublished=e.code == 404)) from None
    except (OSError, ValueError):
        # A refused or stalled connection, a timeout, or a host that does not
        # resolve: the copy could not be fetched, whatever the reason.
        raise GodTableError(_absent(name, unpublished=False)) from None
    handle, path = tempfile.mkstemp(suffix=".csv")
    with os.fdopen(handle, "wb") as out:
        out.write(data)
    return Path(path)


def _read_csv(name: str, path: Path, columns: list):
    """The file, read on the binding's own DuckDB with the engine's query."""
    import duckdb

    con = _connection()

    def query(keep):
        args = ["--read-csv", str(path)]
        for column in keep:
            args += ["--text", column]
        return _call(args, "")

    def frame(sql):
        try:
            return con.execute(sql).df()
        except duckdb.Error as e:
            raise GodTableError(
                f"god: `{name}` could not be read as a table. Every row of a CSV "
                "needs as many fields as its header, and a value holding a comma "
                "needs quotes around it. DuckDB said: "
                + str(e).strip().splitlines()[0].strip()
            ) from None

    # **A `text` column the file does not have is refused**, in the same words
    # as R's. A misspelt name would otherwise mean the column the caller wanted
    # kept as text was not, and the two readers this replaced disagreed even
    # about that: pandas said nothing, and R warned.
    if columns:
        have = list(frame(f"SELECT * FROM ({query([])}) LIMIT 0").columns)
        absent = [column for column in columns if column not in have]
        if absent:
            raise GodError(
                f"god: `{name}` has no column called `{absent[0]}` to keep as text. "
                f"The table has: {', '.join(map(str, have))}"
            )
    return frame(query(columns))


def god_table(name, text=()):
    """Read one of the book's example tables.

    Returns a table ready to pipe. A ``data/<name>.csv`` in the working
    directory or any directory above it is read first; failing that, the copy
    published beside the manual is fetched. So it needs no connection once you
    have a ``data/`` folder, and **your own tables work the same way**: put
    ``invoices.csv`` in ``data/`` and ``god_table("invoices")`` reads it.

    Every table the book uses is published beside it, the cast the preface
    declares and the smaller ones a single chapter needs. A name is letters,
    digits, ``_`` and ``-``; it is not a path, and a ``/`` or a ``..`` is
    refused rather than followed. The file's name has to match the name given,
    letter for letter, on every operating system.

    The file is read the same way in Python and in R, so one file is one table
    in either: a column name is kept exactly as the header writes it, an empty
    field and ``NA`` are missing, a date stays the text it was written as until
    ``to_date`` reads it, and a row with the wrong number of fields is refused
    rather than guessed at.

    Args:
        name: The table's name without the extension, such as ``"sales"``.
        text: Columns that must stay text, one name or a list of them. A CSV
            records what a value is and never what kind of thing it is, so a
            column of ``01``, ``02``, ``03`` can come back as the numbers 1, 2,
            3 unless it is named here. A name the file does not have is
            refused.

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

    A column of leading-zero codes is kept as text by naming it:

    >>> god.god_table("survey", text="respondent")          # doctest: +SKIP
    """
    if not isinstance(name, str):
        raise GodError(
            "god: `god_table()` takes one table name, as in "
            '`god_table("sales")`. The cast is declared in the book\'s preface.'
        )
    # A name is a name, not a path. This mattered less when the only thing a
    # name could do was make a bad URL; now that it also names a file on disk, a
    # `/` or a `..` would reach outside `data/` entirely.
    if not _NAME.fullmatch(name):
        raise GodError(
            f"god: `{name}` is not a table name. A name is letters, digits, "
            '`_` and `-`, as in `god_table("sales")`. It is not a path and not '
            "a file name."
        )
    # **A bare string is one column**, as it is in R. Iterated as it came, it
    # named each letter a column, so `text="code"` kept nothing as text.
    columns = [text] if isinstance(text, str) else list(text)
    path = _walk_up_data(Path.cwd(), name)
    fetched = None
    if path is None:
        fetched = path = _fetch(name)
    try:
        return _read_csv(name, path, columns)
    finally:
        if fetched is not None:
            os.unlink(fetched)
