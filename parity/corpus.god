sales then take 3
---
sales then keep where [region] is "West"
---
sales then keep where [region] is not "West" then sort [revenue]
---
sales then pick [product, revenue]
---
sales then pick all_but [cost]
---
sales then add [margin] as [revenue] - [cost] then pick [product, margin]
---
sales then summarize [total] as total([revenue]), [orders] as row_count() by [product]
---
sales then summarize [total] as total([revenue]) by [region, product] then sort [total] descending
---
sales then summarize [biggest] as largest([revenue]), [smallest] as smallest([cost])
---
sales then keep where [revenue] > 150 and [cost] < 100
---
sales then keep where [region] in {"West", "East"} then summarize [n] as row_count() by [region]
---
sales then keep where not ([region] is "West") then take 1
---
sales then add [net] as ([revenue] - [cost]) * 2 then sort [net] descending then take 2
---
sales then summarize [m] as average([revenue]) by [product] then keep where [m] > 200
---
sales then sort [revenue] descending, [cost] then take 4
---
sales then join products by [product] then pick [product, maker, revenue] then sort [revenue] descending then take 3
---
sales then join products unmatched "none" then summarize [total] as total([revenue]) by [maker]
---
sales then drop_duplicates then take 3
---
sales then rename [area] as [region] then pick [area, revenue] then sort [revenue] descending then take 2
---
sales then fill_missing [cost] as 0 then summarize [total] as total([cost]) by [region]
---
sales then drop_missing [revenue] then summarize [n] as row_count()
---
sales then sort [region], [revenue] descending then take 1 by [region]
---
sales then keep where matching(products, by [product]) then summarize [total] as total([revenue]) by [product]
---
sales then keep where not matching(products, by [product]) then pick [product, revenue]
---
sales then keep where matching(products) then sort [revenue] descending then take 3
---
sales then add [place] as rank([revenue] descending) by [product] then sort [product], [place]
---
sales then sort [revenue] descending then add [n] as row_number() then pick [product, revenue, n]
---
sales then add [place] as rank([cost]) then sort [place], [product]
---
sales then add [best] as first_present([cost], [revenue]) then pick [product, best]
---
sales then fill_missing [cost] as 0 then add [either] as first_present([cost], [revenue]) then summarize [t] as total([either]) by [region]
---
sales then pick where name starts "re" then take 2
---
sales then keep where [region] starts "W" then summarize [n] as row_count() by [product]
---
sales then keep where [product] contains "adge" or [region] ends "st" then take 3
---
sales then pick where name is "product" or name contains "ven" then take 2
---
sales then add where name is "revenue" or name is "cost" as value * 2 then take 2
---
sales then summarize where name ends "ost" as average(value) by [region]
---
sales then summarize where name is "revenue" as total(value)
---
sales then pick where kind is "number" then take 2
---
sales then summarize where kind is "number" as average(value) by [region]
---
sales then pick where kind is not "number" then take 2
---
sales then pick where lower(name) starts "re" then take 2
---
sales then keep where lower([region]) is "west" then take 2
---
sales then add [shout] as upper([product]) then pick [product, shout] then take 2
---
sales then lengthen [revenue, cost]
---
sales then lengthen [revenue, cost] as name [measure], value [amount] then take 4
---
sales then lengthen all_but [region, product, ordered_on] as name [measure], value [amount] then keep where [measure] is "cost"
---
sales then lengthen where name starts "c" as name [measure], value [amount] then take 3
---
sales then summarize [revenue] as total([revenue]) by [region, product] then widen name [product], value [revenue] by [region] giving [Widget, Gadget, Gizmo]
---
sales then summarize [revenue] as total([revenue]) by [region, product] then widen name [product], value [revenue] by [region] missing 0 giving [Widget, Gadget, Gizmo] then add [gap] as [Widget] - [Gadget]
---
sales then pick [region, product, revenue] then widen name [product], value average([revenue]) by [region] giving [Widget, Gadget, Gizmo]
---
sales then add [size] as when([revenue] >= 300, "big", [revenue] >= 150, "medium", otherwise "small") then pick [product, revenue, size] then take 4
---
sales then add [flag] as when([cost] > 90, "high") then pick [product, cost, flag] then take 3
---
sales then summarize [n] as row_count() by [region] then add [busy] as when([n] > 3, yes, otherwise no)
---
sales then add [tidy] as trim([product]), [size] as characters([product]) then pick [product, tidy, size] then take 3
---
sales then add [head] as split_text([product], "d", 1), [swapped] as replace_text([product], "get", "GET") then pick [product, head, swapped] then take 3
---
sales then keep where between([revenue], 150, 400) then sort [revenue] then pick [product, revenue]
---
sales then add [as_text] as to_text([revenue]), [whole] as to_whole([cost]) then pick [as_text, whole] then take 3
---
sales then add [d] as to_date([ordered_on]) then add [y] as year([d]), [m] as month([d]), [dd] as day([d]), [wd] as weekday([d]) then pick [ordered_on, y, m, dd, wd] then sort [ordered_on]
---
sales then sort [revenue] then add [so_far] as running_total([revenue]) then pick [revenue, so_far]
---
sales then sort [ordered_on] then add [so_far] as running_total([revenue]), [before] as previous([revenue]), [after] as following([revenue]) by [region] then pick [region, ordered_on, so_far, before, after]

---
sales then add [shout] as upper(trim([product])), [quiet] as lower(trim([product])) then pick [product, shout, quiet] then take 2
