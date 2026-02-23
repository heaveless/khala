# Architecture

## System Overview

```
                          khala (manager)
                               |
                    +----------+----------+
                    |                     |
              khala-rvc              khala-core
           (Python/PyTorch)               |
                    |          +---------+---------+
                    |          |                   |
                 Unix Socket   Forward Pipeline    Reverse Pipeline
                    |          (ES -> EN)          (EN -> ES)
                    |          |                   |
                    |    Mic ----> GPT ----> RVC ----> BlackHole 2ch ---> Zoom
                    |                                                     |
                    |          BlackHole 16ch <--- Zoom <--- B speaks     |
                    |               |                                     |
                    +--------> GPT ----> Speaker                         |
                                         (you hear)
```

Khala runs two concurrent translation pipelines plus a TUI dashboard, managed by a single binary.

## Pipelines

### Forward Pipeline (you speak, they hear)

Your Spanish speech is translated to English with your voice cloned via RVC.

```
Bluetooth mic -> capture (cpal) -> resample to 24kHz -> base64 encode
  -> OpenAI GPT Realtime API (ES -> EN translation)
  -> RVC voice conversion (Unix socket) -> resample to device rate
  -> BlackHole 2ch (Zoom picks this up as mic)
```

- Uses both `text` and `audio` modalities
- RVC is optional: without it, the API's TTS voice plays directly
- Output goes to `virtual_output` device (BlackHole 2ch)

### Reverse Pipeline (they speak, you hear)

The other person's English is translated to Spanish so you can understand them.

```
BlackHole 16ch (Zoom outputs B's audio here) -> capture (cpal)
  -> resample to 24kHz -> base64 encode
  -> OpenAI GPT Realtime API (EN -> ES translation)
  -> Bluetooth speaker (you hear)
```

- Text-only mode (no RVC, no audio output device)
- Input comes from `virtual_input` device (BlackHole 16ch)
- Translated text appears as subtitles in the TUI

## Queue-Based Translation

Khala uses a queue model for natural, fluid translation:

1. **You speak** — audio streams to the API input buffer continuously
2. **You pause** — client-side VAD detects silence, commits the audio buffer
3. **Translation starts** — if no response is in progress, a `response.create` is sent immediately
4. **You keep speaking** — new sentences queue up while the current translation plays
5. **Translation finishes** — the next queued sentence starts translating automatically

This means:
- Translations are **never cancelled** mid-stream (no repetition, no lost words)
- Multiple sentences **queue naturally** without blocking
- You speak at your own pace, translations catch up seamlessly

## Conversation Cleanup

To prevent the model from drifting into conversational mode, Khala automatically deletes conversation items after each response:

- User audio items are tracked when created (`conversation.item.created`)
- On `response.created`: pending items move to active tracking
- On `response.done`: active items + response output items are deleted via `conversation.item.delete`

This keeps the conversation context minimal (only the system prompt), preventing:
- Repeated translations
- The model responding conversationally ("Sure!", "Let me translate...")
- Context buildup that dilutes the translation prompt

## Client-Side VAD

Khala uses its own Voice Activity Detection instead of the server's turn detection:

- **Why?** Server VAD truncates in-flight responses when it detects new speech. With client-side VAD, the server streams translations uninterrupted while the user speaks the next sentence.
- **How?** A 10ms polling loop monitors input RMS levels. When RMS exceeds the threshold, speech is active. When silence persists for `silence_ms`, speech ends and audio is committed.
- Server `turn_detection` is set to `null` to disable server-side VAD entirely.

## Startup Flow

1. Run pre-flight checks (silent doctor)
2. Start `khala-rvc` Python server as a child process (if enabled)
3. Wait for Unix socket ready (poll every 500ms, 60s timeout)
4. Launch forward + reverse pipelines concurrently
5. Start TUI dashboard (ratatui)
6. On quit: kill RVC server, cleanup socket

## Project Structure

```
khala/
├── Cargo.toml                          # Workspace root
├── install.sh                          # Installation script
├── uninstall.sh                        # Uninstallation script
├── src/
│   ├── main.rs                         # CLI dispatch, RVC lifecycle, doctor
│   ├── cli.rs                          # Subcommand definitions (clap)
│   ├── config.rs                       # TOML config loading
│   └── ui.rs                           # TUI dashboard (ratatui)
├── khala-core/                         # Library crate
│   └── src/
│       ├── lib.rs                      # Public module exports
│       ├── audio.rs                    # Capture/playback (cpal)
│       ├── config.rs                   # Pipeline config struct
│       ├── metrics.rs                  # Lock-free pipeline metrics
│       ├── pipeline.rs                 # Pipeline orchestration
│       ├── protocol.rs                 # OpenAI Realtime API types
│       ├── rvc.rs                      # RVC Unix socket client
│       └── websocket.rs               # WebSocket send/receive
├── khala-config/
│   ├── config.toml                     # Default config template
│   └── prompt.txt                      # Default translation prompt
└── khala-rvc/                          # Python RVC server
    ├── main.py                         # Entry point + CLI args
    ├── processor.py                    # RvcProcessor (voice conversion)
    ├── server.py                       # Asyncio Unix socket server
    ├── macos_compat.py                 # Apple Silicon workarounds
    └── requirements.txt                # Python dependencies
```

## Communication Protocol

### Rust <-> OpenAI (WebSocket)

Uses the OpenAI Realtime API v1 (beta) over WebSocket:

| Direction | Event | Purpose |
|---|---|---|
| Client -> Server | `session.update` | Configure session (modalities, voice, temperature, noise reduction) |
| Client -> Server | `input_audio_buffer.append` | Stream audio chunks (base64 PCM16) |
| Client -> Server | `input_audio_buffer.commit` | Commit buffered audio for processing |
| Client -> Server | `response.create` | Trigger a translation response |
| Client -> Server | `conversation.item.delete` | Clean up processed items |
| Server -> Client | `response.audio.delta` | Translated audio chunks (base64 PCM16) |
| Server -> Client | `response.text.delta` | Translated text chunks |
| Server -> Client | `response.done` | Response completed |
| Server -> Client | `conversation.item.created` | New item in conversation |

### Rust <-> Python RVC (Unix Socket)

Length-prefixed binary protocol over Unix socket:

```
[4 bytes: payload length (u32 LE)] [payload: PCM16 i16 LE samples]
```

- Khala sends audio blocks to the RVC server
- RVC server returns voice-converted audio in the same format
- Special commands: empty payload = flush, reset via separate message type
