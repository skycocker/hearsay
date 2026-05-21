#!/usr/bin/env bash
set -euo pipefail

# Download whisper.cpp GGML model(s) into the hearsay data directory.
#
# Usage:
#   ./scripts/setup-models.sh                       # download large-v3-turbo (~1.5 GB)
#   ./scripts/setup-models.sh tiny                  # download tiny (~75 MB, for tests / quick check)
#   ./scripts/setup-models.sh large-v3-turbo tiny   # both

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
    models=("large-v3-turbo")
fi

BASE_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"

for name in "${models[@]}"; do
    file="ggml-${name}.bin"
    dest="$MODEL_DIR/$file"

    if [ -s "$dest" ]; then
        echo "==> $file already present at $dest ($(du -h "$dest" | cut -f1))"
        continue
    fi

    echo "==> Downloading $file → $dest"
    curl -L --fail --progress-bar "$BASE_URL/$file" -o "$dest.partial"
    mv "$dest.partial" "$dest"
    echo "==> Done: $(du -h "$dest" | cut -f1)"
done

echo
echo "Model directory: $MODEL_DIR"
echo "To use a non-default location, set transcription.model_path in your hearsay config."
