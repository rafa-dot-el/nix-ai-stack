# Unit tests (lib.runTests) for lib/hardening.nix: the per-tool environments that
# turn off self-update and telemetry, and the wrapper flags generated from them.
{ lib, runCommand }:
let
  h = import ../lib/hardening.nix { inherit lib; };
  results = lib.runTests {
    testWrapperArgsAreSetDefaultPairsInOrder = {
      expr = h.toWrapperArgs { B = "2"; A = "1"; };
      expected = [ "--set-default" "A" "1" "--set-default" "B" "2" ];
    };
    testEmptyEnvGivesNoArgs = { expr = h.toWrapperArgs { }; expected = [ ]; };
    testShellStringQuotesValues = {
      expr = h.toWrapperArgsString { X = "a b"; };
      expected = "--set-default X 'a b'";
    };
    testNoToolSetsAForbiddenVariable = {
      expr = lib.filter (v: lib.any (env: env ? ${v}) (lib.attrValues h.env)) h.forbidden;
      expected = [ ];
    };
    testEveryValueIsANonEmptyString = {
      expr = lib.concatLists (lib.mapAttrsToList (tool: env:
        lib.mapAttrsToList (k: v: "${tool}.${k}") (lib.filterAttrs (_: v: !(lib.isString v) || v == "") env)) h.env);
      expected = [ ];
    };
    testHermesDisablesLazyInstallsAndBrowserTelemetry = {
      expr = lib.all (k: h.env.hermes ? ${k}) [
        "HERMES_DISABLE_LAZY_INSTALLS" "DO_NOT_TRACK" "BH_TELEMETRY" "BH_UPDATE_CHECK"
        "ANONYMIZED_TELEMETRY" "BROWSER_USE_VERSION_CHECK" "CUA_DRIVER_RS_TELEMETRY_ENABLED" "HF_HUB_DISABLE_TELEMETRY"
      ];
      expected = true;
    };
    # browser-use reads ANONYMIZED_TELEMETRY by first letter (t/y/1 = on), so "" or "0"-like
    # spellings other than an explicit false must not slip in.
    testBrowserUseTelemetryIsExplicitFalse = { expr = h.env.hermes.ANONYMIZED_TELEMETRY; expected = "false"; };
    testClaudeCodeTurnsOffUpdatesTelemetryErrorsAndFeedback = {
      expr = lib.all (k: h.env.claude-code ? ${k}) [
        "DISABLE_AUTOUPDATER" "DISABLE_TELEMETRY" "DISABLE_ERROR_REPORTING" "DISABLE_FEEDBACK_COMMAND"
        "CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY" "CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL"
      ];
      expected = true;
    };
    # Both disable Remote Control (remoteControlAtStartup is used), so they must never be set.
    testClaudeCodeKeepsRemoteControlWorking = {
      expr = lib.filter (k: h.env.claude-code ? ${k}) [ "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC" "DISABLE_GROWTHBOOK" ];
      expected = [ ];
    };
    # The documented switch (antigravity.google/docs/cli/troubleshooting); the old wrapper's
    # ANTIGRAVITY_DISABLE_AUTOUPDATE never occurs in the binary.
    testAntigravityUsesTheDocumentedUpdateSwitch = {
      expr = h.env.antigravity-cli;
      expected = { AGY_CLI_DISABLE_AUTO_UPDATE = "true"; };
    };
    # openspec 1.16 reads OPENSPEC_TELEMETRY/DO_NOT_TRACK leniently, 1.7 only "0"/"1": these values work in both.
    testOpenspecTurnsOffTelemetryAndUpdateCheck = {
      expr = h.env.openspec;
      expected = { OPENSPEC_TELEMETRY = "0"; DO_NOT_TRACK = "1"; OPENSPEC_NO_UPDATE_CHECK = "1"; };
    };
    # likec4 spawns `likec4 check-update` (npm registry, every 24h) on build/serve; std-env's CI
    # detection is its only off switch.
    testLikec4StopsTheNpmUpdateCheck = { expr = h.env.likec4; expected = { CI = "1"; }; };
    testClaudeDesktopTurnsOffTelemetryAndErrorReporting = {
      expr = h.env.claude-desktop;
      expected = { DISABLE_TELEMETRY = "1"; DO_NOT_TRACK = "1"; DISABLE_ERROR_REPORTING = "1"; };
    };
    testClaudeDesktopManagedSettingsDisableAutoUpdates = {
      expr = h.claudeDesktopManagedSettings.disableAutoUpdates;
      expected = true;
    };
    # plannotator has no telemetry; what leaves the machine is sharing (share.plannotator.ai)
    # and URL annotation through r.jina.ai. Its UI release check has no switch (README).
    testPlannotatorKeepsPlansLocal = {
      expr = h.env.plannotator;
      expected = { PLANNOTATOR_SHARE = "disabled"; PLANNOTATOR_JINA = "0"; };
    };
    # goose: PostHog is opt-in, forced off; OTLP export off. The models.dev catalog fetch at
    # startup has no runtime switch (only the online-model-meta cargo feature).
    testGooseTurnsOffTelemetry = {
      expr = h.env.goose;
      expected = { GOOSE_TELEMETRY_OFF = "1"; GOOSE_TELEMETRY_ENABLED = "false"; OTEL_SDK_DISABLED = "true"; };
    };
    testNotebooklmStaysOnTheWebBackend = { expr = h.env.notebooklm-py; expected = { NOTEBOOKLM_BACKEND = "web"; }; };
    # ollama's server fetches ollama.com model recommendations every 4h and enables cloud
    # features unless OLLAMA_NO_CLOUD is set.
    testOllamaTurnsOffCloud = { expr = h.env.ollama; expected = { OLLAMA_NO_CLOUD = "1"; }; };
    testHermesConfigFragmentTurnsOffPhoneHome = {
      expr = with h.hermesConfig; [ updates.check plugins.auto_update_check_hours model_catalog.enabled
                                    security.allow_lazy_installs lsp.install_strategy ];
      expected = [ false 0 false false "manual" ];
    };
  };
in
if results != [ ] then throw "hardening-lib: ${builtins.toJSON results}"
else runCommand "check-hardening-lib" { } "touch $out"
