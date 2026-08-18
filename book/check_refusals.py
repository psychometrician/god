"""check_refusals.py — the Python half of every documented refusal.

Run from the repository root; run by the Python test suite.

**This is `check_refusals.R`'s twin, and the tabsets are why it exists.** Every
refusal in the book is shown twice, once per language. The R half is a chunk
marked `#| error: true`, and the R guard forces each one and asserts it still
refuses. The Python half is a `try/except GodError` block, which is not marked
at all: it prints the refusal when the grammar refuses, and it would print a
table just as happily the day the grammar stopped. Two languages proving each
other is the whole point of showing both, and until this file only one of them
was proved.

**A pipeline is lazy here too.** `sales >> keep(...)` builds a sentence and
raises nothing; `collect`, or printing, is what hands it to the engine. Every
refusal block in the book collects inside the `try`, and where one does not,
the pipeline it builds is forced here, or a refusal that no longer refuses
would look exactly like one that does.

**An error is not automatically a refusal**, so the same rule as the R guard:
the short, stable list is the errors that mean *this file* failed to give a
chunk what it needs (a missing name, a missing import). Anything else that
raises is the grammar, or the driver carrying the grammar's answer, and both
of those are refusals.
"""

import ast
import contextlib
import io
import os
import sys
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BOOK = ROOT / "book"


@contextlib.contextmanager
def working_directory(directory: Path):
    """Run a chapter's chunks from the chapter's own directory, as knitr does.

    **This guard runs from the repository root and the render does not**, which
    did not matter until a chapter's table came from `god_table`. That walks up
    from the working directory for a `data/<name>.csv`, so from `book/chapters/`
    it finds `book/data/` and from the root it finds nothing and reaches for the
    published copy — a table not yet published is then a `NameError` here and a
    rendered page there. Matching knitr is the fix rather than special-casing
    the walk: a guard that executes chunks should execute them where they run.
    """
    was = Path.cwd()
    os.chdir(directory)
    try:
        yield
    finally:
        os.chdir(was)


def python_chunks(lines: list[str]) -> list[dict]:
    """Every ```{python} chunk, with its first line number and its code."""
    out = []
    i = 0
    while i < len(lines):
        if lines[i].strip() == "```{python}":
            start = i + 1
            i += 1
            body = []
            while i < len(lines) and lines[i].strip() != "```":
                body.append(lines[i])
                i += 1
            out.append({"line": start, "code": "\n".join(body)})
        i += 1
    return out


def catches_goderror(node: ast.Try) -> bool:
    for handler in node.handlers:
        kind = handler.type
        names = []
        if isinstance(kind, ast.Name):
            names = [kind.id]
        elif isinstance(kind, ast.Tuple):
            names = [e.id for e in kind.elts if isinstance(e, ast.Name)]
        if "GodError" in names:
            return True
    return False


def run_statements(statements, namespace, filename):
    """Execute statements; the last one, if an expression, hands back its value."""
    value = None
    for i, statement in enumerate(statements):
        module = ast.Module(body=[statement], type_ignores=[])
        ast.fix_missing_locations(module)
        if i == len(statements) - 1 and isinstance(statement, ast.Expr):
            expression = ast.Expression(body=statement.value)
            ast.fix_missing_locations(expression)
            value = eval(compile(expression, filename, "eval"), namespace)
        else:
            exec(compile(module, filename, "exec"), namespace)
    return value


def force(value, namespace):
    """A pipeline that was built and never run is run now.

    Printing is what forces one in a chapter, so the check forces one here.
    """
    if value is not None and hasattr(value, "written"):
        namespace["collect"](value)
        return "the pipeline ran and returned a table"
    return "it evaluated without complaint"


def main() -> int:
    if not BOOK.exists():
        print("SKIP: no book/ here, so there are no refusal chunks to check")
        return 0

    # The shared fixtures, read the way a chapter reads them.
    shared: dict = {}
    setup = BOOK / "_setup.qmd"
    if setup.exists():
        for chunk in python_chunks(setup.read_text().splitlines()):
            try:
                exec(compile(chunk["code"], str(setup), "exec"), shared)
            except Exception:
                pass

    # The errors that mean this guard is broken rather than the book: Python's
    # spellings of "object not found" and "could not find function".
    harness = (NameError, ImportError)

    checked = 0
    quiet: list[str] = []
    wrong: list[str] = []

    qmds = sorted(p for p in BOOK.rglob("*.qmd") if "/_" not in str(p))
    for f in qmds:
        lines = f.read_text().splitlines()
        chunks = python_chunks(lines)
        parsed = []
        for chunk in chunks:
            try:
                tree = ast.parse(chunk["code"])
            except SyntaxError:
                tree = None
            parsed.append(tree)
        refusing = [
            any(isinstance(n, ast.Try) and catches_goderror(n) for n in ast.walk(t))
            if t is not None
            else False
            for t in parsed
        ]
        if not any(refusing):
            continue

        # Chapter-local tables come from earlier chunks, so the chapter is
        # walked in order up to its last refusal. The fixtures are copied in,
        # not inherited, for the R guard's reason: a chunk has to see exactly
        # the tables a chapter gave it.
        namespace = dict(shared)
        last = max(i for i, r in enumerate(refusing) if r)

        with working_directory(f.parent):
            for i in range(last + 1):
                chunk, tree = chunks[i], parsed[i]
                where = f"{f.relative_to(ROOT)}:{chunk['line']}"
                if tree is None:
                    continue

                if not refusing[i]:
                    # Run for its tables, not for its output.
                    try:
                        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                            exec(compile(tree, str(f), "exec"), namespace)
                    except Exception:
                        pass
                    continue

                # A refusal chunk can carry setup around its `try`, so the chunk
                # is executed statement by statement and only the `try` bodies
                # are held to the promise.
                for node in tree.body:
                    if isinstance(node, ast.Try) and catches_goderror(node):
                        checked += 1
                        shown = " ".join(
                            ast.get_source_segment(chunk["code"], node.body[0]).split()
                        ) if node.body else ""
                        try:
                            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                                value = run_statements(node.body, namespace, str(f))
                                outcome = force(value, namespace)
                        except harness as e:
                            wrong.append(f"{where}: {type(e).__name__}: {e}")
                            continue
                        except Exception:
                            continue  # it refused, which is the promise kept
                        quiet.append(f"{where} ({outcome}): {shown}")
                    else:
                        try:
                            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                                run_statements([node], namespace, str(f))
                        except Exception:
                            pass

    if not checked:
        print(
            "check_refusals.py: found no try/except GodError chunks, "
            "so the scan is broken rather than the book"
        )
        return 1

    if quiet or wrong:
        if quiet:
            print("FAIL: shown as refusals, and they did not refuse")
            for line in quiet:
                print(f"  {line}")
            print("  Either the grammar stopped refusing, or the prose should stop saying it does.")
        if wrong:
            print("FAIL: these failed for a reason that is not a refusal")
            for line in wrong:
                print(f"  {line}")
            print("  That is this guard missing a table, not the book being wrong. Fix the guard.")
        return 1

    print(f"PASS: every Python refusal refuses ( {checked} chunks )")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
