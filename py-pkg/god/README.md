# py-pkg/god

The Python launcher. **Not built yet.**

```python
run("""
sales
  then keep where [region] is "West"
  then summarize [margin] as total([margin]) by [product]
""")
```

## What goes here

| File | Owns |
|---|---|
| `god/run.py` | Text in, frame out. Gets the schema, calls the grammar, runs the query |
| `god/capture.py` | Finding `sales` in the caller's frame, as `duckdb.sql` does |
| `god/magic.py` | The Jupyter cell magic, so a notebook needs no quotes at all |
| `pyproject.toml` | Distribution `grammar-of-data`, import `god` |

## What does not go here

**Any decision at all.** Validation, defaults, coercion and every error message
live in the grammar. This package finds a frame, hands over some text, runs the
query it gets back, and returns a native frame.

## Why the distribution is not called `god`

`god` was taken on PyPI in 2016 by an unrelated package. A distribution name and
an import name are independent, so `pip install grammar-of-data` installs a
package you `import god`, and every example in the manual reads the same in both
languages.

## Triple quotes, so the text is the text

A pipeline needs no escaping and is byte-for-byte the same characters as the one
you would paste into R or into a database.

## What was checked about Python's operators

Run against pandas 3.0.5 and polars 1.43.2.

- **`|>` is impossible.** It does not tokenize, and Python's operator set is fixed
  by the grammar. No import hook will be used to fake it, because that breaks the
  editor support such a front end would exist to provide.
- **`>>` is available.** Neither pandas nor polars defines `__rshift__`, so a
  bare frame falls through to `__rrshift__`. Verified by running it.
- **`|` is refused.** Both define `__or__`, and `frame | verb` never reaches the
  verb — it does an elementwise or and returns a frame of `True`. A silent wrong
  answer is the one outcome this project will not ship.
