"""The printed code, run, and compared against what the sentence means.

**The printing backends are for reading, so nothing normally executes them, and
that is exactly the hole this fills.** A rendering can read perfectly and mean
something else. `pl.when(...).then("big")` is the word "big" to anyone reading
it and is the *column* called `big` to polars, so the printed pipeline either
fails naming a column nobody wrote or, where such a column exists, returns the
wrong answer in silence.

It is the same finding the Spark harness produced one level down: a check that
asks "did it run" passes while the answer is wrong. So this runs the printed
code and compares the table, never the text.

Five real defects came out of the first run of this file, and every one of them
looked correct on the page:

  * `pl.when(...).then("big")` reads a column rather than a value.
  * The second arm of a polars conditional was written `.pl.when(...)`.
  * `F.coalesce(F.col("cost"), 0)` raises: Spark wants a `Column`.
  * `F.split_part(col, "d", 1)` raises for the same reason.
  * Three steps that reorder rows did not restate the order afterwards, so the
    printed code returned different rows from the sentence.

Each target is optional and skipped cleanly when its library is missing, the way
the Spark harness is. **A skip is not a pass and it does not print as one.**

**dplyr joined on 2026-08-19 and was the last printing backend read rather than
run.** It needs R, so it does not fit the scope-and-`eval` shape the three
Python targets share; it gets a driver of its own below. Its first run found
three corpus sentences whose answer was not determined — `drop_duplicates` and
`lengthen` reorder rows, DuckDB's order differs from dplyr's, and a `take` after
either was picking different rows on different backends. The three now declare a
`sort`, which is the rule the grammar already states: any order worth relying on
is a sort away.

**The corpus runs twice since 2026-10-01: on the plain fixture, and on
`parity/holes/`, the same tables with a hole in every column.** The plain one
has no missing value anywhere, so every sentence agreed for the reason that
does not count, and the 2026-09-30 code review found printed pandas dropping a
missing group, three printers totalling nothing to 0, two joining a missing
key to a missing key, and more, sixty disagreements on the first run of the
holed copy. A sentence the query refuses on that data, a `widen` with a
missing name, is reported as refused and not compared: printed code does not
carry the grammar's refusals.

**Three things this compared loosely, and each hid a defect.** Text that
looked like a number was read as one, so `"100.0"` and `"100"` agreed; it is
compared as text now. R wrote a missing value and an empty text alike, as an
empty cell; it writes a missing value as a marker of its own now, and says
which of its columns are text so a CSV's guessing cannot turn `"100"` back into
a number. And rows were compared as a set, which hid printed dplyr handing back
groups in the order it first saw them. **Order is compared wherever the query
settles it**, measured rather than declared: the query is run again on the
fixture's rows shuffled, and if its answer comes back in the same order every
time, the order belongs to the sentence and the printing has to match it.

    python3 parity/printed.py
"""

import io
import subprocess
import tempfile
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "py-pkg" / "god"))

CORPUS = Path(__file__).parent / "corpus.god"
# **Two fixtures, the same three tables each.** The second is the first with a
# hole in every column, and in both partner tables a row whose key is missing.
FIXTURES = [Path(__file__).parent, Path(__file__).parent / "holes"]
TABLES = ("sales", "products", "regions")

# What R writes for a missing value. Not an empty cell, which is how it writes
# an empty text too, and not `NA`, which is a text someone could hold.
MISSING = "__god_missing__"


def sentences(path: Path) -> list[str]:
    """The sentences in a corpus file, read the way `check.py` reads them.

    Comment lines are dropped here too: three harnesses reading one file three
    ways is how a comment in the corpus would break two of them and not the
    third.
    """
    out = []
    for chunk in path.read_text().split("\n---\n"):
        body = "\n".join(
            line for line in chunk.splitlines() if not line.lstrip().startswith("#")
        ).strip()
        if body:
            out.append(body)
    return out


def render(sentence: str, backend: str, columns: list[str]) -> str | None:
    """The printed text, or `None` where the grammar turned the sentence away."""
    from god.run import _binary

    args = [_binary()]
    for c in columns:
        args += ["--columns", c]
    args += ["--as", backend]
    done = subprocess.run(args, input=sentence, capture_output=True, text=True)
    return done.stdout.strip() if done.returncode == 0 else None


def tables(fixture: Path) -> dict:
    """The fixture's three tables, read the way pandas reads a CSV."""
    import pandas as pd

    return {name: pd.read_csv(fixture / f"{name}.csv") for name in TABLES}


def cell(v) -> str:
    """One value as comparable text.

    **Text stays text**, so `"100.0"` and `"100"` are two answers. A number is
    rounded to six places, so two engines' last-digit arithmetic is not a
    disagreement. A truth value is one word whatever type carried it, and a
    date is a date whether it came back as a date or a timestamp at midnight.
    """
    import datetime
    import decimal

    import numpy as np
    import pandas as pd

    if v is None:
        return "missing"
    try:
        if pd.isna(v):
            return "missing"
    except (TypeError, ValueError):
        pass
    if isinstance(v, (bool, np.bool_)):
        return "yes" if v else "no"
    if isinstance(v, str):
        return v
    if isinstance(v, (int, float, np.integer, np.floating, decimal.Decimal)):
        return f"{float(v):.6f}"
    if isinstance(v, (pd.Timestamp, datetime.datetime, datetime.date, np.datetime64)):
        ts = pd.Timestamp(v)
        return ts.strftime("%Y-%m-%d") if ts == ts.normalize() else ts.isoformat(sep=" ")
    return str(v)


def as_rows(frame, ordered: bool = False) -> tuple[list[tuple[str, ...]], list[str]]:
    """A table as comparable text: columns sorted, and rows sorted unless the
    order is part of the answer."""
    frame = frame.copy()
    frame.columns = [str(c) for c in frame.columns]
    frame = frame[sorted(frame.columns)]
    rows = [tuple(cell(v) for v in row) for row in frame.itertuples(index=False)]
    return (rows if ordered else sorted(rows)), sorted(frame.columns)


def query_answer(query: str, frames: dict):
    """The query's table, on one thread so its order is the query's alone."""
    import duckdb

    con = duckdb.connect()
    con.execute("SET threads = 1")
    for name, frame in frames.items():
        con.register(name, frame)
    return con.execute(query).fetch_df()


def settled(query: str, frames: dict, answer) -> bool:
    """Whether the query's row order belongs to the sentence.

    Run again over the fixture's rows reversed and shuffled: an order the
    sentence settles comes back the same, and one left to the engine moves
    with its input. A sort with tied keys moves too, so its rows are compared
    as a set, which is the honest reading of a tie.
    """
    first = as_rows(answer, ordered=True)
    for shuffle in (
        # `copy`, because DuckDB cannot read a reversed view's arrays.
        lambda f: f.iloc[::-1].copy().reset_index(drop=True),
        lambda f: f.sample(frac=1, random_state=7).reset_index(drop=True),
    ):
        moved = query_answer(query, {k: shuffle(v) for k, v in frames.items()})
        if as_rows(moved, ordered=True) != first:
            return False
    return True


# **dplyr is the seventh backend and the only one no harness ran.** It cannot
# join `targets()` below, because that dictionary holds Python scopes and a
# `eval` — dplyr needs R. So it gets a driver of its own, in the shape
# `parity/check.py` already uses for the R spelling: the printed code is written
# to a scratch file, `Rscript` evaluates it against the same three fixtures, and
# the answer comes back as CSV to be compared the way every other target's is.
#
# **The comparison is the table, never the text**, which is the whole point of
# this file: printed dplyr was read by a person before today, and a rendering
# that reads perfectly can still mean something else.
#
# **One R process per fixture, every sentence in it**, since 2026-10-01: an
# `Rscript` per sentence was most of this file's minute, and doubling the
# fixtures would have doubled it.
R_DRIVER = r"""
suppressMessages({
  library(dplyr, warn.conflicts = FALSE)
  library(tidyr, warn.conflicts = FALSE)
})
args <- commandArgs(trailingOnly = TRUE)
fixture <- args[[1]]
scratch <- args[[2]]
# An empty cell is missing, the way pandas reads it, so both sides of every
# comparison start from one table.
read <- function(name) {
  read.csv(file.path(fixture, paste0(name, ".csv")), stringsAsFactors = FALSE,
           na.strings = c("", "NA"))
}
sales <- read("sales")
products <- read("products")
regions <- read("regions")
for (code in sort(list.files(scratch, pattern = "^[0-9]+\\.R$", full.names = TRUE))) {
  out <- sub("\\.R$", ".csv", code)
  printed <- paste(readLines(code, warn = FALSE), collapse = "\n")
  result <- tryCatch({
    answer <- as.data.frame(eval(parse(text = printed)))
    # The first line says which columns are text, so a CSV's guessing cannot
    # read a text "100" back as a number; the rest is the table, with a missing
    # value written as a marker an empty text cannot be confused with.
    classes <- vapply(answer, function(x) class(x)[[1]], character(1))
    con <- file(out, "w")
    writeLines(paste(classes, collapse = ","), con)
    write.csv(answer, con, row.names = FALSE, na = "__god_missing__")
    close(con)
  }, error = function(e) {
    writeLines(paste0("ERROR ", conditionMessage(e)), sub("\\.csv$", ".err", out))
  })
}
"""


def dplyr_answers(printed: dict[int, str], fixture: Path, scratch: Path) -> dict:
    """Every printed dplyr, evaluated by one R process, as frames or errors."""
    import pandas as pd

    for old in scratch.glob("*"):
        old.unlink()
    for n, code in printed.items():
        (scratch / f"{n:04d}.R").write_text(code)
    driver = scratch.parent / "printed-dplyr.R"
    driver.write_text(R_DRIVER)
    done = subprocess.run(
        ["Rscript", str(driver), str(fixture), str(scratch)],
        capture_output=True, text=True,
    )
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip()[-400:] or "Rscript failed")
    out = {}
    for n in printed:
        err = scratch / f"{n:04d}.err"
        csv = scratch / f"{n:04d}.csv"
        if err.exists():
            out[n] = RuntimeError(err.read_text().strip()[6:][:96])
            continue
        lines = csv.read_text().splitlines()
        classes = lines[0].split(",")
        frame = pd.read_csv(
            io.StringIO("\n".join(lines[1:])),
            dtype=str, keep_default_na=False, na_values=[MISSING],
        )
        for name, kind in zip(frame.columns, classes):
            if kind in ("numeric", "integer", "integer64"):
                frame[name] = pd.to_numeric(frame[name])
            elif kind == "logical":
                frame[name] = frame[name].map({"TRUE": True, "FALSE": False})
            elif kind in ("Date", "POSIXct"):
                frame[name] = pd.to_datetime(frame[name])
        out[n] = frame
    return out


def have_r() -> bool:
    """R, with the packages the corpus's printed dplyr reaches for.

    Named rather than probed one at a time so the message can say which is
    missing: `stringr`, `lubridate`, `slider` and `vctrs` are each reached by a
    handful of sentences, and a missing one fails only those.
    """
    wanted = ["dplyr", "tidyr", "stringr", "lubridate", "slider", "vctrs"]
    probe = ";".join(f'if (!requireNamespace("{p}", quietly=TRUE)) quit(status=1)' for p in wanted)
    try:
        return subprocess.run(["Rscript", "-e", probe], capture_output=True).returncode == 0
    except FileNotFoundError:
        return False


def targets(frames: dict) -> dict:
    """Each printing backend that can be run here, with the scope it needs.

    A backend whose library is missing is left out rather than failed, and the
    caller says so. `pandas` is always available, because the package requires
    it.
    """
    import numpy as np
    import pandas as pd

    found = {"pandas": ({"pd": pd, "np": np, **frames}, lambda f: f)}

    try:
        import polars as pl

        found["polars"] = (
            {"pl": pl, **{k: pl.from_pandas(v) for k, v in frames.items()}},
            lambda f: f.to_pandas(),
        )
    except ImportError:
        pass

    try:
        spark = spark_session()
        from pyspark.sql import Window
        from pyspark.sql import functions as F

        found["pyspark"] = (
            {"F": F, "Window": Window,
             **{k: spark.createDataFrame(spark_ready(v)) for k, v in frames.items()}},
            lambda f: f.toPandas(),
        )
    except (ImportError, Exception):
        pass

    return found


_SPARK = None


def spark_session():
    """One Spark session for every fixture, started once."""
    global _SPARK
    if _SPARK is None:
        from pyspark.sql import SparkSession
        import os

        os.environ.setdefault("PYSPARK_PYTHON", sys.executable)
        _SPARK = (
            SparkSession.builder.master("local[1]")
            .appName("god-printed")
            .config("spark.ui.enabled", "false")
            .config("spark.sql.shuffle.partitions", "1")
            # **Spark writes a warehouse directory wherever it is started**, so
            # without this a `spark-warehouse/` folder appears at the
            # repository root every time the suite runs. The root ignore file is
            # an allowlist, so git never showed it and it simply accumulated.
            # Nothing in it is worth keeping: these harnesses register
            # temporary views and never save a table.
            .config("spark.sql.warehouse.dir", tempfile.mkdtemp(prefix="god-spark-"))
            .getOrCreate()
        )
        _SPARK.sparkContext.setLogLevel("ERROR")
    return _SPARK


def spark_ready(frame):
    """A frame Spark reads a hole in as a null.

    pandas holds a missing number as NaN, and Spark keeps NaN as a number of
    its own rather than as a missing value, so every hole is handed over as
    `None`.
    """
    return frame.astype(object).where(frame.notna(), None)


def main() -> int:
    from god.run import _columns_of

    corpus = sentences(CORPUS)
    use_r = have_r()
    reported_skips = False
    failures = 0

    for fixture in FIXTURES:
        frames = tables(fixture)
        columns = [_columns_of(frames["sales"]), f"products={_columns_of(frames['products'])}",
                   f"regions={_columns_of(frames['regions'])}"]
        holed = fixture != FIXTURES[0]
        print(f"\n{'the fixture with holes' if holed else 'the plain fixture'}: {fixture.relative_to(ROOT)}")

        # The query's answer once per sentence, and whether its order is the
        # sentence's. A query that refuses this data is the answer too.
        wanted: dict[int, tuple] = {}
        refused = 0
        for n, sentence in enumerate(corpus, 1):
            query = render(sentence, "sql", columns)
            if query is None:
                continue
            try:
                answer = query_answer(query, frames)
            except Exception as e:  # noqa: BLE001
                if not holed:
                    failures += 1
                    print(f"  QUERY FAILED {n}. {' '.join(sentence.split())[:58]}\n        {e}")
                else:
                    refused += 1
                    print(f"  refused by the query on this data {n}. {' '.join(sentence.split())[:48]}")
                continue
            wanted[n] = (answer, settled(query, frames, answer))

        found = targets(frames)
        if not reported_skips:
            for name in ("polars", "pyspark"):
                if name not in found:
                    print(f"{name} is not installed, so its printed code is unchecked here.")
                    print(f"It is not a required dependency: pip install {name}")

        results: dict[str, dict[int, object]] = {}
        for backend, (scope, collect) in found.items():
            results[backend] = {}
            for n, sentence in enumerate(corpus, 1):
                if n not in wanted:
                    continue
                printed = render(sentence, backend, columns)
                if printed is None:
                    continue
                try:
                    results[backend][n] = collect(eval(printed, dict(scope)))
                except Exception as e:  # noqa: BLE001
                    results[backend][n] = e

        if use_r:
            printed = {}
            for n, sentence in enumerate(corpus, 1):
                if n in wanted:
                    code = render(sentence, "dplyr", columns)
                    if code is not None:
                        printed[n] = code
            with tempfile.TemporaryDirectory(prefix="god-dplyr-") as tmp:
                scratch = Path(tmp) / "sentences"
                scratch.mkdir()
                results["dplyr"] = dplyr_answers(printed, fixture, scratch)

        for backend, answers in results.items():
            agreed = differed = broke = 0
            for n, got in answers.items():
                short = " ".join(corpus[n - 1].split())[:58]
                if isinstance(got, Exception):
                    broke += 1
                    first = str(got).splitlines()[0][:96] if str(got) else type(got).__name__
                    print(f"  WILL NOT RUN {backend} {n}. {short}\n        {first}")
                    continue
                answer, in_order = wanted[n]
                if as_rows(answer, in_order) == as_rows(got, in_order):
                    agreed += 1
                else:
                    differed += 1
                    how = "in order" if in_order else "as a set"
                    print(f"  DIFFERS {backend} {n}, compared {how}. {short}")
                    print(f"        the sentence: {as_rows(answer, in_order)[0][:3]}")
                    print(f"        the printing: {as_rows(got, in_order)[0][:3]}")
            failures += differed + broke
            print(f"{backend}: {agreed} agreed, {differed} differed, {broke} would not run")
        if holed:
            print(f"refused by the query on this data, and not compared: {refused}")
        reported_skips = True

    if not use_r:
        print("R, or one of dplyr/tidyr/stringr/lubridate/slider/vctrs, is missing,")
        print("so printed dplyr is unchecked here. It is not a required dependency.")
    if not use_r or _SPARK is None:
        print("A skip is not a pass. The targets above that were not installed")
        print("have not been checked, and a rendering can be wrong in silence.")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
