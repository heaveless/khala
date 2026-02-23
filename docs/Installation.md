# Installation

## Requirements

- macOS with Apple Silicon (M1/M2/M3/M4)
- Rust toolchain (edition 2024)
- Python 3.9, 3.10, or 3.11 (fairseq is incompatible with 3.12+)
- [BlackHole](https://existential.audio/blackhole/) virtual audio driver (2ch + 16ch)
- [RVC-WebUI](https://github.com/RVC-Project/Retrieval-based-Voice-Conversion-WebUI) codebase (for voice conversion inference, optional)
- OpenAI API key with Realtime API access
- Any headphones/mic as system default

Person B (the other person on the call) needs nothing.

## Install

```bash
git clone https://github.com/user/khala.git
cd khala
./install.sh
```

The install script supports partial installation:

```bash
./install.sh          # Install everything (default)
./install.sh core     # Build and install khala only (Rust)
./install.sh rvc      # Install khala-rvc only (Python)
```

### What install.sh does

1. Checks for `cargo` and Python 3.9-3.11
2. Builds the `khala` Rust binary (`cargo build --release`) and copies it to `~/.local/bin/`
3. Sets up `khala-rvc` Python server with its own virtualenv at `~/.local/share/khala-rvc/`
4. Installs Python dependencies from `requirements.txt`
5. Downloads pre-trained models (HuBERT ~181MB, RMVPE ~173MB) to `~/.config/khala/rvc/`
6. Creates the `khala-rvc` wrapper script in `~/.local/bin/`
7. Generates a default config at `~/.config/khala/config.toml`

### PATH setup

Make sure `~/.local/bin` is in your PATH:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

## Post-install Setup

### 1. Set your OpenAI API key

```bash
export OPENAI_API_KEY="sk-..."
```

Or add it permanently to your shell profile, or set it in `~/.config/khala/config.toml` under `[openai].api_key`.

### 2. Configure RVC paths (optional)

If you want voice cloning, edit `~/.config/khala/config.toml`:

```toml
[rvc]
enabled = true
lib = "/path/to/RVC-WebUI"                        # Your local RVC-WebUI codebase
model = "~/.config/khala/rvc/your-voice.pth"       # Your trained voice model
index = "~/.config/khala/rvc/your-voice.index"     # FAISS index for your model
```

See [RVC Voice Cloning](RVC-Voice-Cloning.md) for details on training your voice model.

### 3. Configure Zoom audio

In Zoom:

- Settings -> Audio -> **Microphone**: select `BlackHole 2ch`
- Settings -> Audio -> **Speaker**: select `BlackHole 16ch`

This works the same for Discord, Google Meet, or any other call app.

### 4. Verify setup

```bash
khala doctor
```

This checks config, API key, and all RVC dependencies.

## Installed File Locations

| Path | Contents |
|---|---|
| `~/.local/bin/khala` | Rust binary |
| `~/.local/bin/khala-rvc` | Python wrapper script |
| `~/.local/share/khala-rvc/` | Python sources + virtualenv |
| `~/.config/khala/config.toml` | Configuration |
| `~/.config/khala/prompt.txt` | Translation prompt |
| `~/.config/khala/rvc/` | Pre-trained models (HuBERT, RMVPE) + your voice model |
| `~/.khala/` | Runtime data (logs, socket) |

## Uninstall

```bash
./uninstall.sh
```

Supports partial uninstall:

```bash
./uninstall.sh          # Remove everything (binaries, config, data)
./uninstall.sh core     # Remove khala binary only
./uninstall.sh rvc      # Remove khala-rvc only (wrapper + venv)
```

Full uninstall removes all installed files: binaries, Python runtime, config, and data directories.
