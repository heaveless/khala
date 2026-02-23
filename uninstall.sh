#!/usr/bin/env bash
set -euo pipefail

BIN_DIR="$HOME/.local/bin"
INSTALL_DIR="$HOME/.local/share/khala-rvc"
CONFIG_DIR="$HOME/.config/khala"
DATA_DIR="$HOME/.khala"

TARGET="${1:-all}"  # core, rvc, or all (default)

info()  { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
ok()    { printf "\033[1;32m  ✓\033[0m %s\n" "$1"; }
skip()  { printf "\033[1;33m  -\033[0m %s\n" "$1"; }

if [[ "$TARGET" != "all" && "$TARGET" != "core" && "$TARGET" != "rvc" ]]; then
    printf "Usage: %s [core|rvc]\n" "$0"
    printf "  core   — uninstall khala only\n"
    printf "  rvc    — uninstall khala-rvc only\n"
    printf "  (none) — uninstall both\n"
    exit 1
fi

remove() {
    local path="$1" label="$2"
    if [ -e "$path" ]; then
        rm -rf "$path"
        ok "Removed $label ($path)"
    else
        skip "$label not found"
    fi
}

if [[ "$TARGET" == "all" || "$TARGET" == "core" ]]; then
    info "Uninstalling khala"
    remove "$BIN_DIR/khala" "khala binary"
fi

if [[ "$TARGET" == "all" || "$TARGET" == "rvc" ]]; then
    info "Uninstalling khala-rvc"
    remove "$BIN_DIR/khala-rvc" "khala-rvc wrapper"
    remove "$INSTALL_DIR"       "khala-rvc runtime"
fi

if [[ "$TARGET" == "all" ]]; then
    info "Removing config and data"
    remove "$CONFIG_DIR" "config"
    remove "$DATA_DIR"   "data"
fi

printf "\n\033[1;32mDone.\033[0m"
if [[ "$TARGET" == "all" ]]; then
    printf " khala has been fully uninstalled.\n"
else
    printf " khala %s has been uninstalled.\n" "$TARGET"
fi
