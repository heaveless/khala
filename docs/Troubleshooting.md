# Troubleshooting

## Doctor Command

Run diagnostics at any time:

```bash
khala doctor
```

Example output:

```
Khala Doctor

  [ok]   Config: /Users/you/.config/khala/config.toml
  [ok]   OPENAI_API_KEY: set
  [ok]   khala-rvc: /Users/you/.local/bin/khala-rvc
  [ok]   RVC lib: /Users/you/RVC-WebUI
  [ok]   RVC model: /Users/you/.config/khala/rvc/my-voice.pth
  [ok]   RVC index: /Users/you/.config/khala/rvc/my-voice.index
  [ok]   HuBERT: /Users/you/.config/khala/rvc/hubert_base.pt
  [ok]   RMVPE: /Users/you/.config/khala/rvc/rmvpe.pt

All checks passed.
```

`khala start` runs these same checks silently before starting. If any check fails, it aborts and tells you to run `khala doctor`.

## Common Issues

### `khala start` fails immediately

Run `khala doctor` to see which check failed. Common causes:

- **`OPENAI_API_KEY` not set** — export the env var or add it to config
- **`[rvc].lib` empty** — set it to your RVC-WebUI directory path
- **Missing model files** — copy your `.pth` and `.index` to `~/.config/khala/rvc/`

### RVC server timeout (60s)

Khala waits up to 60 seconds for the RVC socket to become available. If it times out:

```bash
khala logs
```

Check `rvc-stderr.log` for errors. Common causes:

- **Python version too new** — needs 3.9, 3.10, or 3.11 (fairseq incompatible with 3.12+)
- **Missing Python dependencies** — re-run `./install.sh rvc`
- **RVC lib path incorrect** — verify `[rvc].lib` points to the RVC-WebUI root directory
- **Model file corrupt** — re-download or re-train your voice model

### Model says "understood", "let me translate", or responds conversationally

The model is drifting from its translation-only instructions. Causes and fixes:

1. **Background noise** — enable noise reduction in config:
   ```toml
   [audio]
   noise_reduction = "near_field"   # or "far_field" for laptop mics
   ```

2. **Prompt file outdated** — delete and regenerate:
   ```bash
   rm ~/.config/khala/prompt.txt
   khala start   # regenerates default prompt
   ```

3. **Temperature too high** — lower it for more deterministic output:
   ```toml
   [openai]
   temperature = 0.4
   ```

### Translation repeats or loops

Conversation context may be accumulating. Khala automatically deletes conversation items after each response, but if you see this:

- Restart with `khala start` (fresh session)
- Ensure you're running the latest version

### Audio sounds robotic or distorted

- **Without RVC**: check your speaker device and sample rate settings
- **With RVC**: try adjusting `index_rate` (lower = less processing), `crossfade_time` (higher = smoother transitions), or `f0method` (try `pm` for faster processing)

### No audio output

1. Check that BlackHole is installed: look for "BlackHole 2ch" and "BlackHole 16ch" in System Settings -> Sound
2. Verify Zoom is set to use the correct BlackHole devices
3. Check the TUI — the forward pipeline status should show "Listening..."
4. Make sure your Bluetooth headphones are connected and set as system default

### Translations are too slow

Lower the VAD timings for faster response:

```toml
[vad]
silence_ms = 150     # How quickly to detect end of speech
min_speech_ms = 150  # Minimum speech to trigger translation
```

Lower values = faster commit, but may split sentences.

### API errors in the logs

Some API errors are expected and silently ignored:

| Error Code | Meaning | Action |
|---|---|---|
| `input_audio_buffer_commit_empty` | Committed empty audio buffer | Normal — happens on very short pauses |
| `response_cancel_not_active` | Tried to cancel a non-active response | Normal — race condition |
| `conversation_already_has_active_response` | Overlapping response request | Normal — queued for later |
| `item_delete_invalid_item_id` | Tried to delete already-deleted item | Normal — cleanup race |

Other errors are logged in the TUI with full details.

## Log Inspection

View all RVC logs:

```bash
khala logs
```

Log files are stored at `~/.khala/logs/`:
- `rvc-stdout.log` — RVC server standard output
- `rvc-stderr.log` — RVC server errors and warnings

## Reset to Defaults

To reset everything:

```bash
rm ~/.config/khala/config.toml    # Reset config
rm ~/.config/khala/prompt.txt     # Reset translation prompt
khala start                       # Regenerates both with defaults
```
