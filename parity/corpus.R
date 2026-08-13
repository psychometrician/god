# The corpus, written in R.
#
# **Sentence for sentence with `corpus.god`, in the same order.** Each one here
# must mean exactly what the same-numbered one there means, and the harness
# checks the two files are the same length so that adding to one and forgetting
# the other fails loudly rather than quietly testing fewer sentences.
#
# This file is what makes the third witness possible. The text form and the R
# form reach the grammar by different routes — one is parsed, the other is built
# — so agreement between them is evidence rather than tautology.

sales |> take(3)
---
sales |> keep(region == "West")
---
sales |> keep(region != "West") |> sort(revenue)
---
sales |> pick(product, revenue)
---
sales |> pick(all_but(cost))
---
sales |> add(margin = revenue - cost) |> pick(product, margin)
---
sales |> summarize(total = total(revenue), orders = row_count(), by = product)
---
sales |>
  summarize(total = total(revenue), by = c(region, product)) |>
  sort(descending(total))
---
sales |> summarize(biggest = largest(revenue), smallest = smallest(cost))
---
sales |> keep(revenue > 150 & cost < 100)
---
sales |> keep(region %in% c("West", "East")) |> summarize(n = row_count(), by = region)
---
sales |> keep(!(region == "West")) |> take(1)
---
sales |> add(net = (revenue - cost) * 2) |> sort(descending(net)) |> take(2)
---
sales |> summarize(m = average(revenue), by = product) |> keep(m > 200)
---
sales |> sort(descending(revenue), cost) |> take(4)
---
sales |>
  join(products, by = product) |>
  pick(product, maker, revenue) |>
  sort(descending(revenue)) |>
  take(3)
---
sales |>
  join(products, unmatched = "none") |>
  summarize(total = total(revenue), by = maker)
---
sales |> drop_duplicates() |> take(3)
---
sales |>
  rename(area = region) |>
  pick(area, revenue) |>
  sort(descending(revenue)) |>
  take(2)
---
sales |> fill_missing(cost = 0) |> summarize(total = total(cost), by = region)
---
sales |> drop_missing(revenue) |> summarize(n = row_count())
---
sales |> sort(region, descending(revenue)) |> take(1, by = region)
---
sales |>
  keep(matching(products, by = product)) |>
  summarize(total = total(revenue), by = product)
---
sales |> keep(!matching(products, by = product)) |> pick(product, revenue)
---
sales |> keep(matching(products)) |> sort(descending(revenue)) |> take(3)
---
sales |>
  add(place = rank(descending(revenue)), by = product) |>
  sort(product, place)
---
sales |>
  sort(descending(revenue)) |>
  add(n = row_number()) |>
  pick(product, revenue, n)
---
sales |> add(place = rank(cost)) |> sort(place, product)
---
sales |> add(best = first_present(cost, revenue)) |> pick(product, best)
---
sales |>
  fill_missing(cost = 0) |>
  add(either = first_present(cost, revenue)) |>
  summarize(t = total(either), by = region)
---
sales |> pick(where(startsWith(name, "re"))) |> take(2)
---
sales |>
  keep(startsWith(region, "W")) |>
  summarize(n = row_count(), by = product)
---
sales |> keep(grepl("adge", product, fixed = TRUE) | endsWith(region, "st")) |> take(3)
---
sales |> pick(where(name == "product" | grepl("ven", name, fixed = TRUE))) |> take(2)
---
sales |> add(where(name == "revenue" | name == "cost", value * 2)) |> take(2)
---
sales |> summarize(where(endsWith(name, "ost"), average(value)), by = region)
---
sales |> summarize(where(name == "revenue", total(value)))
---
sales |> pick(where(kind == "number")) |> take(2)
---
sales |> summarize(where(kind == "number", average(value)), by = region)
---
sales |> pick(where(kind != "number")) |> take(2)
---
sales |> pick(where(startsWith(tolower(name), "re"))) |> take(2)
---
sales |> keep(tolower(region) == "west") |> take(2)
---
sales |> add(shout = toupper(product)) |> pick(product, shout) |> take(2)
---
sales |> lengthen(revenue, cost)
---
sales |> lengthen(revenue, cost, name = measure, value = amount) |> take(4)
---
sales |> lengthen(all_but(region, product, ordered_on), name = measure, value = amount) |> keep(measure == "cost")
---
sales |> lengthen(where(startsWith(name, "c")), name = measure, value = amount) |> take(3)
---
sales |> summarize(revenue = total(revenue), by = c(region, product)) |> widen(name = product, value = revenue, by = region, giving = c(Widget, Gadget, Gizmo))
---
sales |> summarize(revenue = total(revenue), by = c(region, product)) |> widen(name = product, value = revenue, by = region, missing = 0, giving = c(Widget, Gadget, Gizmo)) |> add(gap = Widget - Gadget)
---
sales |> pick(region, product, revenue) |> widen(name = product, value = average(revenue), by = region, giving = c(Widget, Gadget, Gizmo))
---
sales |> add(size = when(revenue >= 300, "big", revenue >= 150, "medium", otherwise = "small")) |> pick(product, revenue, size) |> take(4)
---
sales |> add(flag = when(cost > 90, "high")) |> pick(product, cost, flag) |> take(3)
---
sales |> summarize(n = row_count(), by = region) |> add(busy = when(n > 3, TRUE, otherwise = FALSE))
---
sales |> add(tidy = trim(product), size = characters(product)) |> pick(product, tidy, size) |> take(3)
---
sales |> add(head = split_text(product, "d", 1), swapped = replace_text(product, "get", "GET")) |> pick(product, head, swapped) |> take(3)
---
sales |> keep(between(revenue, 150, 400)) |> sort(revenue) |> pick(product, revenue)
---
sales |> add(as_text = to_text(revenue), whole = to_whole(cost)) |> pick(as_text, whole) |> take(3)
---
sales |> add(d = to_date(ordered_on)) |> add(y = year(d), m = month(d), dd = day(d), wd = weekday(d)) |> pick(ordered_on, y, m, dd, wd) |> sort(ordered_on)
---
sales |> sort(revenue) |> add(so_far = running_total(revenue)) |> pick(revenue, so_far)
---
sales |> sort(ordered_on) |> add(so_far = running_total(revenue), before = previous(revenue), after = following(revenue), by = region) |> pick(region, ordered_on, so_far, before, after)

---
sales |> add(shout = upper(trim(product)), quiet = lower(trim(product))) |> pick(product, shout, quiet) |> take(2)
---
sales |> add_rows(sales) |> summarize(rows = row_count(), total = total(revenue))
---
sales |> sort(ordered_on) |> summarize(earliest = first(product), latest = last(product), by = region)
---
sales |> summarize(mid = median(revenue), by = region)
---
sales |> summarize(kinds = unique_count(product), by = region)
---
sales |> add(as_text = to_text(revenue)) |> add(back = to_number(as_text)) |> summarize(t = total(back))
---
sales |> add(d = to_date(ordered_on)) |> add(h = hour(d)) |> pick(ordered_on, h) |> sort(ordered_on)
---
sales |> add(share = revenue / total(revenue)) |> sort(descending(share), ordered_on) |> pick(ordered_on, product, share) |> take(3)
---
sales |> add(share = revenue / total(revenue), by = region) |> keep(share > 0.35) |> sort(region, descending(share)) |> pick(region, product, share)
---
sales |> add(label = join_text(region, " ", product)) |> pick(label, revenue) |> sort(label) |> take(3)
---
sales |> add(key = join_text(region, "-", to_text(revenue))) |> keep(grepl("West", key, fixed = TRUE)) |> pick(key)
---
sales |> add_combinations(region, product) |> fill_missing(revenue = 0) |> summarize(rows = row_count(), total = total(revenue), by = region)
---
sales |> add(tier = when(revenue > 250, "high", otherwise = "low")) |> add_combinations(region, product, by = tier) |> summarize(rows = row_count(), by = tier)
---
sales |> sort(revenue) |> take_last(3) |> pick(product, revenue)
---
sales |> sort(revenue) |> take_last(1, by = region) |> pick(region, product, revenue)
