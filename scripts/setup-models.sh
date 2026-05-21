#!/usr/bin/env bash
set -euo pipefail

# Download whisper.cpp and Gemma 3 model(s) into the hearsay data directory.
#
# Usage:
#   ./scripts/setup-models.sh                            # large-v3-turbo + gemma-3-12b (production defaults)
#   ./scripts/setup-models.sh tiny                       # whisper tiny (~80 MB, for tests)
#   ./scripts/setup-models.sh gemma-3-1b                 # tiny English-only Gemma (~600 MB, for tests)
#   ./scripts/setup-models.sh large-v3-turbo gemma-3-12b # explicit production set
#
# Recognised names: tiny, base, small, medium, large-v3, large-v3-turbo,
#                   gemma-3-1b, gemma-3-4b, gemma-3-12b, gemma-3-27b

MODEL_DIR="${HEARSAY_MODEL_DIR:-}"
if [ -z "$MODEL_DIR" ]; then
    if [ "$(uname)" = "Darwin" ]; then
        MODEL_DIR="$HOME/Library/Application Support/hearsay/models"
    else
        MODEL_DIR="$HOME/.local/share/hearsay/models"
    fi
fi
mkdir -p "$MODEL_DIR"

models=("$@")
if [ ${#models[@]} -eq 0 ]; then
    models=("large-v3-turbo" "gemma-3-12b")
fi

WHISPER_BASE="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"

# Gemma 3 GGUFs come from Bartowski's quants — the canonical source for
# Q4_K_M quantizations of Google's instruction-tuned releases.
gemma_url() {
    local size="$1"
    echo "https://huggingface.co/bartowski/google_gemma-3-${size}-it-GGUF/resolve/main/google_gemma-3-${size}-it-Q4_K_M.gguf"
}

for name in "${models[@]}"; do
    case "$name" in
        gemma-3-*)
            size="${name#gemma-3-}"
            url="$(gemma_url "$size")"
            dest="$MODEL_DIR/${name}.gguf"
            ;;
        *)
            url="$WHISPER_BASE/ggml-${name}.bin"
            dest="$MODEL_DIR/ggml-${name}.bin"
            ;;
    esac

    if [ -s "$dest" ]; then
        echo "==> ${dest##*/} already present ($(du -h "$dest" | cut -f1))"
        continue
    fi

    echo "==> Downloading ${dest##*/}"
    curl -L --fail --progress-bar "$url" -o "$dest.partial"
    mv "$dest.partial" "$dest"
    echo "==> Done: $(du -h "$dest" | cut -f1)"
done

echo
echo "Model directory: $MODEL_DIR"
echo "Override locations: set transcription.model_path / summarization.model_path in your config."
