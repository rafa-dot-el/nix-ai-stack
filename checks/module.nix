# Pure eval check of the NixOS module: assertions, the loopback forcing behind a
# domain, firewall exposure, unit hardening and the desktop-free default package.
{ lib, runCommand, nixosSystem, system, module, packages }:
let
  eval = extra: (nixosSystem {
    inherit system;
    modules = [
      module
      {
        boot.loader.grub.enable = false;
        fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
        system.stateVersion = "25.11";
        security.acme = { acceptTerms = true; defaults.email = "ops@example.invalid"; };
      }
      extra
    ];
  }).config;

  failedAssertions = config: map (a: a.message) (lib.filter (a: !a.assertion) config.assertions);
  hasFailure = config: needle: lib.any (lib.hasInfix needle) (failedAssertions config);
  dashboard = config: config.systemd.services.hermes-agent-dashboard.serviceConfig;

  direct = eval { services.hermes-agent.dashboard = { enable = true; bindAddress = "0.0.0.0"; openFirewall = true; }; };
  behindDomain = eval {
    services.hermes-agent.dashboard = { enable = true; bindAddress = "0.0.0.0"; domain = "hermes.example.invalid"; };
  };
  authNoDomain = eval {
    services.hermes-agent.dashboard = { enable = true; basicAuth = { enable = true; passwordFile = "/run/secrets/p"; }; };
  };
  mtlsNoCa = eval {
    services.hermes-agent.dashboard = { enable = true; domain = "hermes.example.invalid"; mTLS.enable = true; };
  };
  gateway = eval { services.hermes-agent.gateway.enable = true; };
  bare = eval { };
  # The new path; everything above goes through the services.hermes-agent alias.
  newPath = eval { rafa.ai.agents.hermes.server.dashboard = { enable = true; bindAddress = "0.0.0.0"; }; };
  claude = eval { rafa.ai.agents.claude.enable = true; };
  ollama = eval { rafa.ai.runtimes.ollama = { enable = true; variant = "vulkan"; }; };

  checks = {
    "importing the module starts no Hermes service and creates no user" =
      !(bare.systemd.services ? hermes-agent-gateway) && !(bare.systemd.services ? hermes-agent-dashboard)
      && !(bare.users.users ? hermes-agent);
    "the dashboard binds loopback unless told otherwise" =
      lib.hasInfix "--host 127.0.0.1" (dashboard (eval { services.hermes-agent.dashboard.enable = true; })).ExecStart;
    "basicAuth without a domain is refused" = hasFailure authNoDomain "basicAuth.enable requires rafa.ai.agents.hermes.server.dashboard.domain";
    "mTLS without a CA file is refused" = hasFailure mtlsNoCa "mTLS.enable requires rafa.ai.agents.hermes.server.dashboard.mTLS.caFile";
    "a clean config has no failed assertions" = failedAssertions direct == [ ];
    "direct mode binds the configured address" = lib.hasInfix "--host 0.0.0.0" (dashboard direct).ExecStart;
    "direct mode opens its port" = lib.elem 8080 direct.networking.firewall.allowedTCPPorts;
    "a domain forces the dashboard to loopback" = lib.hasInfix "--host 127.0.0.1" (dashboard behindDomain).ExecStart;
    "a domain never opens the dashboard port" = !(lib.elem 8080 behindDomain.networking.firewall.allowedTCPPorts);
    "a domain puts nginx in front" = behindDomain.services.nginx.enable;
    "dashboard runs unprivileged and confined" =
      let s = dashboard direct; in s.NoNewPrivileges && s.ProtectSystem == "strict" && s.PrivateTmp && s.User == "hermes-agent";
    "gateway runs confined" =
      let s = gateway.systemd.services.hermes-agent-gateway.serviceConfig; in s.NoNewPrivileges && s.ProtectSystem == "strict";
    "the service package carries no desktop app" =
      !(lib.hasInfix "desktop" direct.rafa.ai.agents.hermes.server.package.name);
    "the new option path configures the same service" = lib.hasInfix "--host 0.0.0.0" (dashboard newPath).ExecStart;
    "claude writes managed settings with auto-updates off" =
      (builtins.fromJSON claude.environment.etc."claude-desktop/managed-settings.json".text).disableAutoUpdates;
    "ollama runs the chosen variant with cloud features off" =
      ollama.services.ollama.package.outPath == packages.ollama-vulkan.outPath
      && ollama.services.ollama.environmentVariables.OLLAMA_NO_CLOUD == "1";
    "ollama listens on loopback by default" = ollama.services.ollama.host == "127.0.0.1";
  };
  failed = lib.attrNames (lib.filterAttrs (_: ok: !ok) checks);
in
if failed != [ ] then throw "module checks failed: ${lib.concatStringsSep "; " failed}"
else runCommand "check-module" { } ''
  echo "[+] ${toString (lib.length (lib.attrNames checks))} module checks passed"
  touch $out
''
