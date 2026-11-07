# Per-tool environments that turn off self-update, telemetry and other phone-home
# traffic (ADR-0002), and the wrapper flags generated from them. Pure: no pkgs.
#
# Values are --set-default, so an operator can still override any of them.
# Every entry was verified against the tool's source or docs; what cannot be
# switched off is listed in README.org (Hardening).
{ lib }:
rec {
  env = {
    hermes = {
      # Hermes's package manager otherwise installs missing tools (pip/npm/Chromium) at runtime.
      HERMES_DISABLE_LAZY_INSTALLS = "1";
      DO_NOT_TRACK = "1";
      # browser-harness: PostHog off, and the daily PyPI update banner (0.1.13+).
      BH_TELEMETRY = "0";
      BROWSER_HARNESS_TELEMETRY = "0";
      BH_UPDATE_CHECK = "0";
      # browser-use: telemetry is read by first letter, so it must be an explicit "false".
      ANONYMIZED_TELEMETRY = "false";
      BROWSER_USE_CLOUD_SYNC = "false";
      BROWSER_USE_VERSION_CHECK = "false";
      BROWSER_USE_CLOUD_HINTS = "false";
      # Otherwise browser-use downloads uBlock and cookie extensions from clients2.google.com.
      BROWSER_USE_DISABLE_EXTENSIONS = "1";
      # cua-driver: PostHog and the GitHub release check.
      CUA_DRIVER_RS_TELEMETRY_ENABLED = "0";
      CUA_DRIVER_RS_UPDATE_CHECK = "false";
      HF_HUB_DISABLE_TELEMETRY = "1";
    };

    # claude-code (code.claude.com/docs/en/env-vars). Any non-empty value counts.
    # Not set on purpose: CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC and DISABLE_GROWTHBOOK,
    # which make Remote Control unavailable.
    claude-code = {
      DISABLE_AUTOUPDATER = "1";
      DISABLE_INSTALLATION_CHECKS = "1";
      DISABLE_TELEMETRY = "1";
      DISABLE_ERROR_REPORTING = "1";
      DISABLE_FEEDBACK_COMMAND = "1";
      CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY = "1";
      CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL = "1";
    };

    # antigravity-cli: the background self-updater. Telemetry is an account setting
    # (antigravity.google/docs/settings), with no environment switch.
    antigravity-cli = {
      AGY_CLI_DISABLE_AUTO_UPDATE = "true";
    };

    # openspec: PostHog telemetry (edge.openspec.dev) and the npm-registry update check.
    openspec = {
      OPENSPEC_TELEMETRY = "0";
      DO_NOT_TRACK = "1";
      OPENSPEC_NO_UPDATE_CHECK = "1";
    };

    # likec4: `likec4 build|serve` spawn a detached `likec4 check-update` against the npm
    # registry at most every 24h, skipped only under CI/TEST (std-env). LSP --telemetry
    # only forwards logs to the editor, not to the network.
    likec4 = {
      CI = "1";
    };

    # claude-desktop: the Electron main process reads these (telemetry, error sink).
    # Its updater is switched off by managed settings (claudeDesktopManagedSettings).
    # plannotator: no telemetry; keep plans local (no share links, no r.jina.ai page fetches).
    # Its review UI checks GitHub for a newer release with no switch (README.org).
    plannotator = {
      PLANNOTATOR_SHARE = "disabled";
      PLANNOTATOR_JINA = "0";
    };

    # goose (cli and desktop): PostHog telemetry is opt-in, forced off here; OTLP export off.
    # Not switchable at runtime: the models.dev catalog fetch at every start
    # (online-model-meta cargo feature) and the OSV check of npx/uvx extensions.
    goose = {
      GOOSE_TELEMETRY_OFF = "1";
      GOOSE_TELEMETRY_ENABLED = "false";
      OTEL_SDK_DISABLED = "true";
    };

    # notebooklm-py: no telemetry or update check. Pin the web backend: the opt-in android
    # backend calls Google's experiments config with a Clearcut logger. The RotateCookies
    # keep-alive is session traffic and stays. `login` can still self-install Chromium (M14).
    notebooklm-py = {
      NOTEBOOKLM_BACKEND = "web";
    };

    # ollama: the server fetches ollama.com model recommendations every 4h and enables cloud
    # inference/web search; OLLAMA_NO_CLOUD gates all of it (envconfig, server/*.go). The
    # updater lives only in the desktop app, which nixpkgs does not build.
    ollama = {
      OLLAMA_NO_CLOUD = "1";
    };

    claude-desktop = {
      DISABLE_TELEMETRY = "1";
      DO_NOT_TRACK = "1";
      DISABLE_ERROR_REPORTING = "1";
    };
  };

  # /etc/claude-desktop/managed-settings.json (written by the NixOS module).
  claudeDesktopManagedSettings = {
    disableAutoUpdates = true;
  };

  # Variables that must never be set: they do nothing (antigravity) or re-enable
  # what the rest of the environment turns off (claude-code plugins).
  forbidden = [ "ANTIGRAVITY_DISABLE_AUTOUPDATE" "FORCE_AUTOUPDATE_PLUGINS" ];

  # Hermes reads these from config.yaml, not the environment; shipped as a fragment.
  hermesConfig = {
    updates.check = false;
    plugins.auto_update_check_hours = 0;
    model_catalog.enabled = false;
    security.allow_lazy_installs = false;
    lsp.install_strategy = "manual";
    telemetry.shared_metrics.enabled = false;
  };

  toWrapperArgs = e: lib.concatLists (lib.mapAttrsToList (name: value: [ "--set-default" name value ]) e);

  toWrapperArgsString = e: lib.concatStringsSep " "
    (lib.mapAttrsToList (name: value: "--set-default ${lib.escapeShellArg name} ${lib.escapeShellArg value}") e);
}
