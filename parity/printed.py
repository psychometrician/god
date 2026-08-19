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

    python3 parity/printed.py
"""

import subprocess
import tempfile
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "py-pkg" / "god"))

CORPUS = Path(__file__).parent / "corpus.god"
FIXTURE = Path(__file__).parent / "sales.csv"
OTHER = Path(__file__).parent / "products.csv"
REGIONS = Path(__file__).parent / "regions.csv"


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


def as_rows(frame) -> tuple[list[tuple[str, ...]], list[str]]:
    """A table as comparable text: columns sorted, rows sorted, numbers rounded.

    Row order is deliberately not compared here. Every target is asked to restate
    the order the grammar settles on, and where a `take` follows, slicing turns
    an order difference into a difference in *which rows*, which this does catch.
    """
    import pandas as pd

    frame = frame.copy()
    frame.columns = [str(c) for c in frame.columns]
    frame = frame[sorted(frame.columns)]
    rows = []
    for row in frame.itertuples(index=False):
        out = []
        for v in row:
            try:
                absent = v is None or pd.isna(v)
            except (TypeError, ValueError):
                absent = False
            if absent:
                out.append("missing")
            elif isinstance(v, bool):
                out.append(str(v))
            else:
                # numpy's int64 is not a Python int on every platform, so the
                # kind is asked by trying rather than by isinstance.
                try:
                    out.append(f"{float(v):.6f}")
                except (TypeError, ValueError):
                    out.append(str(v))
        rows.append(tuple(out))
    return sorted(rows), sorted(frame.columns)


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
R_DRIVER = r"""
suppressMessages({
  library(dplyr, warn.conflicts = FALSE)
  library(tidyr, warn.conflicts = FALSE)
})
args <- commandArgs(trailingOnly = TRUE)
sales <- read.csv(args[[1]], stringsAsFactors = FALSE)
products <- read.csv(sub("sales.csv", "products.csv", args[[1]]), stringsAsFactors = FALSE)
regions <- read.csv(sub("sales.csv", "regions.csv", args[[1]]), stringsAsFactors = FALSE)
printed <- paste(readLines(args[[2]], warn = FALSE), collapse = "\n")
# `write.csv` rather than printing the frame: a printed data.frame wraps, pads
# and truncates, and every one of those would have to be undone on the other
# side. A CSV is read back by the same pandas that reads the fixtures.
answer <- eval(parse(text = printed))
# A tibble, a grouped frame or a data.table all answer to as.data.frame; a
# grouped result would otherwise carry its grouping into the CSV as an attribute
# nobody compares.
write.csv(as.data.frame(answer), stdout(), row.names = FALSE, na = "")
"""


def dplyr_answer(printed: str, scratch: Path) -> "pd.DataFrame":
    """The printed dplyr, evaluated by R, as a frame. Raises what R said."""
    import pandas as pd

    driver = scratch / "printed-dplyr.R"
    driver.write_text(R_DRIVER)
    code = scratch / "printed.R"
    code.write_text(printed)
    result = subprocess.run(
        ["Rscript", str(driver), str(FIXTURE), str(code)],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip().splitlines()[-1] if result.stderr.strip() else "Rscript failed")
    import io

    return pd.read_csv(io.StringIO(result.stdout))


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


def targets():
    """Each printing backend that can be run here, with the scope it needs.

    A backend whose library is missing is left out rather than failed, and the
    caller says so. `pandas` is always available, because the package requires
    it.
    """
    import pandas as pd

    sales = pd.read_csv(FIXTURE)
    products = pd.read_csv(OTHER)
    regions = pd.read_csv(REGIONS)
    found = {}

    import numpy as np

    found["pandas"] = (
        {"pd": pd, "np": np, "sales": sales, "products": products, "regions": regions},
        lambda f: f,
    )

    try:
        import polars as pl

        found["polars"] = (
            {"pl": pl, "sales": pl.read_csv(FIXTURE), "products": pl.read_csv(OTHER),
             "regions": pl.read_csv(REGIONS)},
            lambda f: f.to_pandas(),
        )
    except ImportError:
        pass

    try:
        from pyspark.sql import SparkSession, Window
        from pyspark.sql import functions as F
        import os

        os.environ.setdefault("PYSPARK_PYTHON", sys.executable)
        spark = (
            SparkSession.builder.master("local[1]")
            .appName("god-printed")
            .config("spark.ui.enabled", "false")
            .config("spark.sql.shuffle.partitions", "1")
            # **Spark writes a warehouse directory wherever it is started**, so without
            # this a `spark-warehouse/` folder appears at the repository root every time
            # the suite runs. The root ignore file is an allowlist, so git never showed
            # it and it simply accumulated. Nothing in it is worth keeping: these
            # harnesses register temporary views and never save a table.
            .config("spark.sql.warehouse.dir", tempfile.mkdtemp(prefix="god-spark-"))
            .getOrCreate()
        )
        spark.sparkContext.setLogLevel("ERROR")
        found["pyspark"] = (
            {
                "F": F,
                "Window": Window,
                "sales": spark.createDataFrame(sales),
                "products": spark.createDataFrame(products),
                "regions": spark.createDataFrame(regions),
            },
            lambda f: f.toPandas(),
        )
    except (ImportError, Exception):
        pass

    return found


def main() -> int:
    import duckdb
    import pandas as pd
    from god.run import _columns_of

    sales = pd.read_csv(FIXTURE)
    products = pd.read_csv(OTHER)
    regions = pd.read_csv(REGIONS)
    columns = [_columns_of(sales), f"products={_columns_of(products)}",
               f"regions={_columns_of(regions)}"]

    found = targets()
    for name in ("polars", "pyspark"):
        if name not in found:
            print(f"{name} is not installed, so its printed code is unchecked here.")
            print(f"It is not a required dependency: pip install {name}")

    duck = duckdb.connect()
    duck.register("sales", sales)
    duck.register("products", products)
    duck.register("regions", regions)

    failures = 0
    for backend, (scope, collect) in found.items():
        agreed = differed = broke = 0
        for n, sentence in enumerate(sentences(CORPUS), 1):
            short = " ".join(sentence.split())[:58]
            query = render(sentence, "sql", columns)
            printed = render(sentence, backend, columns)
            if query is None or printed is None:
                continue

            wanted = duck.execute(query).fetch_df()
            try:
                got = collect(eval(printed, dict(scope)))
            except Exception as e:
                broke += 1
                first = str(e).splitlines()[0][:96] if str(e) else type(e).__name__
                print(f"  WILL NOT RUN {backend} {n}. {short}\n        {first}")
                continue

            if as_rows(wanted) == as_rows(got):
                agreed += 1
            else:
                differed += 1
                print(f"  DIFFERS {backend} {n}. {short}")
                print(f"        the sentence: {as_rows(wanted)[0][:2]}")
                print(f"        the printing: {as_rows(got)[0][:2]}")

        failures += differed + broke
        print(f"{backend}: {agreed} agreed, {differed} differed, {broke} would not run")

    # dplyr, through R, for the reason written beside `R_DRIVER`.
    if have_r():
        agreed = differed = broke = 0
        with tempfile.TemporaryDirectory(prefix="god-dplyr-") as tmp:
            scratch = Path(tmp)
            for n, sentence in enumerate(sentences(CORPUS), 1):
                short = " ".join(sentence.split())[:58]
                query = render(sentence, "sql", columns)
                printed = render(sentence, "dplyr", columns)
                if query is None or printed is None:
                    continue

                wanted = duck.execute(query).fetch_df()
                try:
                    got = dplyr_answer(printed, scratch)
                except Exception as e:
                    broke += 1
                    print(f"  WILL NOT RUN dplyr {n}. {short}\n        {str(e)[:96]}")
                    continue

                if as_rows(wanted) == as_rows(got):
                    agreed += 1
                else:
                    differed += 1
                    print(f"  DIFFERS dplyr {n}. {short}")
                    print(f"        the sentence: {as_rows(wanted)[0][:2]}")
                    print(f"        the printing: {as_rows(got)[0][:2]}")
        failures += differed + broke
        print(f"dplyr: {agreed} agreed, {differed} differed, {broke} would not run")
    else:
        print("R, or one of dplyr/tidyr/stringr/lubridate/slider/vctrs, is missing,")
        print("so printed dplyr is unchecked here. It is not a required dependency.")

    if len(found) < 3 or not have_r():
        print("A skip is not a pass. The targets above that were not installed")
        print("have not been checked, and a rendering can be wrong in silence.")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
