-- A package name is set in code font, in every chapter and every appendix.
--
-- The source writes these names as plain words and nothing in a `.qmd` changes:
-- a writer keeps typing `god` in a sentence and this filter decides the type.
-- Spelling and type are separate decisions, and only the second one lives here.
--
-- **Why a filter, and not three hundred pairs of backticks.** Measured across
-- the book's 51 source files, counting prose only and ignoring what is already
-- inside a chunk or a pair of backticks: `god` appears 55 times across 15 files,
-- `dplyr` 35 across 17, `pandas` 29 across 12, `polars` 16 across 7. Marking
-- those by hand puts the convention in the hands of whoever writes the next
-- sentence, which is how a convention drifts by chapter — a writer holds one
-- file in their head for an afternoon and the one before it fades. A filter
-- cannot drift, an appendix cannot be missed, and a chapter written a year from
-- now is covered without anyone remembering that this rule exists.
--
-- `Code` is the form the book already agrees about: `<code>` in HTML. This book
-- renders HTML and nothing else, so there is one definition of the rule and one
-- place it is wired.

-- Libraries, not applications, and not products. The line is one question: could
-- a sentence hand this to `library()` or to `import`? Then it is code.
--
-- **DuckDB, PySpark, Ibis and Arrow are deliberately absent, and they are the
-- interesting half of the list.** The book writes each of them capitalized,
-- because in prose they are the product rather than the library, and a reader
-- who typed what the code font told them to type would get it wrong: the
-- importable names are `duckdb`, `pyspark`, `ibis` and `arrow`. Those spellings
-- do appear in the book — inside chunks, which this filter never walks.
--
-- Longest first, so a name can never match inside a longer one.
local NAMES = {
  "data.table", "dbplyr", "polars", "pandas", "dplyr", "tidyr", "god", "gog",
}

-- What the boundary test is protecting, all of it real text in this book:
-- `god-cli` and `god-core`, which are crate names; `GOD_CLI`, which survives on
-- case alone; `god_sql` and `god_pipeline`, where an underscore continues the
-- word; and the `/god` that ends a repository path. A name may still be followed
-- by ordinary punctuation, so `god.` at the end of a sentence and `dplyr's` both
-- match, while `god.dev` does not.
--
-- The classes are written out as ASCII ranges rather than as `%w`, and that is
-- not style. Lua matches bytes, and pandoc has already turned `'` into a curly
-- quote by the time a filter sees it, so `dplyr's` ends in the first byte of a
-- three-byte character. Under `%w` that byte reads as a letter and the name goes
-- unmarked, which is the sort of thing that shows up as one plain word in a
-- chapter nobody rereads.
local function boundary_ok(s, i, j)
  local before = i > 1 and s:sub(i - 1, i - 1) or ""
  local after = s:sub(j + 1, j + 1)
  if before ~= "" and before:match("[A-Za-z0-9_/%-%.]") then return false end
  if after ~= "" and after:match("[A-Za-z0-9_/%-]") then return false end
  if after == "." and s:sub(j + 2, j + 2):match("[A-Za-z0-9]") then return false end
  return true
end

-- Pandoc splits text on whitespace, so one `Str` is `god,` or `(god)` or
-- `dplyr's`. Each one is walked character by character and rebuilt as a run of
-- inlines, which is the only way to reach a name with punctuation stuck to it.
local function split(s)
  local out, buf, i, hit_any = {}, {}, 1, false
  while i <= #s do
    local hit = nil
    for _, name in ipairs(NAMES) do
      local j = i + #name - 1
      if s:sub(i, j) == name and boundary_ok(s, i, j) then
        hit = name
        break
      end
    end
    if hit then
      if #buf > 0 then
        out[#out + 1] = pandoc.Str(table.concat(buf))
        buf = {}
      end
      out[#out + 1] = pandoc.Code(hit)
      hit_any = true
      i = i + #hit
    else
      buf[#buf + 1] = s:sub(i, i)
      i = i + 1
    end
  end
  if #buf > 0 then out[#out + 1] = pandoc.Str(table.concat(buf)) end
  if not hit_any then return nil end
  return out
end

return {
  {
    -- Top-down, because a node has to be refused *before* its children are
    -- reached. Bottom-up would rewrite the text first and hand back an element
    -- that had already lost.
    traverse = "topdown",

    -- Already code. A chunk's source and its output never reach a `Str` filter
    -- at all, so this is only for the inline spans a writer marked by hand.
    Code = function(el) return el, false end,

    -- **There is deliberately no exemption here.** The sibling book's filter
    -- exempts its slogan, because that slogan contains the package name and
    -- setting half of it in code font would break the rhyme the line rests on.
    -- `.god-motto` is "Say it once. Run it anywhere.", which names nothing, so
    -- an exemption would guard a case that cannot arise. Add one if the motto
    -- ever takes the name.

    Str = function(el)
      local out = split(el.text)
      if out == nil then return nil end
      -- `false` stops the walk from re-entering what was just built.
      return out, false
    end,
  },
}
