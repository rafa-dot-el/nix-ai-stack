# Pure eval check of the rafa.ai Home Manager module: what each root switch installs,
# what stays off, and the hardened Hermes config fragment.
{ lib, runCommand, homeManagerConfiguration, pkgs, module, packages }:
let
  eval = settings: (homeManagerConfiguration {
    inherit pkgs;
    modules = [
      module
      { home = { username = "tester"; homeDirectory = "/home/tester"; stateVersion = "26.05"; }; }
      settings
    ];
  }).config;
  # By store path: claude-desktop and its FHS variant share a name.
  paths = config: map (p: p.outPath) config.home.packages;
  has = config: pkg: lib.elem pkg.outPath (paths config);
  ours = lib.attrValues packages;

  none = eval { };
  voiceOff = eval { rafa.ai.voice.enable = false; };
  voicePl = eval { rafa.ai.voice = { tts.voice = "pl_PL-gosia-medium"; stt.language = "pl"; }; };
  voiceYaml = c: builtins.fromJSON c.xdg.configFile."rafa/voice.yaml".text;
  claude = eval { rafa.ai.agents.claude.enable = true; };
  claudeFhs = eval { rafa.ai.agents.claude = { enable = true; desktop.fhs = true; }; };
  claudeNoDesktop = eval { rafa.ai.agents.claude = { enable = true; desktop.enable = false; }; };
  goose = eval { rafa.ai.agents.goose.enable = true; };
  gooseCliOnly = eval { rafa.ai.agents.goose = { enable = true; desktop.enable = false; }; };
  hermes = eval { rafa.ai.agents.hermes.enable = true; };
  hermesUnhardened = eval { rafa.ai.agents.hermes.enable = true; rafa.ai.hardening.enable = false; };
  tools = eval { rafa.ai.tools = { openspec.enable = true; likec4.enable = true; }; };
  # A server without the proxy: its own units, nothing else of the mcp stack.
  serverOnly = eval { rafa.ai.tools.playwright.server.enable = true; rafa.ai.voice.enable = false; };
  cliOnly = eval { rafa.ai.tools = { slack.enable = true; gworkspace.enable = true; tmpmail.enable = true; playwright.enable = true; }; };
  slCustom = eval { rafa.ai.tools.slack.server = { enable = true; listen.socket = "%t/custom/slack.sock"; }; };
  mcp = eval {
    rafa.ai.mcp = {
      enable = true;
      servers.mine = { enable = true; exec = [ "/bin/my-mcp" ]; native = true; prefix = "m"; credentials.token = "/run/t"; };
      profiles.coding.servers.playwright.include = [ "browser_*" ];
    };
    rafa.ai.tools.playwright.server.enable = true;
  };
  gw = eval {
    rafa.ai.tools.gworkspace = {
      settings.auth = { client.file = "/run/agenix/gclient"; refresh_token.command = [ "pass" "show" "g/rt" ]; };
      servers = lib.genAttrs [ "gtasks" "gdocs" "gsheets" "gcal" "gdrive" "gmail" "gmeet" ] (_: { enable = true; });
    };
    rafa.ai.mcp.enable = true;
  };
  sl = eval {
    rafa.ai.tools.slack = { server.enable = true; settings = { credentials.token.command = [ "pass" "show" "slack/token" ]; policy.rules = [ { match.self_dm = true; action = "live"; } ]; }; };
    rafa.ai.mcp.enable = true;
  };
  slNoVoice = eval { rafa.ai.voice.enable = false; rafa.ai.tools.slack.server.enable = true; rafa.ai.mcp.enable = true; };
  tm = eval { rafa.ai.tools.tmpmail.server.enable = true; rafa.ai.mcp.enable = true; };
  usageOnly = eval { rafa.ai.agents.claude.usage.enable = true; };
  usageExp = eval { rafa.ai.agents.claude.usage = { enable = true; exporter = { enable = true; port = 9555; }; }; };
  cal = eval {
    rafa.ai.tools.calibre = { server.enable = true; settings = { database = "/srv/books/metadata.db"; attach_roots = [ "/home/tester/share" ]; }; };
    rafa.ai.mcp.enable = true;
  };
  zot = eval {
    rafa.ai.tools.zotero = { server.enable = true; settings = { database = "/srv/zotero/zotero.sqlite"; translation_server = "http://127.0.0.1:1969"; }; };
    rafa.ai.mcp.enable = true;
  };
  units = c: c.systemd.user;
  dropin = c: builtins.fromJSON c.xdg.configFile."rafa/mcp.d/10-nix.yaml".text;

  checks = {
    "only the shared voice is installed by default (rafa.ai.voice, plan 5.5)" =
      lib.filter (has none) ours == [ packages.rafa-voice ];
    "voice off installs nothing from this flake" = !(lib.any (has voiceOff) ours);
    "voice: voice.yaml points at the stable links" =
      let v = voiceYaml none; in v.version == 1 && v.tts.voice == "en_GB-alba-medium"
        && v.tts.dir == "/home/tester/.local/share/rafa/voice/tts" && v.stt.model == "/home/tester/.local/share/rafa/voice/stt"
        && !(v.stt ? language);
    "voice: the stable links are the pinned assets" =
      none.xdg.dataFile."rafa/voice/tts".source == packages.rafa-voice.passthru.assets.piperVoiceDir
      && none.xdg.dataFile."rafa/voice/stt".source == packages.rafa-voice.passthru.assets.fasterWhisperModelDir;
    "voice: settings reach voice.yaml" = (voiceYaml voicePl).tts.voice == "pl_PL-gosia-medium" && (voiceYaml voicePl).stt.language == "pl";
    "voice off writes no voice.yaml" = !(voiceOff.xdg.configFile ? "rafa/voice.yaml");
    "gworkspace: CLIs installed and settings rendered" =
      has gw packages.gworkspace
      && (builtins.fromJSON gw.xdg.configFile."rafa/gworkspace.yaml".text) == {
        version = 1;
        auth = { client.file = "/run/agenix/gclient"; refresh_token.command = [ "pass" "show" "g/rt" ]; };
      };
    "gworkspace: gtasks is a native socket-activated server" =
      lib.toList (units gw).services.mcp-gtasks.Service.ExecStart == [ ''"${packages.gworkspace}/bin/gtasks" "serve" "--systemd-socket"'' ]
      && (units gw).sockets ? mcp-gtasks;
    "gworkspace: gdocs is a native socket-activated server" =
      lib.toList (units gw).services.mcp-gdocs.Service.ExecStart == [ ''"${packages.gworkspace}/bin/gdocs" "serve" "--systemd-socket"'' ];
    "gworkspace: gmeet is a native socket-activated server" =
      lib.toList (units gw).services.mcp-gmeet.Service.ExecStart == [ ''"${packages.gworkspace}/bin/gmeet" "serve" "--systemd-socket"'' ];
    "gworkspace: gmail is a native socket-activated server" =
      lib.toList (units gw).services.mcp-gmail.Service.ExecStart == [ ''"${packages.gworkspace}/bin/gmail" "serve" "--systemd-socket"'' ];
    "gworkspace: gdrive is a native socket-activated server" =
      lib.toList (units gw).services.mcp-gdrive.Service.ExecStart == [ ''"${packages.gworkspace}/bin/gdrive" "serve" "--systemd-socket"'' ];
    "gworkspace: gcal is a native socket-activated server" =
      lib.toList (units gw).services.mcp-gcal.Service.ExecStart == [ ''"${packages.gworkspace}/bin/gcal" "serve" "--systemd-socket"'' ];
    "gworkspace: gsheets is a native socket-activated server" =
      lib.toList (units gw).services.mcp-gsheets.Service.ExecStart == [ ''"${packages.gworkspace}/bin/gsheets" "serve" "--systemd-socket"'' ];
    "gworkspace: no settings, no file" = !(mcp.xdg.configFile ? "rafa/gworkspace.yaml");
    "slack: CLI installed and settings rendered (secret refs only)" =
      has sl packages.slack
      && (builtins.fromJSON sl.xdg.configFile."rafa/slack.yaml".text) == {
        version = 1;
        credentials.token.command = [ "pass" "show" "slack/token" ];
        policy.rules = [ { match.self_dm = true; action = "live"; } ];
      };
    "slack: a native socket-activated server" =
      lib.toList (units sl).services.mcp-slack.Service.ExecStart == [ ''"${packages.slack}/bin/slack" "serve" "--systemd-socket"'' ];
    "slack: the server speaks through the shared voice, by store path" =
      lib.elem "RAFA_VOICE=${packages.rafa-voice}/bin/rafa-voice" (units sl).services.mcp-slack.Service.Environment;
    "slack: voice off, no RAFA_VOICE (tts is refused, never a PATH lookup)" =
      lib.elem "RAFA_VOICE=" (units slNoVoice).services.mcp-slack.Service.Environment;
    "slack: no settings, no file" = !(mcp.xdg.configFile ? "rafa/slack.yaml");
    "tmpmail: CLI installed" = has tm packages.tmpmail;
    "claude: the root switch includes claude-usage" = has claude packages.claude-usage;
    "calibre: calibrectl installed and settings rendered" =
      has cal packages.calibrectl
      && (builtins.fromJSON cal.xdg.configFile."rafa/calibrectl.yaml".text) == { version = 1; database = "/srv/books/metadata.db"; attach_roots = [ "/home/tester/share" ]; };
    "zotero: zoteroctl installed and settings rendered as TOML" =
      has zot packages.zoteroctl
      && lib.hasInfix ''database = "/srv/zotero/zotero.sqlite"'' (builtins.readFile zot.xdg.configFile."rafa/zoteroctl/config.toml".source)
      && lib.hasInfix ''translation_server = "http://127.0.0.1:1969"'' (builtins.readFile zot.xdg.configFile."rafa/zoteroctl/config.toml".source);
    "zotero: a stdio server behind the sidecar" =
      let exec = lib.concatStringsSep " " (lib.toList (units zot).services.mcp-zotero.Service.ExecStart); in
      lib.hasInfix "\"sidecar\" \"--name\" \"zotero\"" exec && lib.hasInfix "-zoteroctl-${packages.zoteroctl.version}/bin/zoteroctl\" \"serve\"" exec;
    "calibre: a stdio server behind the sidecar" =
      let exec = lib.concatStringsSep " " (lib.toList (units cal).services.mcp-calibre.Service.ExecStart); in
      lib.hasInfix "\"sidecar\" \"--name\" \"calibre\"" exec && lib.hasInfix "-calibrectl-${packages.calibrectl.version}/bin/calibrectl\" \"serve\"" exec;
    "claude-usage: the polybar module calls the CLI" =
      lib.hasPrefix "${packages.claude-usage}/bin/claude-usage --format" usageOnly.services.polybar.settings."module/claude-usage".exec
      && usageOnly.services.polybar.settings."module/claude-usage".type == "custom/script";
    "claude-usage: no exporter unless asked" = !((units usageOnly).services ? claude-usage-exporter) && !(usageOnly.xdg.configFile ? "rafa/claude-usage.yaml");
    "claude-usage: the exporter is a user service on loopback" =
      lib.toList (units usageExp).services.claude-usage-exporter.Service.ExecStart == [ "${packages.claude-usage}/bin/claude-usage exporter" ]
      && (builtins.fromJSON usageExp.xdg.configFile."rafa/claude-usage.yaml".text).exporter == { bind = "127.0.0.1"; port = 9555; };
    "tmpmail: a native socket-activated server" =
      lib.toList (units tm).services.mcp-tmpmail.Service.ExecStart == [ ''"${packages.tmpmail}/bin/tmpmail" "serve" "--systemd-socket"'' ];
    "claude root switch installs code, acp and desktop" =
      has claude packages.claude-code && has claude packages.claude-agent-acp && has claude packages.claude-desktop;
    "claude desktop.fhs swaps in the FHS variant" =
      has claudeFhs packages.claude-desktop-fhs && !(has claudeFhs packages.claude-desktop);
    "a sub-option can be turned off under the root switch" =
      has claudeNoDesktop packages.claude-code && !(has claudeNoDesktop packages.claude-desktop);
    "goose root switch installs the CLI and the desktop app" =
      has goose packages.goose-cli && has goose packages.goose-desktop;
    "goose: a sub-option can be turned off under the root switch" =
      has gooseCliOnly packages.goose-cli && !(has gooseCliOnly packages.goose-desktop);
    "goose: installing starts nothing" = (units goose).services == { } && (units goose).sockets == { };
    "goose is opt-in" = !(has none packages.goose-cli) && !(has none packages.goose-desktop);
    "hermes root switch installs hermes" = has hermes packages.default;
    "hermes ships the hardened config fragment" =
      hermes.xdg.dataFile ? "rafa/hermes/hardened-config.yaml";
    "no fragment when hardening is off" =
      !(hermesUnhardened.xdg.dataFile ? "rafa/hermes/hardened-config.yaml");
    "tools are opt-in, one by one" =
      has tools packages.openspec && has tools packages.likec4 && !(has tools packages.plannotator);
    "agents do not pull in tools" = !(has claude packages.plannotator) && !(has hermes packages.openspec);
    "cli only: tools.<x>.enable installs the CLI and creates no unit" =
      has cliOnly packages.slack && has cliOnly packages.gworkspace && has cliOnly packages.tmpmail && has cliOnly packages.playwright-mcp
      && (units cliOnly).services == { } && (units cliOnly).sockets == { };
    "server.enable installs the CLI too" = has sl packages.slack && has tm packages.tmpmail && has mcp packages.playwright-mcp;
    "server without mcp.enable: its own socket and service, no proxy, no mcp binary, no drop-in" =
      lib.attrNames (units serverOnly).sockets == [ "mcp-playwright" ] && lib.attrNames (units serverOnly).services == [ "mcp-playwright" ]
      && !(has serverOnly packages.mcp) && !(serverOnly.xdg.configFile ? "rafa/mcp.d/10-nix.yaml");
    "server: listen.socket defaults to the proxy pool, named after the server" =
      (units sl).sockets.mcp-slack.Socket.ListenStream == "%t/rafa/mcp/servers/slack.sock";
    "server: listen.socket is overridable" =
      (units slCustom).sockets.mcp-slack.Socket.ListenStream == "%t/custom/slack.sock";
    "gworkspace: the CLI switch alone starts no server" = !((units (eval { rafa.ai.tools.gworkspace.enable = true; })).services ? mcp-gtasks);
    "mcp: installs the binary and the proxy in the target" =
      has mcp packages.mcp && (units mcp).services ? mcp-proxy && (units mcp).targets ? rafa-mcp;
    "mcp: one socket and one service per enabled server, none for presets left off" =
      lib.attrNames (units mcp).sockets == [ "mcp-mine" "mcp-playwright" ]
      && lib.attrNames (units mcp).services == [ "mcp-mine" "mcp-playwright" "mcp-proxy" ];
    "mcp: the playwright preset runs headless behind the sidecar" =
      let e = toString (lib.toList (units mcp).services.mcp-playwright.Service.ExecStart); in
      lib.hasInfix "/bin/mcp\" \"sidecar\" \"--name\" \"playwright\"" e && lib.hasInfix "playwright-mcp" e && lib.hasInfix "--headless" e;
    "mcp: a native server runs directly with its credentials" =
      lib.toList (units mcp).services.mcp-mine.Service.ExecStart == [ ''"/bin/my-mcp"'' ]
      && (units mcp).services.mcp-mine.Service.LoadCredential == [ "token:/run/t" ];
    "mcp: the drop-in carries prefixes and profiles" =
      let d = dropin mcp; in d.servers.mine.prefix == "m" && d.servers.playwright.prefix == "web"
        && d.profiles.coding.servers.playwright.include == [ "browser_*" ];

  };
  failed = lib.attrNames (lib.filterAttrs (_: ok: !ok) checks);
in
if failed != [ ] then throw "home-module checks failed: ${lib.concatStringsSep "; " failed}"
else runCommand "check-home-module" { } ''
  echo "[+] ${toString (lib.length (lib.attrNames checks))} home-module checks passed"
  touch $out
''
