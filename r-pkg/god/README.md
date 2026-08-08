# r-pkg/god

The R launcher. **Not built yet.**

```r
run(r"(
sales
  then keep where [region] is "West"
  then summarize [margin] as total([margin]) by [product]
)")
```

## What goes here

| File | Owns |
|---|---|
| `R/run.R` | Text in, table out. Gets the schema, calls the grammar, runs the query |
| `R/capture.R` | Finding `sales` in the caller's environment, so a pipeline names a table plainly |
| `R/engine.R` | The knitr and Quarto chunk engine, so a notebook needs no quotes at all |
| `NAMESPACE` | Hand maintained, not roxygen generated |
| `configure` | Bundles the binary into `inst/bin/`, and refuses rather than installing a package that cannot run |

## What does not go here

**Any decision at all.** Validation, defaults, coercion and every error message
live in the grammar. This package finds a table, hands over some text, runs the
query it gets back, and returns a data frame. That is the whole job.

## Raw strings, so the text is the text

R 4.0 added raw strings, so a pipeline needs no escaping and is byte-for-byte the
same characters as the one you would paste into Python or into a database:

```r
run(r"(sales then keep where [region] is "West")")
```

## No quotes at all, in a notebook

The package registers a chunk engine, exactly as R Markdown already does for
`sql`:

````
```{god}
sales then keep where [region] is "West" then take 10
```
````

## Names this package masks

`run`, and nothing else so far. The grammar's own words — `keep`, `total`, `sort`
— are not R functions here; they live inside the text, where they cannot collide
with anything a reader has attached.
