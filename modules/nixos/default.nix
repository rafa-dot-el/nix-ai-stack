# NixOS side of the rafa.ai tree (ADR-0006): only what needs the system level.
#   agents.hermes.server   Hermes dashboard and gateway services (was services.hermes-agent)
#   agents.claude          /etc/claude-desktop/managed-settings.json
#   runtimes.ollama        the ollama service, hardened, no models shipped
# `packages` and `mkHermesPackage` are this flake's, passed by flake.nix.
{ packages, mkHermesPackage, isx86_64 }:
{ config, lib, ... }:
let
  inherit (lib) mkEnableOption mkOption mkIf types;
  cfg = config.rafa.ai;
  hardening = import ../../lib/hardening.nix { inherit lib; };
  ollamaVariants = {
    cpu = packages.ollama;
    rocm = packages.ollama-rocm;
    cuda = packages.ollama-cuda;
    vulkan = packages.ollama-vulkan;
  };
in
{
  imports = [
    (import ./hermes-server.nix { inherit config lib mkHermesPackage isx86_64; })
    (lib.mkRenamedOptionModule [ "services" "hermes-agent" ] [ "rafa" "ai" "agents" "hermes" "server" ])
  ];

  options.rafa.ai = {
    agents.claude = {
      enable = mkEnableOption "the system side of the Claude stack (Claude Desktop managed settings)";
      desktop.managedSettings = mkOption {
        type = types.attrsOf types.anything;
        default = hardening.claudeDesktopManagedSettings;
        defaultText = lib.literalExpression "{ disableAutoUpdates = true; }";
        description = "Written to /etc/claude-desktop/managed-settings.json.";
      };
    };

    runtimes.ollama = {
      enable = mkEnableOption "the ollama service (no models are shipped; use `ollama pull`)";
      variant = mkOption {
        type = types.enum (lib.attrNames ollamaVariants);
        default = "cpu";
        description = "Acceleration backend.";
      };
      host = mkOption { type = types.str; default = "127.0.0.1"; description = "Listen address."; };
      port = mkOption { type = types.port; default = 11434; description = "Listen port."; };
      modelsDir = mkOption {
        type = types.str;
        default = "/var/lib/ollama/models";
        description = "Where `ollama pull` stores models.";
      };
      openFirewall = mkOption { type = types.bool; default = false; description = "Open `port`."; };
      environment = mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = "Extra OLLAMA_* environment for the service.";
      };
    };
  };

  config = lib.mkMerge [
    (mkIf cfg.agents.claude.enable {
      environment.etc."claude-desktop/managed-settings.json".text = builtins.toJSON cfg.agents.claude.desktop.managedSettings;
    })
    (mkIf cfg.runtimes.ollama.enable {
      services.ollama = {
        enable = true;
        package = ollamaVariants.${cfg.runtimes.ollama.variant};
        inherit (cfg.runtimes.ollama) host port openFirewall;
        models = cfg.runtimes.ollama.modelsDir;
        # The wrapper defaults these too; the service sets them explicitly as well.
        environmentVariables = hardening.env.ollama // cfg.runtimes.ollama.environment;
      };
    })
  ];
}
