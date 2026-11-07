# rafa.ai.voice (plan 5.5): one TTS/STT for the whole stack, on by default. Installs
# rafa-voice, links the pinned voice and model to stable paths under
# ~/.local/share/rafa/voice/{tts,stt} and writes ~/.config/rafa/voice.yaml pointing at them,
# so no consumer (Slack, Hermes profiles, scripts) needs a store path.
{ packages }:
{ config, lib, ... }:
let
  inherit (lib) mkOption mkIf types;
  cfg = config.rafa.ai.voice;
  assets = cfg.package.passthru.assets;
  data = "${config.xdg.dataHome}/rafa/voice";
in
{
  options.rafa.ai.voice = {
    enable = mkOption {
      type = types.bool;
      default = true;
      description = "The shared voice: rafa-voice, voice.yaml and the stable model links.";
    };
    package = mkOption {
      type = types.package;
      default = packages.rafa-voice;
      defaultText = lib.literalMD "this flake's rafa-voice";
      description = "The rafa-voice package (its passthru.assets are the linked voice and model).";
    };
    tts.voice = mkOption {
      type = types.str;
      default = assets.piperVoiceName;
      defaultText = lib.literalExpression ''"en_GB-alba-medium"'';
      description = "Piper voice name; its .onnx and .onnx.json must be in tts/ (the pinned one is).";
    };
    stt.language = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "Language code for transcription; null lets faster-whisper detect it.";
    };
  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];
    xdg.dataFile."rafa/voice/tts".source = assets.piperVoiceDir;
    xdg.dataFile."rafa/voice/stt".source = assets.fasterWhisperModelDir;
    xdg.configFile."rafa/voice.yaml".text = builtins.toJSON {
      version = 1;
      tts = { inherit (cfg.tts) voice; dir = "${data}/tts"; };
      stt = { model = "${data}/stt"; } // lib.optionalAttrs (cfg.stt.language != null) { inherit (cfg.stt) language; };
    };
  };
}
