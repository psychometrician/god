"""Does the same text mean the same thing in R and in Python?

Run from the repository root::

    GOD_CLI=target/release/god-cli python3 parity/check.py

**This is the check the whole project rests on.** Everything else proves the
grammar works in one language at a time; this proves the claim, which is that a
pipeline written once means the same thing wherever it is read.

Two things are compared for every sentence in the corpus:

1. **The query.** Both launchers should produce byte-identical SQL. There is one
   parser and one backend, so this holds by construction — and it is asserted
   anyway, because "by construction" is a belief until something checks it.
2. **The table.** Same rows, same order, same column order, same values.

The second is where a real disagreement could hide, and there is exactly one
place it could come from: **each launcher decides what its host calls a column's
type**, and that is the only judgement either of them makes. If R reads a column
as text where pandas reads it as a number, the grammar checks two different
sentences and nobody would know. That is what this catches.

The fixture is one CSV read by both, so the two are not comparing different data
and calling it agreement.
"""

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# **Each run gets its own directory, so two of these can run at once.** They
# could not before: the driver and the sentence under test were written to
# `parity/.driver.R` and `parity/.pipeline.god`, fixed names, so a second run
# overwrote the sentence between the write and the `Rscript` that read it and
# each witness answered the other run's question. Nothing raised. The tables
# simply disagreed — 57 of 75 in the run that found this, and 52 of 75 in the
# one it was racing — which reads exactly like a regression and is not one.
#
# More than one agent works in this tree at a time, and the suites are the first
# thing each of them runs, so this was a live way to lose an afternoon chasing a
# break that did not exist.
#
# It also cannot leave a stray behind in the repository, which is what the fixed
# names were being cleaned up to avoid.
SCRATCH = Path(tempfile.mkdtemp(prefix="god-parity-"))
sys.path.insert(0, str(ROOT / "py-pkg" / "god"))

import pandas as pd  # noqa: E402

import god  # noqa: E402

CORPUS = ROOT / "parity" / "corpus.god"
R_CORPUS = ROOT / "parity" / "corpus.R"
PY_CORPUS = ROOT / "parity" / "corpus.py"
FIXTURE = ROOT / "parity" / "sales.csv"
OTHER = ROOT / "parity" / "products.csv"

R_DRIVER = r"""
suppressMessages(pkgload::load_all("r-pkg/god", export_all = FALSE, quiet = TRUE))
args <- commandArgs(trailingOnly = TRUE)
sales <- read.csv(args[[1]], stringsAsFactors = FALSE)
products <- read.csv(sub("sales.csv", "products.csv", args[[1]]), stringsAsFactors = FALSE)
pipeline <- readLines(args[[2]], warn = FALSE)
pipeline <- paste(pipeline, collapse = "\n")
mode <- args[[3]]
if (mode == "sql") {
  wanted <- god:::god_needs(pipeline)
  have <- list(sales = sales, products = products)[wanted]
  cat(god:::god_call(god:::columns_args(have, wanted[[1]]), pipeline))
} else if (mode == "native") {
  # The R spelling, evaluated, and asked what it wrote. Nothing runs: the verbs
  # build a sentence and this prints it.
  cat(format(eval(parse(text = pipeline))))
} else {
  answer <- run(pipeline, sales = sales, products = products)
  write.csv(answer, row.names = FALSE, na = "<missing>")
}
"""


def r(pipeline: str, mode: str) -> str:
    driver = SCRATCH / "driver.R"
    driver.write_text(R_DRIVER)
    text = SCRATCH / "pipeline.god"
    text.write_text(pipeline)
    result = subprocess.run(
        ["Rscript", str(driver), str(FIXTURE), str(text), mode],
        capture_output=True,
        text=True,
        cwd=ROOT,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip() or result.stdout.strip())
    # duckdb's first connection announces where it keeps its extensions.
    return "\n".join(
        line for line in result.stdout.splitlines() if not line.startswith(("duckdb is", "ℹ"))
    ).strip()


def py(pipeline: str, mode: str) -> str:
    sales = pd.read_csv(FIXTURE)
    products = pd.read_csv(OTHER)
    if mode == "sql":
        from god.run import _columns_args, _needs

        tables = {"sales": sales, "products": products}
        wanted = _needs(pipeline)
        from god.run import _call

        return _call(_columns_args({k: tables[k] for k in wanted}, wanted[0]), pipeline).strip()
    answer = god.run(pipeline, sales=sales, products=products)
    return answer.to_csv(index=False, na_rep="<missing>").strip()


def py_native(sentence: str) -> str:
    """The Python spelling, evaluated, and asked what it wrote.

    Nothing runs: the verbs build a sentence and this reads it back. The frame is
    bound to the name `sales` so that the verbs can recover it, which is the one
    thing Python had to solve differently from R.
    """
    scope = dict(vars(god))
    scope["sales"] = pd.read_csv(FIXTURE)
    scope["products"] = pd.read_csv(OTHER)
    return eval(sentence, scope).written()


def canonical(pipeline: str) -> str:
    """A pipeline printed back as the grammar itself.

    Both witnesses go through this before they are compared, so the comparison
    is about **what the sentence means** rather than how it was spaced. The R
    front end parenthesizes defensively and writes one step per line; a person
    writing the text form does neither. Printing both from their plans takes
    that difference out, and cannot hide a real one: the printer is a function of
    the plan, so two plans that differ anywhere print differently.
    """
    sales = pd.read_csv(FIXTURE)
    products = pd.read_csv(OTHER)
    from god.run import _binary, _columns_of

    result = subprocess.run(
        [
            _binary(),
            "--columns", _columns_of(sales),
            "--columns", f"products={_columns_of(products)}",
            "--as", "god",
        ],
        input=pipeline,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip())
    return result.stdout.strip()


def normalize_table(csv: str) -> list[list[str]]:
    """A table as text, with the two hosts' formatting differences taken out.

    R quotes text columns and writes 300 where pandas writes 300.0. Neither is a
    disagreement about the answer, so neither should be reported as one — and
    normalizing here is safe because the values themselves are still compared.

    **Truth values joined that list when `when` arrived**, which was the first
    thing in the corpus to put one in an answer. R writes `TRUE` and pandas
    writes `True`, and both are the same value out of the same query: the engine
    returned a boolean and each driver rendered it in its own language. That is
    the host's business and not the grammar's, exactly as the number formatting
    above is.
    """
    truth = {"TRUE": "yes", "True": "yes", "FALSE": "no", "False": "no"}
    rows = []
    for line in csv.splitlines():
        cells = []
        for cell in line.split(","):
            cell = cell.strip().strip('"')
            if cell in truth:
                cell = truth[cell]
            else:
                try:
                    number = float(cell)
                    cell = f"{number:g}"
                except ValueError:
                    pass
            cells.append(cell)
        rows.append(cells)
    return rows


def sentences(path: Path) -> list[str]:
    """The sentences in a corpus file, with its header comment dropped."""
    out = []
    for chunk in path.read_text().split("\n---\n"):
        body = "\n".join(
            line for line in chunk.splitlines() if not line.lstrip().startswith("#")
        ).strip()
        if body:
            out.append(body)
    return out


def main() -> int:
    # The working directory goes even when a witness dies mid-loop: an exception
    # used to leak the working files past the cleanup at the loop's end, back
    # when they were written into the repository and a leftover was a visible
    # stray in `git status`.
    try:
        return _main()
    finally:
        shutil.rmtree(SCRATCH, ignore_errors=True)


def _main() -> int:
    pipelines = sentences(CORPUS)
    r_native = sentences(R_CORPUS)
    py_corpus = sentences(PY_CORPUS)

    # Corpora that have drifted apart would silently test whichever is shortest,
    # and report agreement on the sentences that survived.
    counts = {
        "corpus.god": len(pipelines),
        "corpus.R": len(r_native),
        "corpus.py": len(py_corpus),
    }
    if len(set(counts.values())) != 1:
        print("the corpora disagree on how many sentences there are:")
        for name, count in counts.items():
            print(f"  {name}: {count}")
        return 1

    print(f"{len(pipelines)} sentences, each written four ways\n")

    agreed = 0
    disagreed = 0

    for i, (pipeline, native, native_py) in enumerate(
        zip(pipelines, r_native, py_corpus), 1
    ):
        one_line = " ".join(pipeline.split())
        label = one_line if len(one_line) <= 62 else one_line[:59] + "..."
        try:
            r_sql, py_sql = r(pipeline, "sql"), py(pipeline, "sql")
            r_table, py_table = r(pipeline, "table"), py(pipeline, "table")
            written = r(native, "native")
            written_py = py_native(native_py)
        except Exception as e:
            disagreed += 1
            print(f"  ERROR  {i:2}. {label}\n         {e}")
            continue

        if r_sql != py_sql:
            disagreed += 1
            print(f"  QUERY  {i:2}. {label}\n      R: {r_sql}\n     Py: {py_sql}")
            continue

        if normalize_table(r_table) != normalize_table(py_table):
            disagreed += 1
            print(f"  TABLE  {i:2}. {label}")
            print("         R  |", " / ".join(",".join(row) for row in normalize_table(r_table)))
            print("         Py |", " / ".join(",".join(row) for row in normalize_table(py_table)))
            continue

        # The third and fourth witnesses. The text arrives at the plan by being
        # parsed; each native form arrives by being built, in a different
        # language, by a translator written separately. None can borrow another's
        # answer, so agreement is evidence rather than tautology.
        want = canonical(pipeline)
        got_r = canonical(written)
        got_py = canonical(written_py)
        if want != got_r or want != got_py:
            disagreed += 1
            print(f"  NATIVE {i:2}. {label}")
            print("         text |", " ".join(want.split()))
            if got_r != want:
                print("         R    |", " ".join(got_r.split()))
            if got_py != want:
                print("         Py   |", " ".join(got_py.split()))
            continue

        agreed += 1
        print(f"  ok     {i:2}. {label}")

    print(f"\n{agreed} agreed, {disagreed} disagreed")
    return 1 if disagreed else 0


if __name__ == "__main__":
    sys.exit(main())
