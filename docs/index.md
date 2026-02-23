# Khala

Bidirectional real-time voice translator with voice cloning. Translates Spanish to English and English to Spanish simultaneously using OpenAI's GPT Realtime API, with optional RVC (Retrieval-based Voice Conversion) to preserve your natural voice.

Built for macOS with Apple Silicon.

## How It Works

Person A (you) speaks Spanish and runs Khala. Person B speaks English and uses Zoom/Discord normally — they don't install anything. All processing happens on A's machine.

```
  Your headphones                  BlackHole (middleware)                Zoom
  ================                 =====================                ====

  Bluetooth mic ──> Khala ──> BlackHole 2ch ──────────> Zoom mic input
  (you speak)       (translates ES->EN,                 (B hears English
                     clones your voice)                  in your voice)

  Bluetooth speaker <── Khala <── BlackHole 16ch <──── Zoom speaker output
  (you hear)             (translates EN->ES)            (B speaks English)
```

### Audio device roles

| Device | Who uses it | Role |
|---|---|---|
| Bluetooth mic | Khala (auto-detected) | Captures your voice |
| Bluetooth speaker | Khala (auto-detected) | Plays B's translated voice to you |
| BlackHole 2ch | Zoom (set as mic) | Pipe: Khala's translated output -> Zoom |
| BlackHole 16ch | Zoom (set as speaker) | Pipe: Zoom's audio -> Khala for translation |

Person B doesn't know Khala exists. They just hear English and speak English normally.

## Quick Start

```bash
git clone https://github.com/user/khala.git
cd khala
./install.sh
```

Set your API key and start:

```bash
export OPENAI_API_KEY="sk-..."
khala start
```

See [Installation](Installation.md) for the full setup guide.

## CLI Commands

| Command | Description |
|---|---|
| `khala start` | Start the translator |
| `khala start --rvc` | Start with RVC voice cloning enabled |
| `khala start --no-rvc` | Start without RVC (even if config has it enabled) |
| `khala doctor` | Verify system setup — config, API key, RVC dependencies |
| `khala config` | Show config file path and contents |
| `khala logs` | Display RVC server log contents |

Press `q` or `Esc` to quit the TUI.

## Documentation

- **[Installation](Installation.md)** — Requirements, install script, post-install setup, Zoom configuration
- **[Configuration](Configuration.md)** — Full `config.toml` reference with all options
- **[Architecture](Architecture.md)** — System design, pipelines, protocols, project structure
- **[RVC Voice Cloning](RVC-Voice-Cloning.md)** — Voice conversion setup, model training, Apple Silicon notes
- **[Troubleshooting](Troubleshooting.md)** — Common issues, doctor checks, log inspection
