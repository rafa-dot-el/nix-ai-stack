{ lib
, buildNpmPackage
, nodejs_24
, graphviz
, playwright-driver
, runtimeShell
, runCommand
, curl
, jq
, yq-go
, git
, python3
, emacs-nox
, makeFontsConf
, dejavu_fonts
, makeWrapper
}:

# LikeC4 — architecture as code (https://likec4.dev), packaged complete:
#
#   bin/likec4         CLI wrapper: publish refused, AI keys stripped, Vite cache
#                      outside the store, Playwright browsers from nixpkgs
#   bin/likec4-mcp     MCP server over stdio (for .mcp.json); --repo PATH is
#                      relative to the git root of the current directory
#   bin/claude-mcp-install-likec4
#                      register it for the current project in Claude Code
#                      (<repo>/.mcp.json, project scope, repo-relative)
#   bin/hermes-mcp-install-likec4
#                      register it in a Hermes profile (no project scope in
#                      Hermes; additions-only ruamel.yaml edit of config.yaml)
#   bin/likec4-vitest  guardrail test runner (vitest + likec4 model API, pinned)
#   bin/likec4-validate-hook
#                      Claude Code PostToolUse hook: validate the workspace of
#                      an edited .c4 file, errors back to Claude (exit 2)
#   share/emacs/site-lisp/likec4-mode.el
#                      major mode + LSP registration (eglot and lsp-mode)
#
# passthru.tests.smoke exercises every one of them against tests/fixture.

let
  version = "1.60.0";

  # likec4 declares engines.node >= 22.22.3. buildNpmPackage does NOT enforce
  # "engines" -- it builds silently against a too-old Node. Asserted at eval
  # time. nodejs_24 satisfies it; nixpkgs 25.11 nodejs_22 (22.22.2) does not.
  requiredNode = "22.22.3";
  nodejs = nodejs_24;

  # Playwright: the npm package and the browsers must be the same release or
  # Playwright looks for browser revisions that do not exist. The browsers come
  # from nixpkgs (no runtime download); package.json `overrides` pins the npm
  # side to the same version. A nixpkgs bump that changes playwright-driver
  # fails here until package.json is updated and the lock regenerated.
  lock = lib.importJSON ./package-lock.json;
  lockedPlaywright =
    (lock.packages."node_modules/likec4/node_modules/playwright-core"
      or lock.packages."node_modules/playwright-core").version;

  browsers = playwright-driver.browsers.override {
    withFirefox = false;
    withWebkit = false;
    withChromiumHeadlessShell = true;
  };

  # Round-trip YAML editor for the tracked Hermes configs (additions-only edits).
  hermesConfigPython = python3.withPackages (ps: [ ps.ruamel-yaml ]);

  assertions = [
    (lib.assertMsg (lib.versionAtLeast nodejs.version requiredNode)
      "likec4 requires node >= ${requiredNode}, but the pinned nodejs is ${nodejs.version}; bump the nodejs attribute in default.nix")
    (lib.assertMsg (lockedPlaywright == playwright-driver.version)
      ("package-lock.json pins playwright-core ${lockedPlaywright} but nixpkgs provides "
        + "playwright-driver ${playwright-driver.version}; set package.json overrides.playwright "
        + "and overrides.playwright-core to ${playwright-driver.version} and run `task lock`"))
  ];

  hardening = import ../../lib/hardening.nix { inherit lib; };

  likec4 = assert lib.all (a: a) assertions; buildNpmPackage (finalAttrs: {
    pname = "likec4";
    inherit version nodejs;
    src = lib.fileset.toSource {
      root = ./.;
      fileset = lib.fileset.unions [
        ./package.json
        ./package-lock.json
        ./bin
        ./emacs
      ];
    };

    npmDepsHash = "sha256-yo3HKCZi3taJcJbt4FHExEk927FidBuNHFcp2V5mc0I=";

    # Nothing to compile -- this directory pins the CLI and its test runner.
    dontNpmBuild = true;

    # Never let Playwright's install script fetch browsers during the build.
    env.PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD = "1";

    installPhase = ''
      runHook preInstall

      mkdir -p $out/lib $out/bin $out/share/emacs/site-lisp
      cp -r node_modules $out/lib/
      cp package.json $out/lib/

      # Redirect Vite's cache out of the read-only store. The CLI passes a
      # Vite config with root = the bundled app and configFile:false, so the
      # default cacheDir resolves inside $out. Each of its three Vite configs
      # (app / webcomponent / react; the dev server spreads the app config)
      # gets cacheDir from LIKEC4_VITE_CACHE_DIR, set by bin/likec4.
      cli=$out/lib/node_modules/likec4/dist/cli/index.mjs
      n=$(grep -ao 'configFile:!1' "$cli" | wc -l)
      if [ "$n" -ne 3 ]; then
        echo "likec4: expected 3 Vite configs to patch in $cli, found $n (upstream changed; revisit the patch)" >&2
        exit 1
      fi
      # sed, not substituteInPlace: the bundle contains NUL bytes, which
      # substituteInPlace refuses. The count check below replaces --replace-fail.
      sed -i 's/configFile:!1/configFile:!1,cacheDir:process.env.LIKEC4_VITE_CACHE_DIR||void 0/g' "$cli"
      patched=$(grep -ao 'cacheDir:process.env.LIKEC4_VITE_CACHE_DIR' "$cli" | wc -l)
      if [ "$patched" -ne 3 ]; then
        echo "likec4: Vite cacheDir patch applied $patched times, expected 3" >&2
        exit 1
      fi

      for script in likec4 likec4-mcp likec4-vitest claude-mcp-install-likec4 likec4-validate-hook; do
        substitute bin/$script.sh $out/bin/$script \
          --subst-var-by shell ${runtimeShell} \
          --subst-var-by node ${nodejs}/bin/node \
          --subst-var-by lib $out/lib \
          --subst-var-by out $out \
          --subst-var-by version ${version} \
          --subst-var-by browsers ${browsers} \
          --subst-var-by graphviz ${graphviz} \
          --subst-var-by git ${git}/bin/git \
          --subst-var-by jq ${jq}/bin/jq
        chmod +x $out/bin/$script
      done

      substitute bin/hermes-mcp-install-likec4.py $out/bin/hermes-mcp-install-likec4 \
        --subst-var-by python ${hermesConfigPython}/bin/python3
      chmod +x $out/bin/hermes-mcp-install-likec4

      substitute emacs/likec4-mode.el $out/share/emacs/site-lisp/likec4-mode.el \
        --subst-var-by likec4 $out/bin/likec4

      runHook postInstall
    '';

    # No npm update check (lib/hardening.nix); the scripts in bin/ are wrapped as they are.
    nativeBuildInputs = [ makeWrapper ];
    postFixup = ''
      for bin in likec4 likec4-mcp; do
        wrapProgram $out/bin/$bin ${hardening.toWrapperArgsString hardening.env.likec4}
      done
    '';

    passthru = {
      inherit browsers;
      hardeningEnv = hardening.env.likec4;
      tests.smoke = runCommand "likec4-smoke-test"
        {
          nativeBuildInputs = [ finalAttrs.finalPackage curl jq yq-go git emacs-nox ];
          FONTCONFIG_FILE = makeFontsConf { fontDirectories = [ dejavu_fonts ]; };
        } ''
        set -euo pipefail
        export HOME=$TMPDIR XDG_CACHE_HOME=$TMPDIR/cache
        cp -r ${./tests/fixture} ws
        chmod -R u+w ws
        cd ws

        echo "[*] 1 validate"
        likec4 validate .

        echo "[*] 2 publish is refused (exit 64)"
        rc=0; likec4 publish . || rc=$?
        [ "$rc" -eq 64 ]

        echo "[*] 3 static build"
        likec4 build -o site . >/dev/null
        test -s site/index.html

        echo "[*] 4 dev server serves its pre-bundled dependencies (Vite cache outside the store)"
        likec4 start --listen 127.0.0.1 --port 5199 . > dev.log 2>&1 &
        dev=$!
        for _ in $(seq 1 60); do
          curl -sf -o /dev/null http://127.0.0.1:5199/ && break
          sleep 1
        done
        trap 'echo "[-] dev server log:"; cat dev.log' ERR
        main=$(curl -sf --max-time 120 http://127.0.0.1:5199/src/main)
        # The optimized deps must be served from the user cache, not the store.
        dep=$(printf '%s' "$main" | grep -oE "/@fs$XDG_CACHE_HOME/likec4/${version}/vite/deps/react\.js[^\"]*" | head -1)
        test -n "$dep"
        curl -sf --max-time 120 -o dep.js "http://127.0.0.1:5199$dep"
        test -s dep.js
        if grep -q 'EROFS\|ENOENT.*\.vite' dev.log; then cat dev.log; exit 1; fi
        kill $dev
        trap - ERR

        echo "[*] 5 PNG export with the nixpkgs browsers"
        likec4 export png -o png -f index . >/dev/null
        find png -name '*.png' | grep -q .

        echo "[*] 6 MCP server answers initialize"
        printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
          | timeout 120 likec4-mcp . | head -n 1 | jq -e '.result.serverInfo.name' >/dev/null

        echo "[*] 7 guardrail runner: generated model + vitest from the package"
        likec4 gen model . -o test/__generated__/model.ts --skip-layout >/dev/null
        likec4-vitest run

        echo "[*] 8 Emacs major mode loads and points at this package's LSP"
        emacs --batch -L ${finalAttrs.finalPackage}/share/emacs/site-lisp -l likec4-mode \
          --eval '(progn (with-temp-buffer (likec4-mode) (unless (eq major-mode (quote likec4-mode)) (kill-emacs 1))) (unless (file-executable-p (car likec4-lsp-command)) (kill-emacs 2)))'

        echo "[*] 9 claude-mcp-install-likec4: project .mcp.json, repo-relative, merged, idempotent"
        mkdir -p repo/docs && cp -r likec4.config.json model.c4 repo/docs/
        git -C repo init -q
        printf '%s\n' '{"mcpServers":{"other":{"command":"x"}}}' > repo/.mcp.json
        rc=0; (cd repo && claude-mcp-install-likec4 .) || rc=$?   # repo root holds no model directly
        [ "$rc" -eq 0 ]
        (cd repo/docs && claude-mcp-install-likec4)
        (cd repo/docs && claude-mcp-install-likec4)                 # second run: no change
        jq -e '.mcpServers.other.command == "x"' repo/.mcp.json >/dev/null
        jq -e '.mcpServers.likec4 == {"type":"stdio","command":"likec4-mcp","args":["--repo","docs"]}' repo/.mcp.json >/dev/null
        rc=0; (cd "$TMPDIR" && mkdir -p empty && cd empty && claude-mcp-install-likec4) || rc=$?
        [ "$rc" -eq 66 ]                                            # no likec4 project: refused
        # the written entry works from any directory inside the repo
        (cd repo && mkdir -p deep/er && cd deep/er && \
          printf '%s\n' \
            '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}}' \
            '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
            '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list-projects","arguments":{}}}' \
          | timeout 120 likec4-mcp --repo docs | grep -m1 -F '"id":2') > projects.json
        grep -q fixture projects.json

        echo "[*] 10 hermes-mcp-install-likec4: profile config, additions only, \''${HOME}-relative"
        H=$PWD/hermes
        mkdir -p hermes/profiles/arch hermes/profiles/mixed
        cp ${./tests/hermes/styled.yaml} hermes/profiles/arch/config.yaml
        cp ${./tests/hermes/styled.yaml} hermes/config.yaml
        cp ${./tests/hermes/mixed.yaml} hermes/profiles/mixed/config.yaml
        chmod -R u+w hermes
        mkdir -p "$HOME/models" && cp -r repo/docs "$HOME/models/shop"
        (cd "$HOME/models/shop" && HERMES_ROOT=$H hermes-mcp-install-likec4 --profile arch)
        (cd "$HOME/models/shop" && HERMES_ROOT=$H hermes-mcp-install-likec4 --profile arch)   # no change
        [ "$(yq '.mcp_servers.likec4.command' hermes/profiles/arch/config.yaml)" = likec4-mcp ]
        [ "$(yq '.mcp_servers.likec4.args[0]' hermes/profiles/arch/config.yaml)" = "\''${HOME}/models/shop" ]
        [ "$(yq '.mcp_servers.mcp-taskfile.command' hermes/profiles/arch/config.yaml)" = mcp-taskfile ]
        # the edit only adds lines: blank lines, comments, wrapping, `~` all intact
        diff ${./tests/hermes/styled.yaml} hermes/profiles/arch/config.yaml > edit.diff || true
        test -s edit.diff
        if grep -q '^<' edit.diff; then cat edit.diff; exit 1; fi
        cmp -s hermes/config.yaml ${./tests/hermes/styled.yaml}                # other scopes untouched
        rc=0; HERMES_ROOT=$H hermes-mcp-install-likec4 --profile mixed "$HOME/models/shop" || rc=$?
        [ "$rc" -eq 65 ]                                            # cannot edit without reformatting: refused
        cmp -s hermes/profiles/mixed/config.yaml ${./tests/hermes/mixed.yaml}  # ... and left untouched
        rc=0; HERMES_ROOT=$H hermes-mcp-install-likec4 --profile missing "$HOME/models/shop" || rc=$?
        [ "$rc" -eq 66 ]                                            # unknown profile: refused

        echo "[*] 11 likec4-validate-hook: Claude PostToolUse, workspace-level, exit 2 on invalid"
        hook() { jq -n --arg f "$1" '{tool_name:"Edit",tool_input:{file_path:$f}}' | likec4-validate-hook; }
        S=$TMPDIR/hookws
        mkdir -p $S/single $S/mp/a $S/mp/b $S/mp/shared
        cp likec4.config.json model.c4 $S/single/
        hook "$S/single/README.md"                                   # not .c4: silent pass
        hook "$S/single/model.c4" 2> h.err                           # valid single project
        [ ! -s h.err ]
        for p in a b; do
          cp model.c4 $S/mp/$p/
          jq --arg n "$p" '.name = $n' likec4.config.json > $S/mp/$p/likec4.config.json
        done
        printf '%s\n' 'model {' '  broken = nosuchkind "x"' '}' >> $S/mp/b/model.c4
        rc=0; hook "$S/mp/a/model.c4" 2> h.err || rc=$?              # edit in a, error in sibling b
        [ "$rc" -eq 2 ]
        grep -q "nosuchkind" h.err
        rc=0; hook "$S/mp/shared/spec.c4" 2> h.err || rc=$?           # config-less dir -> parent workspace
        [ "$rc" -eq 2 ]

        echo "[+] likec4 smoke test passed"
        touch $out
      '';
    };

    meta = {
      description = "Architecture as code (C4 model): CLI, MCP server, guardrail test runner and Emacs mode";
      homepage = "https://likec4.dev";
      changelog = "https://github.com/likec4/likec4/releases";
      license = lib.licenses.mit;
      sourceProvenance = with lib.sourceTypes; [ fromSource ];
      platforms = lib.platforms.linux;
      mainProgram = "likec4";
    };
  });
in
likec4
