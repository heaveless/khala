# Configuration

All configuration lives in `~/.config/khala/config.toml`. A default config is generated on first run or by `install.sh`.

View your current config:

```bash
khala config
```

## Full Reference

```toml
[openai]
# api_key = ""                # Prefer OPENAI_API_KEY env var
model = "gpt-realtime-mini"   # OpenAI Realtime model
voice = "cedar"               # TTS voice for output audio
temperature = 0.4             # 0.0-1.0 (lower = more deterministic)

[translation]
source_lang = "Spanish"       # Your language (what you speak)
target_lang = "English"       # Their language (what they speak)

[audio]
format = "pcm16"              # Audio format (pcm16 only currently)
noise_reduction = "near_field" # "near_field" (headset), "far_field" (laptop mic), or omit to disable
sample_rate = 24000           # API sample rate in Hz
# mic_device = ""             # Omit for system default
# speaker_device = ""         # Omit for system default

[devices]
virtual_output = "BlackHole 2ch"   # Pipe: Khala -> Zoom
virtual_input = "BlackHole 16ch"   # Pipe: Zoom -> Khala

[vad]
threshold = 0.5               # VAD sensitivity (0.0-1.0)
silence_ms = 150              # Silence duration before committing audio (ms)
prefix_ms = 200               # Audio to include before speech start (ms)
min_speech_ms = 150           # Minimum speech duration to translate (ms)

[rvc]
enabled = false               # Enable RVC voice conversion
lib = ""                      # Path to RVC-WebUI codebase (required if enabled)
model = "{config_dir}/rvc/model.pth"     # Your trained voice model
index = "{config_dir}/rvc/model.index"   # FAISS index file
hubert = "{config_dir}/rvc/hubert_base.pt"  # Pre-trained HuBERT model
rmvpe = "{config_dir}/rvc/rmvpe.pt"      # Pre-trained RMVPE model
socket = "{data_dir}/rvc.sock"           # Unix socket path for IPC
f0method = "rmvpe"            # F0 extraction: rmvpe, pm, harvest, or crepe
pitch = 0                     # Pitch shift in semitones (0 = no change)
index_rate = 0.3              # Voice feature blending (0.0-1.0)
block_time = 0.1              # RVC processing block size in seconds
extra_time = 2.5              # Extra context for voice conversion
crossfade_time = 0.05         # Crossfade between blocks in seconds
```

## Section Details

### [openai]

| Field | Default | Description |
|---|---|---|
| `api_key` | (none) | OpenAI API key. Prefer `OPENAI_API_KEY` env var instead. |
| `model` | `gpt-realtime-mini` | Realtime API model. Options: `gpt-realtime-mini`, `gpt-4o-mini-realtime-preview` |
| `voice` | `cedar` | TTS voice. Options: `alloy`, `ash`, `ballad`, `cedar`, `coral`, `echo`, `sage`, `shimmer`, `verse` |
| `temperature` | `0.4` | Generation randomness. Lower = more deterministic translations. Range: 0.0-1.0 |

### [translation]

| Field | Default | Description |
|---|---|---|
| `source_lang` | `Spanish` | The language you speak |
| `target_lang` | `English` | The language the other person speaks |

### [audio]

| Field | Default | Description |
|---|---|---|
| `format` | `pcm16` | Audio encoding format |
| `noise_reduction` | `near_field` | Server-side noise filtering before VAD and model. `near_field` for headsets, `far_field` for laptop/room mics. Omit to disable. |
| `sample_rate` | `24000` | Sample rate in Hz for the API |
| `mic_device` | (system default) | Input device name. Omit to use system default. |
| `speaker_device` | (system default) | Output device name. Omit to use system default. |

### [devices]

| Field | Default | Description |
|---|---|---|
| `virtual_output` | `BlackHole 2ch` | Virtual device that Zoom reads as its microphone |
| `virtual_input` | `BlackHole 16ch` | Virtual device that Zoom writes its speaker output to |

### [vad]

Voice Activity Detection — controls when speech is detected and committed for translation.

| Field | Default | Description |
|---|---|---|
| `threshold` | `0.5` | VAD sensitivity. Higher = requires louder speech. |
| `silence_ms` | `150` | How long silence must last before audio is committed (ms). Lower = faster response, but may split mid-sentence. |
| `prefix_ms` | `200` | Audio buffer included before detected speech start (ms). Prevents clipping the beginning of words. |
| `min_speech_ms` | `150` | Minimum speech duration to be considered valid (ms). Filters out noise bursts. |

### [rvc]

RVC (Retrieval-based Voice Conversion) — clones your voice onto the translated output. See [RVC Voice Cloning](RVC-Voice-Cloning.md) for setup details.

| Field | Default | Description |
|---|---|---|
| `enabled` | `false` | Enable/disable RVC |
| `lib` | (empty) | Path to your local RVC-WebUI codebase |
| `model` | `{config_dir}/rvc/model.pth` | Your trained `.pth` voice model |
| `index` | `{config_dir}/rvc/model.index` | FAISS `.index` file for your model |
| `hubert` | `{config_dir}/rvc/hubert_base.pt` | Pre-trained HuBERT (downloaded by installer) |
| `rmvpe` | `{config_dir}/rvc/rmvpe.pt` | Pre-trained RMVPE (downloaded by installer) |
| `socket` | `{data_dir}/rvc.sock` | Unix socket path for Rust <-> Python IPC |
| `f0method` | `rmvpe` | Pitch extraction method: `rmvpe` (best), `pm` (fast), `harvest`, `crepe` |
| `pitch` | `0` | Pitch shift in semitones. 0 = no change. |
| `index_rate` | `0.3` | How much to blend FAISS voice features (0.0-1.0). Higher = more voice cloning. |
| `block_time` | `0.1` | Processing block size in seconds. Lower = less latency, more CPU. |
| `extra_time` | `2.5` | Extra audio context for conversion quality. |
| `crossfade_time` | `0.05` | Crossfade duration between blocks to avoid clicks. |

## Translation Prompt

The translation prompt lives at `~/.config/khala/prompt.txt`. It controls how the model behaves. The default prompt enforces strict translation-only behavior:

- `{from}` is replaced with your `source_lang`
- `{to}` is replaced with your `target_lang`

Delete the file to regenerate the default prompt on next start.
