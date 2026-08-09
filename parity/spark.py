"""The same corpus, run on DuckDB and on Spark, with the two tables compared.

**This is what makes the multi-engine claim true rather than promised.** Every
other check in this directory proves the two *hosts* agree; this one proves the
two *engines* do. It is the same argument one level down: a sentence that means
one thing on a laptop and another on a cluster is worse than a sentence that
refuses, because nobody notices.

**It compares rows, never queries.** That is not a style preference here, it is
the finding that produced this file. `SELECT "region" FROM t` parses on Spark,
runs, returns the right number of rows, and every value in it is the text
`'region'` rather than the column, because Spark reads a double-quoted name as a
string. A check that asked "did the query run" would have passed while the
answer was wrong in every cell.

It needs pyspark and a JVM, which most machines will not have. It skips cleanly
when they are missing rather than failing, the way the sibling check does.

    python3 parity/spark.py
"""

import os
import subprocess
import tempfile
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "py-pkg" / "god"))

CORPUS = Path(__file__).parent / "corpus.god"
FIXTURE = Path(__file__).parent / "sales.csv"
OTHER = Path(__file__).parent / "products.csv"

# The one sentence in the corpus a dialect is *expected* to turn away, and the
# reason. An entry here is a decision on file, not a known bug: it says the
# engine cannot express what the sentence means, and that saying so is right.
# Nothing is in it today, because every `widen` in the corpus declares what it
# makes.
EXPECTED_REFUSALS: dict[str, str] = {}


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


def compile_to(pipeline: str, backend: str, columns: list[str]) -> tuple[bool, str]:
    """The query, or the refusal. Both are answers."""
    from god.run import _binary

    args = [_binary()]
    for c in columns:
        args += ["--columns", c]
    args += ["--as", backend]
    done = subprocess.run(args, input=pipeline, capture_output=True, text=True)
    if done.returncode != 0:
        return False, done.stderr.strip()
    # An assumption is printed on stderr and is not part of the query.
    return True, done.stdout.strip()


def as_rows(frame) -> list[list[str]]:
    """A table as text, so two engines are compared on values rather than types.

    DuckDB hands back a numpy int64 where Spark hands back a Python int, and a
    test that failed on that would be reporting on the drivers rather than on
    the grammar.
    """
    out = []
    for row in frame:
        out.append([_scalar(v) for v in row])
    return out


def _scalar(v) -> str:
    if v is None:
        return "missing"
    if isinstance(v, float) and v == int(v):
        return str(int(v))
    try:
        import decimal

        if isinstance(v, decimal.Decimal):
            return _scalar(float(v))
    except ImportError:
        pass
    return str(v)


def main() -> int:
    try:
        from pyspark.sql import SparkSession
    except ImportError:
        print("pyspark is not installed, so the Spark dialect is unchecked here.")
        print("It is not a required dependency; nothing else in the suite needs it.")
        return 0

    import duckdb
    import pandas as pd
    from god.run import _columns_of

    sales = pd.read_csv(FIXTURE)
    products = pd.read_csv(OTHER)
    columns = [_columns_of(sales), f"products={_columns_of(products)}"]

    duck = duckdb.connect()
    duck.register("sales", sales)
    duck.register("products", products)

    os.environ.setdefault("PYSPARK_PYTHON", sys.executable)
    spark = (
        SparkSession.builder.master("local[1]")
        .appName("god-parity")
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
    spark.createDataFrame(sales).createOrReplaceTempView("sales")
    spark.createDataFrame(products).createOrReplaceTempView("products")

    agreed = disagreed = refused = 0
    for n, sentence in enumerate(sentences(CORPUS), 1):
        short = " ".join(sentence.split())[:58]
        ok_duck, duck_text = compile_to(sentence, "sql", columns)
        ok_spark, spark_text = compile_to(sentence, "spark", columns)

        if not ok_spark:
            if sentence in EXPECTED_REFUSALS:
                refused += 1
                print(f"  refused{n:4d}. {short}...  ({EXPECTED_REFUSALS[sentence]})")
            else:
                disagreed += 1
                print(f"  SPARK REFUSED {n}. {short}\n        {spark_text}")
            continue
        if not ok_duck:
            disagreed += 1
            print(f"  DUCKDB REFUSED {n}. {short}\n        {duck_text}")
            continue

        try:
            want = as_rows(duck.execute(duck_text).fetchall())
        except Exception as e:  # noqa: BLE001
            disagreed += 1
            print(f"  DUCKDB FAILED {n}. {short}\n        {e}")
            continue
        try:
            got = as_rows([tuple(r) for r in spark.sql(spark_text).collect()])
        except Exception as e:  # noqa: BLE001
            disagreed += 1
            print(f"  SPARK FAILED {n}. {short}\n        {' '.join(str(e).split())[:150]}")
            continue

        if want == got:
            agreed += 1
            print(f"  ok     {n:4d}. {short}...")
        else:
            disagreed += 1
            print(f"  DIFFER {n:4d}. {short}")
            print(f"        duckdb: {want[:3]}")
            print(f"        spark : {got[:3]}")

    spark.stop()
    print(f"\n{agreed} agreed, {refused} refused as designed, {disagreed} disagreed")
    return 1 if disagreed else 0


if __name__ == "__main__":
    raise SystemExit(main())
