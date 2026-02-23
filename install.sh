#!/usr/bin/env bash
set -euo pipefail

INSTALL_DIR="$HOME/.local/share/khala-rvc"
BIN_DIR="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/khala"
DATA_DIR="$HOME/.khala"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

TARGET="${1:-all}"  # core, rvc, or all (default)

info()  { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
ok()    { printf "\033[1;32m  ✓\033[0m %s\n" "$1"; }
fail()  { printf "\033[1;31m  ✗\033[0m %s\n" "$1"; exit 1; }

if [[ "$TARGET" != "all" && "$TARGET" != "core" && "$TARGET" != "rvc" ]]; then
    printf "Usage: %s [core|rvc]\n" "$0"
    printf "  core   — build and install khala only\n"
    printf "  rvc    — install khala-rvc only\n"
    printf "  (none) — install both\n"
    exit 1
fi

# --- Find compatible Python (3.9–3.11) ---

find_python() {
    for cmd in python3.9 python3.10 python3.11; do
        if command -v "$cmd" >/dev/null 2>&1; then
            echo "$cmd"
            return
        fi
    done
    fail "Python 3.9–3.11 required (fairseq incompatible with 3.12+). Install with: brew install python@3.9"
}

# --- Prerequisites ---

info "Checking prerequisites"
if [[ "$TARGET" == "all" || "$TARGET" == "core" ]]; then
    command -v cargo >/dev/null 2>&1 || fail "cargo not found. Install Rust first."
    ok "cargo $(cargo --version | cut -d' ' -f2)"
fi
if [[ "$TARGET" == "all" || "$TARGET" == "rvc" ]]; then
    PYTHON="$(find_python)"
    ok "$PYTHON $($PYTHON --version | cut -d' ' -f2)"
fi

mkdir -p "$BIN_DIR"

# --- Build and install khala (Rust) ---

if [[ "$TARGET" == "all" || "$TARGET" == "core" ]]; then
    info "Building khala"
    cargo build --release --manifest-path "$SCRIPT_DIR/Cargo.toml"
    ok "Built target/release/khala"

    cp "$SCRIPT_DIR/target/release/khala" "$BIN_DIR/khala"
    ok "Installed $BIN_DIR/khala"
fi

# --- Install khala-rvc (Python) ---

if [[ "$TARGET" == "all" || "$TARGET" == "rvc" ]]; then
    info "Installing khala-rvc"
    mkdir -p "$INSTALL_DIR"
    cp "$SCRIPT_DIR/khala-rvc/"*.py "$INSTALL_DIR/"
    ok "Copied Python sources to $INSTALL_DIR"

    if [ ! -d "$INSTALL_DIR/.venv" ]; then
        info "Creating Python venv"
        "$PYTHON" -m venv "$INSTALL_DIR/.venv"
        ok "Created $INSTALL_DIR/.venv"
    fi

    info "Installing Python dependencies"
    "$INSTALL_DIR/.venv/bin/pip" install --quiet "pip<24.1"
    "$INSTALL_DIR/.venv/bin/pip" install --quiet -r "$SCRIPT_DIR/khala-rvc/requirements.txt"
    ok "Python dependencies installed"

    # --- Pre-trained model assets ---

    RVC_DIR="$CONFIG_DIR/rvc"
    HF_BASE="https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/main"

    download() {
        local url="$1" dest="$2"
        if [ -f "$dest" ]; then
            ok "Already exists: $(basename "$dest")"
            return
        fi
        mkdir -p "$(dirname "$dest")"
        info "Downloading $(basename "$dest")..."
        curl -fSL --progress-bar -o "$dest" "$url" || fail "Failed to download $(basename "$dest")"
        ok "Downloaded $(basename "$dest")"
    }

    download "$HF_BASE/hubert_base.pt" "$RVC_DIR/hubert_base.pt"
    download "$HF_BASE/rmvpe.pt"       "$RVC_DIR/rmvpe.pt"

    # --- khala-rvc wrapper ---

    cat > "$BIN_DIR/khala-rvc" << 'WRAPPER'
#!/usr/bin/env bash
exec "$HOME/.local/share/khala-rvc/.venv/bin/python" "$HOME/.local/share/khala-rvc/main.py" "$@"
WRAPPER
    chmod +x "$BIN_DIR/khala-rvc"
    ok "Installed $BIN_DIR/khala-rvc"
fi

# --- Default config ---

if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    info "Creating default config"
    mkdir -p "$CONFIG_DIR"
    mkdir -p "$DATA_DIR"

    sed -e "s|{config_dir}|$CONFIG_DIR|g" \
        -e "s|{data_dir}|$DATA_DIR|g" \
        "$SCRIPT_DIR/khala-config/config.toml" > "$CONFIG_DIR/config.toml"

    ok "Created $CONFIG_DIR/config.toml"
else
    ok "Config already exists at $CONFIG_DIR/config.toml"
fi

# --- PATH check ---

if ! echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
    printf "\n\033[1;33m  ⚠\033[0m %s is not in your PATH. Add it:\n" "$BIN_DIR"
    printf "    echo 'export PATH=\"%s:\$PATH\"' >> ~/.zshrc\n" "$BIN_DIR"
fi

# --- Done ---

printf "\n\033[1;32mInstallation complete.\033[0m\n"
if [[ "$TARGET" == "all" || "$TARGET" == "core" ]]; then
    printf "  khala:     %s/khala\n" "$BIN_DIR"
fi
if [[ "$TARGET" == "all" || "$TARGET" == "rvc" ]]; then
    printf "  khala-rvc: %s/khala-rvc\n" "$BIN_DIR"
fi
printf "  config:    %s/config.toml\n" "$CONFIG_DIR"
printf "\nRun 'khala doctor' to verify your setup.\n"
