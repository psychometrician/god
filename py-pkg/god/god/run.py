"""The Python launcher.

**This module carries bytes and decides nothing.** Validation, defaults, coercion
and every message belong to the grammar; a rule implemented here is a rule R
would get wrong, and then the two languages disagree about what a sentence means.
What is left is: find the table, describe it, hand the text over, run what comes
back.

It does not read the pipeline either. Picking the table's name out of the text
would be parsing — in a host, and a second time — so the grammar is asked instead
(``--needs``). The temptation to do it here with a regular expression is exactly
how two implementations start to differ, and it would look harmless in review.

Everything here mirrors the R launcher deliberately. Where the two could each
make a reasonable choice, they make the same one, because the point of the
project is that they agree.
"""

from __future__ import annotations

import inspect
import os
import shutil
import subprocess
import sys
from pathlib import Path

__all__ = ["run", "show_as", "show_steps", "god_sql", "GodError"]


# **Re-exported rather than defined here**, so that the binding's own refusals
# can be `GodError`s too. `columns.py` is the lower module and cannot import
# this one; the class moved down and this name keeps working.
from .columns import GodError  # noqa: F401  (re-exported for callers)


def run(pipeline: str, **tables):
    """Run a pipeline and return a frame.

    The table named at the head of the pipeline is looked up where you are
    calling from, the way ``duckdb.sql("SELECT * FROM df")`` finds ``df``. Pass
    one by name when it is not in scope.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> run('sales then keep where [region] is "West"', sales=sales)
      region  revenue
    0   West      100
    1   West      150
    """
    # A pipeline can name more than one table, which is what `join` brought. The
    # grammar says which, in the order it names them, and the first is the head.
    sources = _needs(pipeline)

    for source in sources:
        if source not in tables:
            found = _look_up(source, _asked_from())
            if found is None:
                found = _in_catalog(source)
            if found is None:
                raise GodError(
                    f"the pipeline reads a table called `{source}`, and there is no "
                    f"such table here.\n  Pass it by name: run(pipeline, {source}=your_data)"
                )
            tables[source] = found

    return _query(pipeline, tables, sources[0])


def _scannable(frame):
    """The frame, in the form the engine scans fastest.

    The engine reads a registered pandas frame through a hook that re-analyzes
    it on every query, which prices at seconds once a frame holds tens of
    millions of rows. The same rows as an Arrow table register in milliseconds
    and scan a third faster, so a pandas frame is converted on its way in.
    Measured on twenty million taxi rows rather than assumed: 1.7 seconds of
    registration became 0.05, and the whole pipeline went from 2.6 seconds to
    under one. `preserve_index=False` because the engine's pandas hook never
    saw the index either, and a column appearing from nowhere would change
    what a sentence means. A frame Arrow cannot convert — a column of mixed
    junk — keeps the old road, because slower is not wrong.
    """
    try:
        import pandas
        import pyarrow

        if isinstance(frame, pandas.DataFrame):
            return pyarrow.Table.from_pandas(frame, preserve_index=False)
    except Exception:
        pass
    return frame


def _query(pipeline: str, tables: dict, source: str):
    """Turn a pipeline into a query, run it, and hand back the rows.

    Shared by ``run`` and by the native verbs, which differ only in where the
    text came from: a string the caller wrote, or a sentence the verbs built. By
    the time either arrives here they are the same thing, and they had better be,
    because a second execution path is a second set of answers.
    """
    session = _spark_session(tables)
    if session is not None:
        return _on_spark(session, pipeline, tables, source)

    sql = _call(_columns_args(tables, source), pipeline)

    connection = _connection()
    for name, frame in tables.items():
        connection.register(name, _scannable(frame))
    try:
        return connection.sql(sql).df()
    except Exception as e:
        # One refusal fires from inside the query rather than before it —
        # `widen` on a name that appears twice — so it arrives as the driver's
        # error with the grammar's words inside. It leaves here as a
        # `GodError` like every other refusal: a caller gets one exception
        # surface, not a tour of the drivers underneath.
        raise GodError(str(e)) from None
    finally:
        # A name in one pipeline must not be findable by the next. A connection
        # that quietly remembers is one where a typo resolves to last week's data.
        for name in tables:
            try:
                connection.unregister(name)
            except Exception:
                pass


def _is_spark(frame) -> bool:
    """Whether this is a Spark frame, asked without importing pyspark.

    Importing it to find out would start a JVM for anyone who merely has it
    installed, so the question is asked of the object instead.
    """
    return type(frame).__module__.startswith("pyspark.")


def _spark_session(tables: dict):
    """The session to run on, or `None` for the engine on this machine.

    **The engine follows the data.** A pipeline over Spark frames runs on Spark
    and a pipeline over pandas frames runs here, so nothing has to be configured
    and no word is added to say which. The session comes off the frame itself,
    which is also what keeps a pipeline from spanning two of them.
    """
    sessions = [f.sparkSession for f in tables.values() if _is_spark(f)]
    if not sessions:
        return None
    if len(sessions) < len(tables):
        raise GodError(
            "this pipeline mixes tables from Spark with tables from here, and one "
            "query cannot read both.\n  Bring them to the same place first"
        )
    return sessions[0]


def _on_spark(session, pipeline: str, tables: dict, source: str):
    """Run a pipeline on Spark and hand back a Spark frame.

    **The answer comes back in the same currency as the question.** Give this
    pandas frames and it gives a pandas frame; give it Spark frames and it gives
    a Spark frame, still on the cluster. Materializing the answer of a pipeline
    over a warehouse table would be a way to take a driver down, and `collect` in
    Spark's own vocabulary already means that, so the word is left alone and the
    result is left where it is.
    """
    sql = _call(_columns_args(tables, source) + ["--as", "spark"], pipeline)

    # A name in parts is already a table the catalog knows, so it is left for
    # Spark to resolve. Only a frame the caller is holding needs a name.
    temporary = [name for name in tables if "." not in name]
    for name in temporary:
        tables[name].createOrReplaceTempView(name)
    try:
        return session.sql(sql)
    finally:
        for name in temporary:
            try:
                session.catalog.dropTempView(name)
            except Exception:
                pass


class _Written(str):
    """A rendering that shows itself the way it will be read.

    An ordinary `str` echoes at a prompt with its quotes and its `\\n` escapes
    showing, which is the wrong picture of a query someone asked to look at. R's
    `show_as` prints the text and returns it invisibly; Python has no invisible
    return, so the same effect is a string that reprs as itself.

    It is a `str`, so every string method still works on it.
    """

    __slots__ = ()

    def __repr__(self) -> str:
        return str(self)


def show_as(pipeline: str, as_: str = "dplyr", **tables) -> _Written:
    """The same pipeline, written in a language you already know.

    A small vocabulary covers most of what people do and never all of it, so the
    question is not whether you reach its edge but what happens when you do.

    ``as_`` is ``"sql"``, ``"spark"``, ``"dplyr"``, ``"pandas"``, ``"polars"``,
    ``"pyspark"``, or ``"god"`` itself. An unknown name is refused and the
    message lists the real ones, so this list going stale costs nothing.

    **This returns rather than prints.** A notebook and a prompt both show what
    an expression evaluates to, so printing as well would show it twice.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> show_as(sales >> keep(col.region == "West"), "dplyr")
    sales |>
      filter((region == "West"))
    """
    args, sentence = _asking(pipeline, tables, _asked_from())
    return _Written(_call(args + ["--as", as_], sentence))


class _Steps:
    """What a pipeline does to the table, drawn.

    **Two ways to look at one drawing, and the reader's surroundings pick.** A
    prompt gets the ladder, because that is what the rest of a session looks
    like; a notebook or a rendered page gets the picture, because a page can hold
    one. Neither is an argument anybody has to pass.

    ``.text`` and ``.svg`` reach either one outright, for writing to a file.
    """

    __slots__ = ("_args", "_sentence", "_drawn")

    def __init__(self, args: list[str], sentence: str):
        self._args = args
        self._sentence = sentence
        # Drawn when it is asked for, and once. Somebody who only ever looks at
        # the ladder should not pay for a picture nobody sees.
        self._drawn: dict[str, str] = {}

    def _draw(self, way: str) -> str:
        if way not in self._drawn:
            self._drawn[way] = _call(self._args + ["--draw", way], self._sentence)
        return self._drawn[way]

    @property
    def text(self) -> str:
        """The ladder, as one string."""
        return self._draw("text")

    @property
    def svg(self) -> str:
        """The picture, as one string. It carries its own stylesheet."""
        return self._draw("svg")

    def __repr__(self) -> str:
        # The drawing ends in a newline, as a file should. A prompt adds one of
        # its own, so showing it here would leave a blank line under every
        # ladder.
        return self.text.rstrip("\n")

    def _repr_html_(self) -> str:
        # Jupyter and Quarto both look for this before falling back to ``repr``.
        # The picture is markup and reaches the page as markup; at an ordinary
        # prompt nothing looks for it and the ladder still answers.
        return self.svg


def show_steps(pipeline, **tables) -> _Steps:
    """What a pipeline does to the table, step by step.

    **Nothing runs.** The grammar checks the whole sentence against the columns
    before anything is executed, so this is a picture of what would happen, drawn
    from the same reading that would refuse a column that is not there.

    Every step shows the table as it stands once that step has run, with the
    columns it makes marked and the ones it takes away marked where they leave. A
    second table gets a row of its own under the step that reads it, so a join
    shows what crossed over and what matched. A sentence the grammar refuses is
    still drawn, as far as it checked, with the refusal under the words that
    stopped it — which is the question an error message on its own cannot answer.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> show_steps(sales >> keep(col.region == "West") >> take(1))
    sales                              region:text  revenue:number
    ├ keep where ([region] is "West")  region  revenue
    └ take 1                           region  revenue
        at most 1 rows
    """
    args, sentence = _asking(pipeline, tables, _asked_from())
    return _Steps(args, sentence)


def _asking(pipeline, tables: dict, caller) -> tuple[list[str], str]:
    """Which tables does this pipeline read, and how are they described?

    **Shared by everything that asks the grammar about a pipeline rather than
    running it.** The callers used to carry a copy each, and a copy of a lookup is
    how one of them ends up resolving a table the other does not.
    """
    # A pipeline built from the native verbs already carries its tables and
    # knows what they are called, so there is nothing to look up.
    from .verbs import Pipeline

    if isinstance(pipeline, Pipeline):
        return _columns_args(pipeline.tables, pipeline.source), pipeline.written()

    # The same lookup ``run`` does, and for the same reason: since ``join``, a
    # sentence can name more than one table, so every name the grammar reports
    # is resolved rather than only the head.
    sources = _needs(pipeline)
    for source in sources:
        if source not in tables:
            found = _look_up(source, caller)
            if found is None:
                found = _in_catalog(source)
            if found is None:
                raise GodError(
                    f"the pipeline reads a table called `{source}`, and there is no such table here"
                )
            tables[source] = found
    return _columns_args(tables, sources[0]), pipeline


def god_sql(pipeline: str, columns: str) -> str:
    """The query a pipeline becomes.

    Examples
    --------
    >>> import pandas as pd
    >>> from god import *
    >>> sales = pd.DataFrame({"region": ["West", "East", "West"],
    ...                       "revenue": [100, 120, 150]})
    >>> print(god_sql("sales then take 1", "region:text,revenue:number"))
    WITH step0 AS (SELECT * FROM "sales"),
         step1 AS (SELECT * FROM step0 LIMIT 1)
    SELECT * FROM step1
    """
    return _call(["--columns", columns], pipeline)


# -- talking to the grammar -------------------------------------------------


def _needs(pipeline: str) -> list[str]:
    """Which tables does this pipeline read? Asked rather than worked out."""
    return [line.strip() for line in _call(["--needs"], pipeline).splitlines() if line.strip()]


def _columns_args(tables: dict, source: str) -> list[str]:
    """How the tables are described to the grammar.

    The head table's columns go in bare, and any other table names itself first.
    One flag with two shapes rather than two flags, because the second shape only
    exists for ``join`` and a pipeline without one should not have to know about
    it.
    """
    args = ["--columns", _columns_of(tables[source])]
    for name, frame in tables.items():
        if name != source:
            args += ["--columns", f"{name}={_columns_of(frame)}"]
    return args


def _call(args: list[str], pipeline: str) -> str:
    """The one place a process is started."""
    result = subprocess.run(
        [_binary(), *args],
        input=pipeline,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise GodError("\n" + result.stderr.rstrip())
    # An assumption is not a failure and never stops anything, but it is never
    # silent either.
    if result.stderr.strip():
        print(result.stderr.rstrip())
    return result.stdout


# What the engine is called on this machine.
#
# Windows is the only platform that spells it differently, and it spells it
# differently in three places at once: the copy ``setup.py`` packs into the
# wheel, the file a walk-up finds in ``target/release/``, and anything already on
# the PATH. So the name is decided once here rather than written out at each
# site. A per-site copy is how a Windows wheel installs perfectly and then cannot
# find the engine it just installed.
_EXE = "god-cli.exe" if os.name == "nt" else "god-cli"


def _binary() -> str:
    # The order is the contract, and R resolves in the same order. An explicit
    # ``GOD_CLI`` always wins. A source tree's own build outranks the bundled
    # copy, because the copy ``setup.py`` packs during a wheel build lingers
    # beside the source afterward, exactly as old as the last wheel — bundled
    # first is how a harness spends a day testing last week's engine. The
    # bundled engine is the installed package's answer; the working directory's
    # tree and the PATH come last, because neither has a reason to match this
    # copy of the binding. The walk-ups exist **because the message below names
    # `cargo build --release`**, and a message that names a fix the code then
    # ignores is worse than no message.
    named = os.environ.get("GOD_CLI", "")
    if named and Path(named).exists():
        return named

    beside_source = _walk_up(Path(__file__).resolve().parent)
    if beside_source:
        return beside_source

    bundled = Path(__file__).parent / "bin" / _EXE
    if bundled.exists():
        return str(bundled)

    beside_cwd = _walk_up(Path.cwd())
    if beside_cwd:
        return beside_cwd

    found = shutil.which(_EXE)
    if found:
        return found

    raise GodError(
        "the god engine was not found. Build it with `cargo build --release`, "
        "or point GOD_CLI at it"
    )


def _walk_up(start: Path) -> str | None:
    """``target/release/god-cli``, in this directory or any above it."""
    for directory in (start, *start.parents):
        candidate = directory / "target" / "release" / _EXE
        if candidate.exists():
            return str(candidate)
    return None


# -- finding the table ------------------------------------------------------


def _look_up(name: str, caller):
    """Find a frame in the scope that asked for it.

    **The frame is handed in rather than counted back to**, and that is not
    fussiness. This used to walk a fixed two frames — past itself and past the
    ``run`` that called it — which is correct exactly as long as nobody puts a
    function in between. Factoring the table lookup out so two callers could
    share it did put one in between, and every name went unfound. Passing the
    frame is how R's side has always done it, and it cannot come apart the same
    way.

    Locals first and then globals, which is the order Python itself resolves a
    name.
    """
    if caller is None:
        return None
    return caller.f_locals.get(name, caller.f_globals.get(name))


def _asked_from():
    """The frame that called the function calling this."""
    frame = inspect.currentframe()
    try:
        return frame.f_back.f_back
    finally:
        del frame


def _in_catalog(name: str):
    """A table the Spark session knows about, where one is running.

    **A name in parts is a table in a catalog and never a local variable**, so
    looking in the caller's scope for `main.sales.orders` was always going to
    fail. Where a session is up, it is asked. Where none is, this returns
    nothing and the caller reports the missing table as it always did.

    Nothing is imported to find out. `getActiveSession` on a pyspark that was
    never started would build one, and the cost of that is a JVM.
    """
    if "." not in name:
        return None
    pyspark = sys.modules.get("pyspark.sql")
    if pyspark is None:
        return None
    try:
        session = pyspark.SparkSession.getActiveSession()
        if session is None:
            return None
        return session.table(name)
    except Exception:
        return None


# -- describing a table -----------------------------------------------------


def _columns_of(frame) -> str:
    """A frame's columns, in the grammar's words.

    **This is the one thing the launcher knows that the grammar does not**: what
    the host calls a column's type. The mapping is deliberately coarse, because
    the grammar draws only the distinctions that change whether a sentence is
    legal, and a type it has no opinion about passes every test rather than
    failing them.
    """
    pairs = []
    for name, kind in zip(_names(frame), _kinds(frame)):
        pairs.append(f"{name}:{kind}")
    if not pairs:
        raise GodError("that table has no columns")
    return ",".join(pairs)


def _names(frame) -> list[str]:
    return [str(c) for c in frame.columns]


def _kinds(frame) -> list[str]:
    # Spark says what a column holds as a word rather than as a dtype object,
    # and asking it the pandas way gives a Column with no `dtype` at all. That
    # would answer `unknown` for every column, which agrees with every rule and
    # so turns off the type checking rather than failing.
    if _is_spark(frame):
        return [_god_type_named(kind) for _, kind in frame.dtypes]
    kinds = []
    for column in frame.columns:
        kinds.append(_god_type(frame[column]))
    return kinds


def _god_type_named(kind: str) -> str:
    """A type the host names in a word, in the grammar's words."""
    kind = kind.lower()
    if kind.startswith("date") or kind.startswith("timestamp"):
        return "date"
    if kind.startswith("bool"):
        return "truth"
    if any(kind.startswith(k) for k in ("int", "long", "short", "byte", "float",
                                        "double", "decimal", "bigint", "smallint",
                                        "tinyint")):
        return "number"
    if kind.startswith("string") or kind.startswith("varchar") or kind.startswith("char"):
        return "text"
    return "unknown"


def _god_type(column) -> str:
    kind = str(getattr(column, "dtype", "")).lower()
    if "datetime" in kind or "date" in kind or "timestamp" in kind:
        return "date"
    if "bool" in kind:
        return "truth"
    if any(k in kind for k in ("int", "float", "decimal", "double")):
        return "number"
    if any(k in kind for k in ("str", "object", "category", "utf8")):
        return "text"
    return "unknown"


# -- one connection, reused -------------------------------------------------

_state: dict = {}


def _connection():
    """One connection, reused.

    Opening one per pipeline is real work for a query that takes a millisecond.
    The engine holds no state between pipelines — the tables are registered and
    unregistered around each one — so there is nothing for a shared connection to
    leak.
    """
    import duckdb

    if "connection" not in _state:
        _state["connection"] = duckdb.connect()
    return _state["connection"]
