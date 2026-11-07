"""rafa-voice-stt: transcribe one audio file with a local faster-whisper model (offline)."""
import argparse

from faster_whisper import WhisperModel


def main() -> None:
    parser = argparse.ArgumentParser(prog="rafa-voice-stt")
    parser.add_argument("--model", required=True, help="CTranslate2 model directory")
    parser.add_argument("--output", required=True, help="text file to write")
    parser.add_argument("--language", default=None)
    parser.add_argument("file")
    args = parser.parse_args()
    model = WhisperModel(args.model, device="cpu", compute_type="int8", local_files_only=True)
    segments, _info = model.transcribe(args.file, language=args.language)
    with open(args.output, "w", encoding="utf-8") as out:
        out.write(" ".join(segment.text.strip() for segment in segments) + "\n")


if __name__ == "__main__":
    main()
