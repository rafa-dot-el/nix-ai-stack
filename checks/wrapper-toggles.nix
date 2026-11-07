# Pure eval check: each feature toggle adds or removes exactly its tools from the
# hermes wrappers' PATH (and the browser default only comes with the browser).
{ lib, runCommand, mkHermesPackage }:
let
  names = pkg: map lib.getName pkg.passthru.pathTools;
  full = mkHermesPackage { withDesktop = false; };
  noBrowser = mkHermesPackage { withDesktop = false; withBrowser = false; };
  noLsp = mkHermesPackage { withDesktop = false; withLsp = false; };
  browserTools = [ "chromium" "browser-use" "buzz-cli" "cua-driver" "agent-browser" ];
  lspTools = [ "pyright" "typescript-language-server" "gopls" "rust-analyzer" "nixd" "clang-tools" ];
  has = pkg: tool: lib.elem tool (names pkg);
  failures =
    lib.optional (!lib.all (has full) (browserTools ++ lspTools ++ [ "xclip" "xsel" ]))
      "full build misses tools: ${toString (lib.filter (t: !has full t) (browserTools ++ lspTools))}"
    ++ lib.optional (lib.any (has noBrowser) browserTools) "withBrowser = false still ships browser tools"
    ++ lib.optional (noBrowser.passthru.browserWrapArgs != "") "withBrowser = false still sets AGENT_BROWSER_EXECUTABLE_PATH"
    ++ lib.optional (lib.any (has noLsp) lspTools) "withLsp = false still ships LSP servers"
    ++ lib.optional (!lib.all (has noLsp) browserTools) "withLsp = false dropped browser tools";
in
if failures != [ ] then throw "wrapper-toggles: ${lib.concatStringsSep "; " failures}"
else runCommand "check-wrapper-toggles" { } "touch $out"
