# The stack's pinned voice assets (plan 5.5), shared by Hermes (voice.nix) and rafa-voice:
# one Piper voice and one faster-whisper model, fetched by hash, nothing at runtime.
{ fetchurl, runCommand }:
let
  piperVoiceName = "en_GB-alba-medium";

  # Hashes from rhasspy/piper-voices HuggingFace repo (content-addressed, verified).
  piperVoiceOnnx = fetchurl {
    url = "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_GB/alba/medium/${piperVoiceName}.onnx";
    hash = "sha256-QBNpxKgdCf3YbDLFyGRECBHb3MZkZs3i1k9xM6Zq0Ds=";
  };
  piperVoiceConfig = fetchurl {
    url = "https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_GB/alba/medium/${piperVoiceName}.onnx.json";
    hash = "sha256-qpZaLwLsztYywmlOH8crv/bWXyZfq1Z8qUWRjHPdifQ=";
  };

  # PiperVoice.load() requires .onnx and .onnx.json under real names in same dir.
  piperVoiceDir = runCommand "rafa-piper-voice-${piperVoiceName}" { } ''
    mkdir -p "$out"
    ln -s ${piperVoiceOnnx} "$out/${piperVoiceName}.onnx"
    ln -s ${piperVoiceConfig} "$out/${piperVoiceName}.onnx.json"
  '';

  # STT model pre-fetched to avoid lazy download via huggingface_hub.
  # Config: stt.local.model → <this directory> in config.yaml.
  fasterWhisperModelName = "base";
  fasterWhisperModelDir = runCommand "rafa-faster-whisper-${fasterWhisperModelName}" { } ''
    mkdir -p "$out"
    ln -s ${fetchurl {
      url = "https://huggingface.co/Systran/faster-whisper-${fasterWhisperModelName}/resolve/main/config.json";
      hash = "sha256-VqbYEQ0xHxnI8EceVigyx1J/FGtWcnW/yln898GE2po=";
    }} "$out/config.json"
    ln -s ${fetchurl {
      url = "https://huggingface.co/Systran/faster-whisper-${fasterWhisperModelName}/resolve/main/model.bin";
      hash = "sha256-0BwwFIgcnG8xM8GC89KIfrbKHHiadTjFwAcZaFegpqk=";
    }} "$out/model.bin"
    ln -s ${fetchurl {
      url = "https://huggingface.co/Systran/faster-whisper-${fasterWhisperModelName}/resolve/main/tokenizer.json";
      hash = "sha256-+3tjGR6bsEUILHn9dCoxBqEsmVE6sw30oNR/pstv0Ks=";
    }} "$out/tokenizer.json"
    ln -s ${fetchurl {
      url = "https://huggingface.co/Systran/faster-whisper-${fasterWhisperModelName}/resolve/main/vocabulary.txt";
      hash = "sha256-NM4/4cUEECez+NQpEicJk/mG28S7NM8n+VHjSh5FORM=";
    }} "$out/vocabulary.txt"
  '';
in
{
  inherit piperVoiceDir piperVoiceName fasterWhisperModelDir fasterWhisperModelName;
}
