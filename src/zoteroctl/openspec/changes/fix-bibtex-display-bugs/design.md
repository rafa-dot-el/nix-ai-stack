## Context

Both `zoteroctl/bibtex.py` and `calibrectl/bibtex.py` share the same `generate_cite_key` implementation copied from the original monolith. The function handles up to 26 collisions (a–z) then breaks out of the loop with a fixed string `{base_key}26`, which is never guaranteed to be unique — every subsequent call for that base key returns the same `{base_key}26`. In a real library with 167 NIST tech reports all authored by "Security" in 2024, this produces 140 entries sharing cite key `Security202426`.

The `list-collections --parent` bug is a display-logic mismatch: the data layer returns the correct child collections, but the renderer filters `parentKey is None` to find "top level" items. When `--parent` is specified, every result has a non-None `parentKey`, so the rendered list is always empty.

`calibrectl export-bibtex` was ported from the monolith without adding the `--all` flag that zoteroctl already has.

## Goals / Non-Goals

**Goals:**
- `generate_cite_key` produces a unique key for any number of collisions
- `list-collections --parent KEY` renders all matched children visibly
- `calibrectl export-bibtex --all` exports all 1125 books in one call
- Regression tests cover all three fixes

**Non-Goals:**
- Changing the cite key scheme (AuthorYear remains the format)
- Changing the alphabetic suffix ordering (a–z continues to be used first)
- Adding pagination to export-bibtex
- Changing how `query` handles attachment/note types (that is intentional raw-SQL behaviour)

## Decisions

### Cite key overflow: counter-based infinite loop

Replace the `break` with an indefinitely incrementing counter. After a–z (slots 1–26) use numeric suffixes starting at 27, then 28, etc. No `break` in the loop — only exit is when a unique key is found.

```python
# Before (broken):
suffix_ord = ord("a")
while cite_key in used_keys:
    cite_key = f"{base_key}{chr(suffix_ord)}"
    suffix_ord += 1
    if suffix_ord > ord("z"):
        cite_key = f"{base_key}{suffix_ord - ord('a')}"
        break          # ← always returns same key for entry 27+

# After:
n = 1
while cite_key in used_keys:
    suffix = chr(ord("a") + n - 1) if n <= 26 else str(n)
    cite_key = f"{base_key}{suffix}"
    n += 1
```

Alternatives considered: using a UUID suffix or item key fallback — rejected because those break the human-readable AuthorYear convention.

### list-collections --parent: conditional render path

When `--parent` is provided, skip the tree-nesting logic entirely and render each returned collection as a flat entry. The tree logic is still used for the unfiltered case (no `--parent`).

Alternatives considered: fix the top_level filter to include items whose parentKey matches the requested parent — rejected because the tree logic would then render those as roots and also try to find their children from within the same result set, which would produce duplicates.

### calibrectl export-bibtex --all: reuse list_books_query

Add `--all` flag that calls `list_books_query(conn)` (no filters, no limit) and passes all books through `book_to_bibtex`. The existing `used_keys` set already handles cite key dedup across the full set. No new helpers needed.

## Risks / Trade-offs

- **Counter suffix gap**: switching from `z` → `27` (not `aa`) means keys like `Smith1999z` and `Smith199927` are not intuitively sequential. Acceptable — cite keys are identifiers, not sorted labels.
- **Large export performance**: exporting all 1125 calibre books reads each book's full metadata with N+1 queries. Acceptable for a CLI tool; the existing single-book path has the same pattern.
- **Same bug in calibrectl/bibtex.py**: fix must be applied to both files identically, or a regression will reappear in calibrectl when a user has 27+ books by the same author in the same year.
