# Khala

Bidirectional real-time voice translator with voice cloning. Translates Spanish to English and English to Spanish simultaneously using OpenAI's GPT Realtime API, with optional RVC to preserve your natural voice.

Built for macOS with Apple Silicon. Person B just uses Zoom normally — no extra software needed.

## How It Works

```
  Your headphones                  BlackHole (middleware)                Zoom
  ================                 =====================                ====

  Bluetooth mic ──> Khala ──> BlackHole 2ch ──────────> Zoom mic input
  (you speak)       (translates ES->EN,                 (B hears English
                     clones your voice)                  in your voice)

  Bluetooth speaker <── Khala <── BlackHole 16ch <──── Zoom speaker output
  (you hear)             (translates EN->ES)            (B speaks English)
```

## Quick Start

```bash
git clone https://github.com/user/khala.git
cd khala
./install.sh
```

```bash
export OPENAI_API_KEY="sk-..."
khala start
```

Press `q` or `Esc` to quit.

## Commands

| Command | Description |
|---|---|
| `khala start` | Start the translator |
| `khala start --rvc` | Start with voice cloning enabled |
| `khala start --no-rvc` | Start without voice cloning |
| `khala doctor` | Verify system setup |
| `khala config` | Show config file |
| `khala logs` | Show RVC server logs |

## Documentation

- **[Installation](docs/Installation.md)** — Requirements, install script, post-install setup, Zoom configuration
- **[Configuration](docs/Configuration.md)** — Full `config.toml` reference with all options
- **[Architecture](docs/Architecture.md)** — System design, pipelines, protocols, project structure
- **[RVC Voice Cloning](docs/RVC-Voice-Cloning.md)** — Voice conversion setup, model training, Apple Silicon notes
- **[Troubleshooting](docs/Troubleshooting.md)** — Common issues, doctor checks, log inspection

## License

MIT
