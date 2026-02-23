# RVC Voice Cloning

RVC (Retrieval-based Voice Conversion) lets Khala output translations **in your own voice** instead of a generic TTS voice. The other person hears English spoken in your voice, making the conversation more natural.

## How It Works

1. OpenAI translates your speech and outputs audio in a TTS voice (e.g., "cedar")
2. The TTS audio is sent to the RVC server via Unix socket
3. RVC converts the TTS voice to match your voice using a trained model
4. The converted audio is sent to Zoom through BlackHole

RVC only runs on the **forward pipeline** (your voice -> their ears). The reverse pipeline (their voice -> your ears) doesn't use RVC.

## Setup

### 1. Get RVC-WebUI

Clone the RVC codebase (needed for inference libraries):

```bash
git clone https://github.com/RVC-Project/Retrieval-based-Voice-Conversion-WebUI.git
```

Set the path in your config:

```toml
[rvc]
enabled = true
lib = "/path/to/Retrieval-based-Voice-Conversion-WebUI"
```

### 2. Train Your Voice Model

Use RVC-WebUI to train a model on recordings of your voice:

1. Record 10-30 minutes of your voice (clean audio, no background noise)
2. Use the RVC-WebUI training tab to create a model
3. Copy the `.pth` file and `.index` file to `~/.config/khala/rvc/`

```toml
[rvc]
model = "~/.config/khala/rvc/your-voice.pth"
index = "~/.config/khala/rvc/your-voice.index"
```

### 3. Pre-trained Models

The installer automatically downloads:

- **HuBERT** (`hubert_base.pt`, ~181MB) — feature extraction model
- **RMVPE** (`rmvpe.pt`, ~173MB) — pitch extraction model

These go to `~/.config/khala/rvc/` and are referenced by config.

### 4. Verify

```bash
khala doctor
```

All RVC checks should pass (model, index, HuBERT, RMVPE, lib path).

## Usage

```bash
khala start           # Uses config setting (rvc.enabled)
khala start --rvc     # Force enable RVC
khala start --no-rvc  # Force disable RVC
```

If RVC is enabled but `khala-rvc` isn't found in PATH, Khala falls back to passthrough mode automatically.

## RVC Configuration

Key tuning parameters in `[rvc]`:

| Field | Default | Effect |
|---|---|---|
| `f0method` | `rmvpe` | Pitch extraction. `rmvpe` = best quality, `pm` = fastest |
| `pitch` | `0` | Pitch shift in semitones. Adjust if your model sounds off-pitch. |
| `index_rate` | `0.3` | Voice cloning strength (0.0-1.0). Higher = more your voice, may introduce artifacts. |
| `block_time` | `0.1` | Processing block size (seconds). Lower = less latency. |
| `extra_time` | `2.5` | Extra audio context for quality. |
| `crossfade_time` | `0.05` | Crossfade between blocks to avoid clicks. |

See [Configuration](Configuration.md) for the full reference.

## Apple Silicon Notes

The RVC Python server requires specific workarounds for macOS Apple Silicon:

### OMP_NUM_THREADS=1

**Required.** FAISS and PyTorch both initialize OpenMP. Having two OpenMP runtimes causes a segfault. Setting `OMP_NUM_THREADS=1` before any imports prevents this.

Khala sets this automatically when launching `khala-rvc`.

### Import Order

FAISS must be imported **before** PyTorch. The `macos_compat.py` module handles this:

```python
import faiss       # MUST be first
import torch       # After faiss
```

Reversing this order causes an OpenMP threading conflict and segfault.

### PyTorch 2.6+ Compatibility

`torch.load` requires `weights_only=True` by default in PyTorch 2.6+, but RVC models contain Python objects that need full unpickling. The compat module monkey-patches `torch.load` to use `weights_only=False`.

### MPS (Metal Performance Shaders)

- GPU acceleration via MPS is enabled for inference
- `multiprocessing.Manager()` + MPS = segfault (fork inherits Metal state) — avoided in the server design
- Environment variables set by Khala:
  - `PYTORCH_ENABLE_MPS_FALLBACK=1` — fallback to CPU for unsupported MPS ops
  - `PYTORCH_MPS_HIGH_WATERMARK_RATIO=0.0` — prevent MPS memory limits

### Known Tensor Issues

- `net_g.infer()` requires `skip_head`/`return_length` as `torch.Tensor`, not int
- `torch.max` on 0-d tensor can't be tuple-unpacked — use `torch.argmax().item()` instead

## khala-rvc Server

The RVC server (`khala-rvc`) is a Python asyncio Unix socket server:

- Starts as a child process of `khala`
- Listens on the configured Unix socket path
- Receives PCM16 audio blocks, converts voice, returns converted audio
- Protocol: length-prefixed binary (4-byte u32 LE + PCM16 samples)
- Logs go to `~/.khala/logs/rvc-stdout.log` and `rvc-stderr.log`

Inspect logs with:

```bash
khala logs
```
