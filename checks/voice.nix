# rafa-voice offline round trip in the sandbox (no network): say a sentence with the pinned
# Piper voice to WAV and MP3, then transcribe the WAV with the pinned faster-whisper model.
{ runCommand, rafa-voice, file }:
runCommand "check-rafa-voice" { nativeBuildInputs = [ rafa-voice file ]; } ''
  set -euo pipefail
  export HOME=$TMPDIR XDG_CONFIG_HOME=$TMPDIR/config
  rafa-voice say "The quick brown fox jumps over the lazy dog." -o $TMPDIR/fox.wav
  file $TMPDIR/fox.wav | grep -q "WAVE audio"
  rafa-voice say - -o $TMPDIR/fox.mp3 <<< "Hello from the stack."
  file $TMPDIR/fox.mp3 | grep -qiE "MPEG|Audio file with ID3"
  echo "[+] say: WAV $(stat -c %s $TMPDIR/fox.wav) bytes, MP3 $(stat -c %s $TMPDIR/fox.mp3) bytes"
  heard=$(rafa-voice transcribe $TMPDIR/fox.wav --language en)
  echo "[*] heard: $heard"
  # A speech model's exact words vary with the CPU (CI once heard "quick-brem"), so the round trip
  # must recover most of the sentence, not all of it: 6 of its 8 distinct words.
  matched=0
  for w in the quick brown fox jumps over lazy dog; do
    if echo "$heard" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z\n' ' ' | grep -qw "$w"; then matched=$((matched + 1)); fi
  done
  echo "[*] transcribe: $matched of 8 words recovered"
  [ "$matched" -ge 6 ]
  echo "[+] transcribe: round trip recovered the sentence"
  if rafa-voice say hi -o $TMPDIR/x.exe 2>/dev/null; then echo "[-] accepted an unknown format"; exit 1; fi
  echo "[+] unknown formats are refused"
  touch $out
''
