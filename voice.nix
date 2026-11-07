# voice.nix — local TTS/STT: Piper (TTS), faster-whisper (STT), sherpa (wake word).
#
# Piper and sherpa are built against pkgsHermesRev (upstream's own pinned
# nixpkgs revision), not the top-level nixpkgs — ABI mismatch otherwise, since
# hermes-agent's sealed Python venv is built against that same revision.
#
# sherpa-onnx-core/sherpa-onnx/sentencepiece stay as pinned prebuilt wheels:
# they're native onnxruntime-linked .so's, and hermes's lazy-install check
# requires these exact versions — a from-source C++/cmake rebuild would be a
# much larger, version-fragile undertaking for no real supply-chain benefit
# over the from-source treatment given to the pure-Python packages in
# browser.nix.
{ pkgsHermesRev, fetchurl, runCommand }:

let
  # piper-tts built against upstream's nixpkgs revision (pkgsHermesRev) with
  # python3.12 to match the sealed venv. ABI mismatch if built against our nixpkgs-25.11.
  piperTtsApp =
    (pkgsHermesRev.callPackage
      "${pkgsHermesRev.path}/pkgs/by-name/pi/piper-tts/package.nix"
      {
        python3Packages = pkgsHermesRev.python312.pkgs;
        # Inference only: training pulls in torch/lightning/tensorboard for a
        # feature this package never uses.
        withTrain = false;
        withHTTP = false;
        withAlignment = false;
      }).overridePythonAttrs
      (old: {
        # Drop onnxruntime (already in hermes's venv). All three attrs needed:
        # dependencies drives propagatedBuildInputs, pythonRemoveDeps patches dist-info,
        # dontCheckRuntimeDeps prevents re-flagging the removed requirement.
        dependencies = builtins.filter (p: (p.pname or p.name or "") != "onnxruntime") old.dependencies;
        pythonRemoveDeps = [ "onnxruntime" ];
        dontCheckRuntimeDeps = true;
      });

  # toPythonModule bridges piper-tts (a buildPythonApplication) to extraPythonPackages.
  piperTts = pkgsHermesRev.python312.pkgs.toPythonModule piperTtsApp;

  # Wake word: sherpa (not openwakeword, which has no py3.12 wheel).
  # Exact pinned versions (sherpa-onnx 1.13.4, sentencepiece 0.2.2) required by
  # hermes's lazy-install check; nixpkgs has mismatched versions. Vendor wheels.
  sherpaOnnxCore = pkgsHermesRev.python312.pkgs.buildPythonPackage {
    pname = "sherpa-onnx-core";
    version = "1.13.4";
    format = "wheel";
    src = fetchurl {
      url = "https://files.pythonhosted.org/packages/41/be/38c57721d71ee74d984b1ca21720a8ca8477d6d341026af24ff658866ef9/sherpa_onnx_core-1.13.4-py3-none-manylinux2014_x86_64.whl";
      hash = "sha256-NnqgbO6Qs/15WdTgcdb8ghcQuFmvOZtJh+XDEZ7mrio=";
    };
    nativeBuildInputs = [ pkgsHermesRev.autoPatchelfHook ];
    buildInputs = [ pkgsHermesRev.stdenv.cc.cc.lib ];
    dontCheckRuntimeDeps = true;
    doCheck = false;
  };

  # sherpa-onnx's .so files link against sherpaOnnxCore's in a different store path.
  # autoPatchelfHook needs sherpaOnnxCore in buildInputs to resolve RPATH.
  wakeExtraPythonPackages =
    let
      mkHermesWheel = { pname, version, url, hash, dependencies ? [ ], extra ? { } }:
        pkgsHermesRev.python312.pkgs.buildPythonPackage ({
          inherit pname version dependencies;
          format = "wheel";
          src = fetchurl { inherit url hash; };
          dontCheckRuntimeDeps = true;
          doCheck = false;
        } // extra);
    in
    [
      (mkHermesWheel {
        pname = "sherpa-onnx";
        version = "1.13.4";
        url = "https://files.pythonhosted.org/packages/cc/b1/8dfe5d1d72c92ea1c95db999a95b61bfbb9769f1c569f06e572eda095c52/sherpa_onnx-1.13.4-cp312-cp312-manylinux2014_x86_64.manylinux_2_17_x86_64.whl";
        hash = "sha256-XwFY81E9Otqx67oMJvDIFeU7oTuWhG2S7wla4l1kiGA=";
        dependencies = [ sherpaOnnxCore ];
        extra = {
          nativeBuildInputs = [ pkgsHermesRev.autoPatchelfHook ];
          buildInputs = [ sherpaOnnxCore pkgsHermesRev.stdenv.cc.cc.lib ];
          # autoPatchelfHook needs explicit search path for sherpaOnnxCore's nested .so's.
          preFixup = ''
            addAutoPatchelfSearchPath "${sherpaOnnxCore}/${pkgsHermesRev.python312.sitePackages}/sherpa_onnx/lib"
          '';
        };
      })
      (mkHermesWheel {
        pname = "sentencepiece";
        version = "0.2.2";
        url = "https://files.pythonhosted.org/packages/b6/2d/37e3da037318a70066ded0d51bc2a7f35491ae6338dd993d5eb1503fc3b5/sentencepiece-0.2.2-cp312-cp312-manylinux_2_27_x86_64.manylinux_2_28_x86_64.whl";
        hash = "sha256-yKFosEC8YWgSk/ealJtdkRyOJQhvQmAoW42Xq18Rldo=";
      })
      # pypinyin lazy-loaded by sherpa_onnx's keyword-spotter; no exact pin needed.
      pkgsHermesRev.python312.pkgs.pypinyin
    ];

in
{
  # The bundle: extraPythonPackages consumers wire this straight into
  # base.override { extraPythonPackages = hermes-voice-dependencies; }.
  hermes-voice-dependencies = [ piperTts ] ++ wakeExtraPythonPackages;

  # The voice and STT model come from packages/voice, shared with rafa-voice.
  inherit (import ./packages/voice/assets.nix { inherit fetchurl runCommand; })
    piperVoiceDir piperVoiceName fasterWhisperModelDir fasterWhisperModelName;
}
