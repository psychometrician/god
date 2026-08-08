# The corpus, written in Python.
#
# **Sentence for sentence with `corpus.god` and `corpus.R`, in the same order.**
# Each one here must mean exactly what the same-numbered one in the other two
# means, and the harness checks all three files are the same length so that
# adding to one and forgetting the others fails loudly rather than quietly
# testing fewer sentences.
#
# The three reach the grammar by different routes: the text is parsed, and each
# native form is built. None can borrow another's answer, so agreement between
# them is evidence rather than tautology.
#
# Note the two differences from the R file, and that there are only two: the pipe
# is `>>` rather than `|>`, and a column is `col.name` rather than a bare name.
# Python adds two rules of its own that come from the language: `&` binds more
# tightly than `==`, so comparisons joined by it need parentheses, and negation
# is `~`, because `not` cannot be overloaded.

sales >> take(3)
---
sales >> keep(col.region == "West")
---
sales >> keep(col.region != "West") >> sort(col.revenue)
---
sales >> pick(col.product, col.revenue)
---
sales >> pick(all_but(col.cost))
---
sales >> add(margin = col.revenue - col.cost) >> pick(col.product, col.margin)
---
sales >> summarize(total = total(col.revenue), orders = row_count(), by = col.product)
---
(sales
  >> summarize(total = total(col.revenue), by = [col.region, col.product])
  >> sort(descending(col.total)))
---
sales >> summarize(biggest = largest(col.revenue), smallest = smallest(col.cost))
---
sales >> keep((col.revenue > 150) & (col.cost < 100))
---
sales >> keep(col.region.is_in(["West", "East"])) >> summarize(n = row_count(), by = col.region)
---
sales >> keep(~(col.region == "West")) >> take(1)
---
sales >> add(net = (col.revenue - col.cost) * 2) >> sort(descending(col.net)) >> take(2)
---
sales >> summarize(m = average(col.revenue), by = col.product) >> keep(col.m > 200)
---
sales >> sort(descending(col.revenue), col.cost) >> take(4)
---
(sales
  >> join(products, by = col.product)
  >> pick(col.product, col.maker, col.revenue)
  >> sort(descending(col.revenue))
  >> take(3))
---
(sales
  >> join(products, unmatched = "none")
  >> summarize(total = total(col.revenue), by = col.maker))
---
sales >> drop_duplicates() >> take(3)
---
(sales
  >> rename(area = col.region)
  >> pick(col.area, col.revenue)
  >> sort(descending(col.revenue))
  >> take(2))
---
sales >> fill_missing(cost = 0) >> summarize(total = total(col.cost), by = col.region)
---
sales >> drop_missing(col.revenue) >> summarize(n = row_count())
---
sales >> sort(col.region, descending(col.revenue)) >> take(1, by = col.region)
---
(sales
  >> keep(matching(products, by = col.product))
  >> summarize(total = total(col.revenue), by = col.product))
---
sales >> keep(~matching(products, by = col.product)) >> pick(col.product, col.revenue)
---
sales >> keep(matching(products)) >> sort(descending(col.revenue)) >> take(3)
---
(sales
  >> add(place = rank(descending(col.revenue)), by = col.product)
  >> sort(col.product, col.place))
---
(sales
  >> sort(descending(col.revenue))
  >> add(n = row_number())
  >> pick(col.product, col.revenue, col.n))
---
sales >> add(place = rank(col.cost)) >> sort(col.place, col.product)
---
sales >> add(best = first_present(col.cost, col.revenue)) >> pick(col.product, col.best)
---
(sales
  >> fill_missing(cost = 0)
  >> add(either = first_present(col.cost, col.revenue))
  >> summarize(t = total(col.either), by = col.region))
---
sales >> pick(where(name.starts("re"))) >> take(2)
---
(sales
  >> keep(col.region.starts("W"))
  >> summarize(n = row_count(), by = col.product))
---
sales >> keep(col.product.contains("adge") | col.region.ends("st")) >> take(3)
---
sales >> pick(where((name == "product") | name.contains("ven"))) >> take(2)
---
sales >> add(where((name == "revenue") | (name == "cost"), value * 2)) >> take(2)
---
sales >> summarize(where(name.ends("ost"), average(value)), by = col.region)
---
sales >> summarize(where(name == "revenue", total(value)))
---
sales >> pick(where(kind == "number")) >> take(2)
---
sales >> summarize(where(kind == "number", average(value)), by = col.region)
---
sales >> pick(where(kind != "number")) >> take(2)
---
sales >> pick(where(lower(name).starts("re"))) >> take(2)
---
sales >> keep(lower(col.region) == "west") >> take(2)
---
sales >> add(shout = upper(col.product)) >> pick(col.product, col.shout) >> take(2)
---
sales >> lengthen(col.revenue, col.cost)
---
sales >> lengthen(col.revenue, col.cost, name = col.measure, value = col.amount) >> take(4)
---
sales >> lengthen(all_but(col.region, col.product, col.ordered_on), name = col.measure, value = col.amount) >> keep(col.measure == "cost")
---
sales >> lengthen(where(name.starts("c")), name = col.measure, value = col.amount) >> take(3)
---
sales >> summarize(revenue = total(col.revenue), by = [col.region, col.product]) >> widen(name = col.product, value = col.revenue, by = col.region, giving = [col.Widget, col.Gadget, col.Gizmo])
---
sales >> summarize(revenue = total(col.revenue), by = [col.region, col.product]) >> widen(name = col.product, value = col.revenue, by = col.region, missing = 0, giving = [col.Widget, col.Gadget, col.Gizmo]) >> add(gap = col.Widget - col.Gadget)
---
sales >> pick(col.region, col.product, col.revenue) >> widen(name = col.product, value = average(col.revenue), by = col.region, giving = [col.Widget, col.Gadget, col.Gizmo])
---
sales >> add(size = when(col.revenue >= 300, "big", col.revenue >= 150, "medium", otherwise = "small")) >> pick(col.product, col.revenue, col.size) >> take(4)
---
sales >> add(flag = when(col.cost > 90, "high")) >> pick(col.product, col.cost, col.flag) >> take(3)
---
sales >> summarize(n = row_count(), by = col.region) >> add(busy = when(col.n > 3, True, otherwise = False))
---
sales >> add(tidy = trim(col.product), size = characters(col.product)) >> pick(col.product, col.tidy, col.size) >> take(3)
---
sales >> add(head = split_text(col.product, "d", 1), swapped = replace_text(col.product, "get", "GET")) >> pick(col.product, col.head, col.swapped) >> take(3)
---
sales >> keep(between(col.revenue, 150, 400)) >> sort(col.revenue) >> pick(col.product, col.revenue)
---
sales >> add(as_text = to_text(col.revenue), whole = to_whole(col.cost)) >> pick(col.as_text, col.whole) >> take(3)
---
sales >> add(d = to_date(col.ordered_on)) >> add(y = year(col.d), m = month(col.d), dd = day(col.d), wd = weekday(col.d)) >> pick(col.ordered_on, col.y, col.m, col.dd, col.wd) >> sort(col.ordered_on)
---
sales >> sort(col.revenue) >> add(so_far = running_total(col.revenue)) >> pick(col.revenue, col.so_far)
---
sales >> sort(col.ordered_on) >> add(so_far = running_total(col.revenue), before = previous(col.revenue), after = following(col.revenue), by = col.region) >> pick(col.region, col.ordered_on, col.so_far, col.before, col.after)

---
sales >> add(shout = upper(trim(col.product)), quiet = lower(trim(col.product))) >> pick(col.product, col.shout, col.quiet) >> take(2)
