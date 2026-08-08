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

__all__ = ["run", "show_as", "god_sql", "GodError"]


class GodError(Exception):
    """A pipeline the grammar refused.

    The message is the grammar's own, already rendered with its caret. Wrapping
    it in "god-cli failed (exit 2)" would replace something written for a person
    with something written for a program.
    """


def run(pipeline: str, **tables):
    """Run a pipeline and return a frame.

    The table named at the head of the pipeline is looked up where you are
    calling from, the way ``duckdb.sql("SELECT * FROM df")`` finds ``df``. Pass
    one by name when it is not in scope.
    """
    # A pipeline can name more than one table, which is what `join` brought. The
    # grammar says which, in the order it names them, and the first is the head.
    sources = _needs(pipeline)

    for source in sources:
        if source not in tables:
            found = _look_up(source)
            if found is None:
                found = _in_catalog(source)
            if found is None:
                raise GodError(
                    f"the pipeline reads a table called `{source}`, and there is no "
                    f"such table here.\n  Pass it by name: run(pipeline, {source}=your_data)"
                )
            tables[source] = found

    return _query(pipeline, tables, sources[0])


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
        connection.register(name, frame)
    try:
        return connection.sql(sql).df()
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
    """
    # A pipeline built from the native verbs already carries its table and knows
    # what it is called, so there is nothing to look up.
    from .verbs import Pipeline

    if isinstance(pipeline, Pipeline):
        return _Written(_call(
            _columns_args(pipeline.tables, pipeline.source) + ["--as", as_],
            pipeline.written(),
        ))

    source = _needs(pipeline)[0]
    frame = tables.get(source)
    if frame is None:
        frame = _look_up(source)
    if frame is None:
        raise GodError(
            f"the pipeline reads a table called `{source}`, and there is no such table here"
        )
    return _Written(_call(["--columns", _columns_of(frame), "--as", as_], pipeline))


def god_sql(pipeline: str, columns: str) -> str:
    """The query a pipeline becomes."""
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
    bundled = Path(__file__).parent / "bin" / _EXE
    if bundled.exists():
        return str(bundled)

    # Running from the source tree, which is how this is used before there is an
    # installed package to bundle a binary into.
    named = os.environ.get("GOD_CLI", "")
    if named and Path(named).exists():
        return named

    found = shutil.which(_EXE)
    if found:
        return found

    # Where ``cargo build --release`` actually puts it. Looked for **because the
    # message below tells the reader to run that command**, and a message that
    # names a fix the code then ignores is worse than no message: you do the
    # thing it asked for, nothing changes, and the tool looks broken rather than
    # unconfigured.
    built = _built_binary()
    if built:
        return built

    raise GodError(
        "the god engine was not found. Build it with `cargo build --release`, "
        "or point GOD_CLI at it"
    )


def _built_binary() -> str | None:
    """Walk up looking for ``target/release/god-cli``.

    From the working directory and from this file both, because the two differ:
    a session run from the repository root finds it by the first, and one run
    from anywhere else finds it by the second.
    """
    starts = [Path.cwd(), Path(__file__).resolve().parent]
    for start in starts:
        for directory in (start, *start.parents):
            candidate = directory / "target" / "release" / _EXE
            if candidate.exists():
                return str(candidate)
    return None


# -- finding the table ------------------------------------------------------


def _look_up(name: str):
    """Find a frame in the caller's scope.

    Two frames up: past this function and past the ``run`` that called it. Locals
    first and then globals, which is the order Python itself resolves a name.
    """
    frame = inspect.currentframe()
    try:
        caller = frame.f_back.f_back
        if caller is None:
            return None
        return caller.f_locals.get(name, caller.f_globals.get(name))
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
