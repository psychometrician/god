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

```bash
pip install grammar-of-data
```

Then `import god`. The wheel carries its own engine, so there is nothing else
to install.

The manual, live in both languages, is at
<https://psychometrician.github.io/god-book/>.
