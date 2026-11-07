#!@shell@
# likec4 — pinned CLI wrapper (nix package packages/likec4).
#
# Every guard lives here, so no project Taskfile has to remember it:
#  - `publish` is refused: it ships the model (hostnames, topology, account
#    ids) to LikeC4 Cloud.
#  - AI provider keys are stripped: the dev server detects them and attempts
#    a runtime `npm install` of a provider SDK.
#  - Vite's cache is redirected out of the read-only nix store. The CLI builds
#    its Vite config with root = the app inside this package, so the default
#    cache (node_modules/.vite) would be in /nix/store and the dependency
#    optimizer fails (the dev server never serves react). The CLI is patched
#    at build time to read LIKEC4_VITE_CACHE_DIR (default.nix).
#  - Playwright uses the browsers from nixpkgs (no runtime download); the
#    npm Playwright version is pinned to the same version (package.json
#    overrides, asserted in default.nix).
set -eu

case "${1:-}" in
  publish)
    echo "likec4: 'publish' is disabled in this package (the model must not leave the machine)" >&2
    exit 64
    ;;
esac

unset OPENAI_API_KEY GEMINI_API_KEY ANTHROPIC_API_KEY GOOGLE_API_KEY

: "${LIKEC4_VITE_CACHE_DIR:=${XDG_CACHE_HOME:-${HOME:-/tmp}/.cache}/likec4/@version@/vite}"
export LIKEC4_VITE_CACHE_DIR
mkdir -p "$LIKEC4_VITE_CACHE_DIR"

export PLAYWRIGHT_BROWSERS_PATH="@browsers@"
export PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1
export PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS=true
export PATH="@graphviz@/bin:$PATH"

exec "@node@" "@lib@/node_modules/likec4/bin/likec4.mjs" "$@"
