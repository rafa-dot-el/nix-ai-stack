## 1. Fix cite key overflow in zoteroctl

- [x] 1.1 Rewrite `generate_cite_key` in `src/zoteroctl/zoteroctl/bibtex.py`: replace the `break`-on-overflow logic with an infinite counter (a–z then 27, 28, …)
- [x] 1.2 Add regression test to `src/zoteroctl/tests/test_bibtex.py` for 27th collision (returns `{base}27`) and 140th collision (returns `{base}140`)

## 2. Fix cite key overflow in calibrectl

- [x] 2.1 Apply the identical fix to `src/calibrectl/calibrectl/bibtex.py`
- [x] 2.2 Add the same regression tests to `src/calibrectl/tests/test_bibtex.py`

## 3. Fix list-collections --parent display

- [x] 3.1 In `src/zoteroctl/zoteroctl/cli.py` `list_collections` command: when `parent` argument is provided, render all returned collections as a flat list instead of the tree-nesting path
- [x] 3.2 Add test to `src/zoteroctl/tests/test_cli.py` confirming `list-collections --parent KEY` with children produces visible output

## 4. Add calibrectl export-bibtex --all

- [x] 4.1 Add `--all` flag to `export_bibtex` command in `src/calibrectl/calibrectl/cli.py`; when set, call `list_books_query(conn)` (no filters) and export all books
- [x] 4.2 Update error message from "Specify BOOK_ID or --tag" to "Specify BOOK_ID, --tag, or --all"
- [x] 4.3 Add smoke test to `src/calibrectl/tests/test_cli.py` for `export-bibtex --all` producing entries for all books in the fixture DB

## 5. Verify and commit

- [x] 5.1 Run `task test` in both `src/zoteroctl/` and `src/calibrectl/` — all pass, coverage gates hold
- [x] 5.2 Smoke test: `uv run zoteroctl export --all | grep "^@" | sort | uniq -d` → empty output (no duplicates)
- [x] 5.3 Smoke test: `uv run zoteroctl list-collections --parent PMMRHFTH` → 11 collections rendered
- [x] 5.4 Smoke test: `uv run calibrectl export-bibtex --all | grep "^@" | wc -l` → 1125
- [x] 5.5 Commit all changes with GPG signature
