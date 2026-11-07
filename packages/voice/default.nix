# rafa-voice (src/voice): the stack's one TTS/STT CLI (plan 5.5). The wrapper puts Piper,
# ffmpeg and rafa-voice-stt (faster-whisper) on PATH and defaults the pinned voice and model; a
# ~/.config/rafa/voice.yaml (rafa.ai.voice) overrides them. Offline: no downloads at runtime.
{ lib, callPackage, makeWrapper, symlinkJoin, fetchurl, runCommand, writers, python3Packages, piper-tts, ffmpeg-headless }:
let
  assets = import ./assets.nix { inherit fetchurl runCommand; };
  unwrapped = (callPackage ../../src/package.nix { }) {
    pname = "rafa-voice";
    crate = "voice";
    description = "One TTS (Piper) and STT (faster-whisper) CLI for the AI stack";
  };
  # faster-whisper, the library Hermes's own STT uses; whisper-ctranslate2 would add
  # pyannote (diarization) and its large, here unbuildable, closure for nothing.
  stt = writers.writePython3Bin "rafa-voice-stt" { libraries = [ python3Packages.faster-whisper ]; flakeIgnore = [ "E501" ]; }
    (builtins.readFile ./stt.py);
  env = {
    RAFA_VOICE_TTS_DIR = "${assets.piperVoiceDir}";
    RAFA_VOICE_TTS_VOICE = assets.piperVoiceName;
    RAFA_VOICE_STT_MODEL = "${assets.fasterWhisperModelDir}";
    # huggingface_hub (under faster-whisper): never fetch, never report.
    HF_HUB_OFFLINE = "1";
    HF_HUB_DISABLE_TELEMETRY = "1";
  };
in
symlinkJoin {
  name = "rafa-voice-${unwrapped.version}";
  inherit (unwrapped) version;
  paths = [ unwrapped ];
  nativeBuildInputs = [ makeWrapper ];
  postBuild = ''
    wrapProgram $out/bin/rafa-voice \
      --prefix PATH : ${lib.makeBinPath [ piper-tts ffmpeg-headless stt ]} \
      ${lib.concatStringsSep " " (lib.mapAttrsToList (k: v: "--set-default ${k} ${lib.escapeShellArg v}") env)}
  '';
  passthru = { inherit assets unwrapped stt; hardeningEnv = env; };
  meta = unwrapped.meta // { mainProgram = "rafa-voice"; };
}
