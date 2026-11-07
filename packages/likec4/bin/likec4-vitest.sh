#!@shell@
# likec4-vitest — runs a project's LikeC4 guardrail suite with the vitest and
# likec4 pinned in this package; no `npm install` in the project.
#
# Test files import the generated model (`likec4 gen model`), which imports
# `likec4/model`, and the config imports `vitest/config`; both resolve from
# ./node_modules. This script maintains ./node_modules as a REAL directory of
# symlinks into the store (so Vite/vitest can still write their cache in
# ./node_modules/.vite) and marks it as managed. It refuses to touch a
# node_modules it did not create. Add `node_modules/` to .gitignore.
set -eu
unset OPENAI_API_KEY GEMINI_API_KEY ANTHROPIC_API_KEY GOOGLE_API_KEY

lib="@lib@/node_modules"
marker="node_modules/.likec4-nix"

if [ -e node_modules ] && [ ! -e "$marker" ]; then
  echo "[-] likec4-vitest: ./node_modules exists and is not managed by likec4-nix; refusing to modify it" >&2
  exit 65
fi

if [ ! -e "$marker" ] || [ "$(cat "$marker")" != "$lib" ]; then
  mkdir -p node_modules
  find node_modules -mindepth 1 -maxdepth 2 -type l -delete
  for entry in "$lib"/* "$lib"/.bin; do
    name="$(basename "$entry")"
    case "$name" in
      @*)
        mkdir -p "node_modules/$name"
        for scoped in "$entry"/*; do
          ln -sfn "$scoped" "node_modules/$name/$(basename "$scoped")"
        done
        ;;
      *) ln -sfn "$entry" "node_modules/$name" ;;
    esac
  done
  printf '%s' "$lib" > "$marker"
fi

exec "@node@" "$lib/vitest/vitest.mjs" "$@"
