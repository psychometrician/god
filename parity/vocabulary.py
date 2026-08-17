"""Every word the grammar has, checked against what each binding exposes.

**The engine is the only list, and this proves the bindings agree with it.**
`god-cli --vocabulary` prints the verbs, the functions and the grammar words
straight out of `god-core`, so nothing here writes down a vocabulary of its own.
That is the failure this file exists to catch: a list copied into a binding, or
into a test, goes stale the day a word is added and says nothing about it.

It has already happened. `except_` was the one word spelled differently in the
two languages, from M6 until it was noticed by a person reading the manual, and
no test could have found it because no test compared the two.

Two things are checked.

1. **Every verb the engine has is exported by both bindings.** A verb is the one
   kind of word both languages must bind: R exports its verbs and reads
   everything else out of a syntax tree, and Python exports both.
2. **Every function and marker Python exports is a word the engine has.** Python
   evaluates expressions, so its whole vocabulary is real bindings, and a name
   there that the grammar does not know is a name that cannot survive the trip.

What is deliberately *not* checked is that R exports the functions. It does not,
and must not: R captures an expression unevaluated, so `total` is read out of the
tree and defining one would mask a name for nothing.
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Python exports these and the grammar does not know them, each for a reason
# that is about Python rather than about the vocabulary.
PYTHON_ONLY = {
    "col": "how Python names a column; R writes a bare name",
    "Expr": "the type a column expression has; R uses no class",
    "GodError": "the exception a refusal becomes; R uses a condition",
    "collect": "materializing, which is a launcher word rather than a verb",
    "run": "the text form",
    "show_as": "the text form",
    "show_steps": "what a pipeline does to the table, drawn; a launcher word rather than a verb",
    "god_sql": "the text form",
    "god_table": "the book's tables, fetched by name from the published site",
}

# A grammar word both bindings spell differently, and identically to each other.
#
# **This is not `PYTHON_ONLY` and the difference matters.** That list is for
# names Python has and the grammar does not. These are the grammar's own words
# reached through a host that cannot say them: the text form writes `any` and
# `every`, which it may because it evaluates nothing, and Python evaluates —
# `any` is one of its builtins, and the vocabulary has avoided shadowing a
# builtin since `total` was chosen over `sum`.
#
# **R could have used the short words and does not, on purpose.** R never
# evaluates a condition either, so `any(...)` inside `keep` would only ever be a
# symbol. One spelling across both bindings is worth more than each binding
# matching the text form on its own, so R compounds them too — the way
# `take_last` and `first_present` are already compounded.
#
# Each entry maps the binding's spelling to the grammar word it carries, so this
# file cannot drift into a place to hide a name nobody checked.
BINDING_SPELLING = {
    "where_any": "any",
    "where_every": "every",
}


# The launcher names R exports beside its verbs, for the same reason.
#
# **`use_engine` is R's and Python has no equivalent, which is idiom rather than
# vocabulary.** The two languages reach a cluster differently: a Spark frame in
# Python carries its own session, so the engine there follows the data and needs
# nothing said; R has no such object, and a warehouse connection is a `DBI`
# handle somebody has to hand over. The sentences and every word in them are
# identical either way, which is what may not differ.
R_EXTRA = {"collect", "run", "show_as", "show_steps", "god_sql", "god_table", "use_engine"}


def engine_vocabulary() -> dict[str, set[str]]:
    # The binding's own resolution, `GOD_CLI` included, rather than a private
    # copy of the path. A private copy is how this harness once asked a
    # different engine than the one the sentences ran on.
    sys.path.insert(0, str(ROOT / "py-pkg" / "god"))
    from god.run import GodError, _binary

    try:
        engine = _binary()
    except GodError as refusal:
        print(refusal, file=sys.stderr)
        raise SystemExit(1)
    out = subprocess.run(
        [engine, "--vocabulary"], capture_output=True, text=True, check=True
    ).stdout
    words: dict[str, set[str]] = {}
    for line in out.splitlines():
        if not line.strip():
            continue
        role, word = line.split("\t", 1)
        words.setdefault(role, set()).add(word)
    return words


def r_exports() -> set[str]:
    namespace = (ROOT / "r-pkg" / "god" / "NAMESPACE").read_text()
    return set(re.findall(r"^export\(([^)]+)\)", namespace, re.MULTILINE))


def python_exports() -> set[str]:
    sys.path.insert(0, str(ROOT / "py-pkg" / "god"))
    import god

    return set(god.__all__)


def main() -> int:
    engine = engine_vocabulary()
    verbs = engine.get("verb", set())
    functions = set().union(
        engine.get("aggregate", set()),
        engine.get("scalar", set()),
        engine.get("window", set()),
    )
    words = engine.get("word", set())

    r = r_exports()
    py = python_exports()
    problems: list[str] = []

    print(f"the engine has {len(verbs)} verbs, {len(functions)} functions, "
          f"{len(words)} grammar words")

    for missing in sorted(verbs - r):
        problems.append(f"R does not export the verb `{missing}`")
    for missing in sorted(verbs - py):
        problems.append(f"Python does not export the verb `{missing}`")

    # R exports verbs and the launcher, and nothing else.
    for extra in sorted(r - verbs - R_EXTRA - set(BINDING_SPELLING)):
        problems.append(
            f"R exports `{extra}`, which is not a verb. R reads expressions out "
            "of the syntax tree and should bind none of them"
        )

    # Everything Python exports is a verb, a function, a grammar word, or on the
    # short list above with its reason.
    known = verbs | functions | words | set(PYTHON_ONLY) | set(BINDING_SPELLING)
    for extra in sorted(py - known):
        problems.append(
            f"Python exports `{extra}`, which the grammar does not have. Add it "
            "to the vocabulary, or to PYTHON_ONLY with the reason it is Python's"
        )

    # **A binding spelling is only allowed while the word it carries is real.**
    # Otherwise this list becomes a place to park a name nothing checks — which
    # is the failure every other list in this file is shaped to prevent.
    #
    # Only Python has to *export* one. R reads its conditions out of the syntax
    # tree and binds no expression name at all, so `where_any` is a symbol there
    # exactly as `where` and `value` already are, and looking for it in
    # `NAMESPACE` would be looking in the wrong place.
    for spelled, word in sorted(BINDING_SPELLING.items()):
        if word not in words:
            problems.append(
                f"`{spelled}` is recorded as the binding spelling of `{word}`, "
                f"and the grammar has no word `{word}`"
            )
        if spelled not in py:
            problems.append(f"Python does not export `{spelled}`")

    # And every function the engine has is reachable from Python, since Python
    # evaluates expressions and cannot read one out of a tree.
    for missing in sorted(functions - py):
        problems.append(f"Python does not export the function `{missing}`")

    if problems:
        for p in problems:
            print(f"  DISAGREE  {p}")
        print(f"\n{len(problems)} disagreement(s) between the engine and a binding")
        return 1

    print(f"the two bindings agree with the engine: {len(r)} R exports, "
          f"{len(py)} Python")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
