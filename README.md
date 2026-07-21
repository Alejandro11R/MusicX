# Cadence

Cadence is a lightweight desktop music launcher for focus sessions. Search, press Enter, and keep working.

No library, no playlists, no recommendations. Just search → listen → focus.

## Philosophy

- Validate in the terminal first.
- Build incrementally.
- One responsibility per module.
- No speculative abstractions.
- Concrete code first, abstractions only when needed.

## Stack

- Tauri v2 + Rust
- React + TypeScript + Vite
- Playback via `mpv` (controlled over its IPC socket)
- Search via `yt-dlp`

## Development

```bash
npm install
npm run tauri dev
```
