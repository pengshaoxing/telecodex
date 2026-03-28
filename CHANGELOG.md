# Changelog

## Unreleased

### Added
- **Reply-to-message context**: Users can reply to any message in Telegram to provide context; the referenced text is injected into the Codex prompt so it understands what the user is referring to
- **Phased streaming messages**: Each Codex intermediate reasoning phase is now preserved as a separate, permanent Telegram message instead of being overwritten — users can scroll back to review the agent's step-by-step analysis
- **Tool message reply guard**: Replying to a tool action message (emoji-prefixed) sends a helpful notification instead of forwarding meaningless context to Codex
- **Remote Whisper API transcription**: Support OpenAI-compatible speech-to-text APIs (SiliconFlow, Groq, etc.) via `[whisper]` config section
- **FFmpeg audio speedup**: Optionally speed up voice messages before sending to Whisper API to reduce transcription cost (`whisper.speed_factor`)
- **Persistent codex app-server process**: Reuse the codex subprocess across turns within a session instead of spawning a new one each time
- **Configurable idle process timeout**: `idle_process_timeout_seconds` controls how long an idle codex process stays alive (default: 300s)
- **Transcription engine display**: Show which engine (Whisper/Handy Parakeet) was used after transcribing voice messages
- **Model catalog display**: `/model` command now shows all available models from Codex, not just the current one
- **Built-in log file support**: `log_file` config option with dual-layer tracing (stderr + file)
- **Inline file delivery**: Codex can deliver files inline via `<telegramFile>` tags
- **`requestUserInput` tool support**: Interactive keyboard-based input prompts from Codex
- **Tracing coverage**: Added debug/warn logging to telegram.rs, config.rs, commands.rs, codex_history.rs
- **Setup guide**: `docs/setup.md` with step-by-step instructions

### Fixed
- **Multi-phase text accumulation**: `RunSummary.assistant_text` now concatenates all agent message phases instead of only keeping the last one
- **Streaming message display**: Messages no longer get stuck at "typing" or show truncated content; multi-message split during streaming
- **Shutdown hang**: Second command no longer gets permanently stuck after first turn completes (pipe drop + timeout fix)
- **Duplicate search keywords**: Removed duplicates in `auto_search_mode_for_prompt`

### Changed
- `transcribe-rs` is now an optional dependency (feature-gated behind `local-transcription`)
- Worker loop uses `WorkerMessage` enum for turn processing and process lifecycle control
