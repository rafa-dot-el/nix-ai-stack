## Why

Live testing of zoteroctl and calibrectl against real databases revealed three bugs: BibTeX export silently produces 140 duplicate cite keys for libraries with more than 26 works by the same author in the same year; `list-collections --parent` fetches the right data but renders nothing; and calibrectl has no way to export the whole library to BibTeX.

## What Changes

- Fix `generate_cite_key` in `zoteroctl/bibtex.py` and `calibrectl/bibtex.py`: replace the broken overflow logic (always returns `{base_key}26` after slot z) with a counter that keeps incrementing indefinitely
- Fix `list-collections --parent` display in `zoteroctl/cli.py`: when `--parent` is given, all results have a non-None `parentKey`, so the current tree-render loop renders nothing; render as flat list in that case
- Add `--all` flag to `calibrectl export-bibtex` so the entire library can be exported without needing a tag filter

## Capabilities

### New Capabilities

- `bibtex-cite-key-dedup`: Unique cite key generation that handles more than 26 collisions per base key
- `export-bibtex-all`: Export entire Calibre library to BibTeX in a single command

### Modified Capabilities

- `list-collections-parent-display`: Fix the `--parent` filter display so matched children are rendered

## Impact

- `src/zoteroctl/zoteroctl/bibtex.py` — `generate_cite_key` function
- `src/calibrectl/calibrectl/bibtex.py` — `generate_cite_key` function (same bug, same fix)
- `src/zoteroctl/zoteroctl/cli.py` — `list_collections` command display block
- `src/calibrectl/calibrectl/cli.py` — `export_bibtex` command: add `--all` option + `list_books_query` call
- `src/zoteroctl/tests/test_bibtex.py` — add overflow regression test
- `src/calibrectl/tests/test_bibtex.py` — add overflow regression test
- `src/calibrectl/tests/test_cli.py` — add export-bibtex --all smoke test
