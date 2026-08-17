"""The same corpus, run on DuckDB and on a Databricks SQL warehouse.

**Named for what it tests rather than for whom.** It was `databricks.py` for
about a minute, which put a module called `databricks` first on the path and
made `from databricks import sql` import this file. The connector reported
itself missing and the run skipped, cleanly and wrongly, which is the shape of
failure this whole directory exists to refuse.

**`parity/spark.py` proves the Spark dialect runs on Spark. This proves it runs
on Databricks**, which is not the same claim and is the one the book makes: a
chapter tells the reader that a table living in a warehouse can be read where it
sits and the same sentence runs there unchanged. Until this file, nothing tested
that against a real warehouse. Local pyspark and Databricks SQL are different
engines behind one dialect name, and the differences that bite are the quiet
ones.

**It compares rows, never queries**, which is `spark.py`'s finding one level
along: `SELECT "region" FROM t` parses on Spark, runs, returns the right number
of rows, and every value is the text `region` rather than the column. A check
asking whether the query ran would pass while every cell was wrong.

**Nothing is left behind in the workspace.** The fixtures are temporary views on
one connection, so they exist for the length of the run and are gone when it
closes. Nothing is created in a catalog, and no file is uploaded.

It skips cleanly without the connector or without credentials, the way the
pyspark check skips without a JVM. Credentials come from the environment and are
never read from a file in this tree:

    export DATABRICKS_SERVER_HOSTNAME=...
    export DATABRICKS_HTTP_PATH=/sql/1.0/warehouses/...
    export DATABRICKS_TOKEN=...
    python3 parity/warehouse.py
"""

import os
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "py-pkg" / "god"))

from spark import CORPUS, FIXTURE, OTHER, as_rows, compile_to, sentences  # noqa: E402

# A sentence this engine is *expected* to turn away, and the reason. An entry
# here is a decision on file rather than a known bug: it says the engine cannot
# express what the sentence means, and that saying so is right.
EXPECTED_REFUSALS: dict[str, str] = {}

NEEDED = ("DATABRICKS_SERVER_HOSTNAME", "DATABRICKS_HTTP_PATH", "DATABRICKS_TOKEN")


def views(cursor, name: str, frame) -> None:
    """A fixture as a temporary view, built from literals rather than a file.

    **Uploading data to somebody's workspace to run a test is a larger act than
    the test.** These fixtures are fifteen rows and four; written as `VALUES`
    they need no storage, no permission and no cleanup, and the view is scoped to
    this connection.
    """
    columns = list(frame.columns)
    rows = []
    for row in frame.itertuples(index=False):
        cells = []
        for value in row:
            if value is None or value != value:
                cells.append("NULL")
            elif isinstance(value, str):
                cells.append("'" + value.replace("'", "''") + "'")
            else:
                cells.append(str(value))
        rows.append("(" + ", ".join(cells) + ")")
    named = ", ".join(f"`{c}`" for c in columns)
    cursor.execute(
        f"CREATE OR REPLACE TEMPORARY VIEW {name} ({named}) AS VALUES " + ", ".join(rows)
    )


def main() -> int:
    try:
        from databricks import sql as dbsql
    except ImportError:
        print("databricks-sql-connector is not installed, so the warehouse is unchecked here.")
        print("It is not a required dependency; nothing else in the suite needs it.")
        return 0

    missing = [name for name in NEEDED if not os.environ.get(name)]
    if missing:
        print("no warehouse credentials in the environment, so nothing was checked.")
        print("Missing: " + ", ".join(missing))
        return 0

    import duckdb
    import pandas as pd
    from god.run import _columns_of

    sales = pd.read_csv(FIXTURE)
    products = pd.read_csv(OTHER)
    regions = pd.read_csv(OTHER.parent / "regions.csv")
    columns = [_columns_of(sales), f"products={_columns_of(products)}",
               f"regions={_columns_of(regions)}"]

    duck = duckdb.connect()
    duck.register("sales", sales)
    duck.register("products", products)
    duck.register("regions", regions)

    connection = dbsql.connect(
        server_hostname=os.environ["DATABRICKS_SERVER_HOSTNAME"],
        http_path=os.environ["DATABRICKS_HTTP_PATH"],
        access_token=os.environ["DATABRICKS_TOKEN"],
    )
    cursor = connection.cursor()
    views(cursor, "sales", sales)
    views(cursor, "products", products)
    views(cursor, "regions", regions)

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
            cursor.execute(spark_text)
            got = as_rows([tuple(r) for r in cursor.fetchall()])
        except Exception as e:  # noqa: BLE001
            disagreed += 1
            print(f"  WAREHOUSE FAILED {n}. {short}\n        {' '.join(str(e).split())[:170]}")
            continue

        if want == got:
            agreed += 1
            print(f"  ok     {n:4d}. {short}...")
        else:
            disagreed += 1
            print(f"  DIFFER {n:4d}. {short}")
            print(f"        duckdb   : {want[:3]}")
            print(f"        warehouse: {got[:3]}")

    cursor.close()
    connection.close()
    print(f"\n{agreed} agreed, {refused} refused as designed, {disagreed} disagreed")
    return 1 if disagreed else 0


if __name__ == "__main__":
    raise SystemExit(main())
