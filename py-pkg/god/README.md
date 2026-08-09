# god — a grammar of data, for Python

One small vocabulary for manipulating tables, spelled the same way in Python,
in R, and as plain text. A pipeline is checked whole before any of it runs, so
a bad column is reported at the step that names it rather than failing partway
through — or worse, not failing.

```python
import pandas as pd
from god import *

sales = pd.read_csv("sales.csv")

sales >> keep(col.region == "West") \
      >> summarize(margin = total(col.margin), by = col.product)
```

The same sentence runs as plain text, byte-for-byte what you would paste into
R or into a database:

```python
run("""
sales
  then keep where [region] is "West"
  then summarize [margin] as total([margin]) by [product]
""")
```

And when you reach the edge of the vocabulary, it shows you the same pipeline
in a tool you already know — `show_as(pipeline, "pandas")`, or `"polars"`,
`"pyspark"`, `"dplyr"`, `"sql"`, `"spark"`.

## Installing

The distribution is `grammar-of-data` and the import is `god` — the PyPI name
`god` was taken in 2016 by an unrelated package, and a distribution name and
an import name are independent. Until the PyPI release lands, install a wheel
built from the repository at <https://github.com/psychometrician/god>, which
carries the engine inside it.

## What is here

| File | Owns |
|---|---|
| `god/verbs.py` | The fourteen verbs. Each builds a sentence and decides nothing |
| `god/columns.py` | How Python names a column: `col.region`, and the expressions it grows |
| `god/run.py` | The text form, finding the engine, and running the query |
| `setup.py` | Packs the engine into the wheel, and tags the wheel for the platform the engine was built for |

**Any decision at all** lives in the grammar, not here. This package finds a
frame, hands over some text, runs the query it gets back, and returns a native
frame.

## What was checked about Python's operators

- **`|>` is impossible.** It does not tokenize, and Python's operator set is
  fixed by the grammar. No import hook fakes it, because that breaks the editor
  support a front end exists to provide.
- **`>>` is available.** Neither pandas nor polars defines `__rshift__`, so a
  bare frame falls through to `__rrshift__`. Verified by running it.
- **`|` is refused.** Both define `__or__`, and `frame | verb` never reaches
  the verb — it does an elementwise or and returns a frame of `True`. A silent
  wrong answer is the one outcome this project will not ship.

The manual, live in both languages, is at
<https://psychometrician.github.io/god-book/>.
