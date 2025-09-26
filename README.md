# Rust Audio Player (Android)

A simple Android‑focused audio player using Slint for UI and Rodio + Symphonia for audio playback/decoding.

- UI: Slint 1.13 (compiled from `ui/app.slint`)
- Audio: Rodio 0.21 with Symphonia decoders
- Target: Android APK (built with cargo‑apk)

## Features

- Play/Pause/Stop, Previous/Next (auto‑advance when a track ends)
- Seek bar with current time and total duration
- Search box to filter the visible list
- Scans for audio files from a simple "music" directory (or `AUDIO_PLAYER_MUSIC_DIR` env var)

Supported file types scanned by default:
mp3, flac, wav, ogg, opus, aac, m4a, alac, aiff, aif

## Build

Build the Rust library locally (debug):

```powershell
cargo build
```

Optimized:

```powershell
cargo build --release
```

Outputs are an installable Android APK (via `cargo apk`). There is no desktop `main` binary.

### Android (APK)

We use `cargo-apk` to build an installable APK. Locally:

```powershell
# Install cargo-apk once
cargo install cargo-apk --locked

# Build a release APK
cargo apk build --release
```

The resulting APK can be found under `target/android/release/` (or similar) and can be installed via `adb install`.

## CI/CD

The GitHub Actions workflow `.github/workflows/release.yml` runs on tag pushes (`v*`) and produces an installable Android APK, which is uploaded as a GitHub Release asset.

## Notes and Limitations

- This code removes desktop features such as folder pickers, persistent settings, and theme toggles.
- Permissions and file access on mobile are platform‑specific; the simple "music" directory approach is for testing only.
- For a production app, add proper Android permissions and a platform file‑access strategy.

## Credits

- [Slint](https://github.com/slint-ui/slint) for the UI
- [Rodio](https://github.com/RustAudio/rodio) and [Symphonia](https://github.com/pdeljanov/Symphonia) for audio playback and decoding

---

If you run into issues or have ideas for improvements, feel free to open an issue or PR.
