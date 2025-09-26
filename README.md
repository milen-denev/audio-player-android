# Rust Audio Player (Mobile‑focused)

A simplified mobile‑targeted audio player core using Slint for UI and Rodio + Symphonia for audio playback/decoding.

- UI: Slint 1.13 (compiled from `ui/app.slint`)
- Audio: Rodio 0.21 with Symphonia decoders
- Targets: Android and iOS (library outputs for embedding in host apps)

This repository now builds a library (no desktop binary) suitable for integration into Android and iOS apps. A minimal Slint UI is provided and wired to playback controls, but packaging and host‑app glue are left to the platform projects.

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

Outputs are libraries (cdylib/staticlib/rlib). There is no desktop `main` binary in this branch.

### Android

We use `cargo-ndk` in CI to build `.so` libraries for common ABIs. Locally:

```powershell
# Install cargo-ndk once
cargo install cargo-ndk --locked

# Build release libs for common ABIs (adjust NDK path/targets as needed)
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
cargo ndk -t armeabi-v7a -t arm64-v8a -t x86 -t x86_64 -p 21 build --release
```

You’ll get `.so` files under `target/<triple>/release/` named like `librust_audio_player.so`. To consume them, create a small Android project, add JNI bindings as needed, and package into an AAR. The CI workflow uploads these `.so` files as artifacts on tag pushes.

### iOS

CI builds static libraries for device and simulator, then bundles them into an XCFramework. Locally you can do:

```powershell
rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim
cargo build --release --target aarch64-apple-ios
cargo build --release --target aarch64-apple-ios-sim
cargo build --release --target x86_64-apple-ios

# Create an XCFramework (macOS only)
lipo -create -output ios/librust_audio_player.sim.a target/aarch64-apple-ios-sim/release/librust_audio_player.a target/x86_64-apple-ios/release/librust_audio_player.a
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/librust_audio_player.a \
  -library ios/librust_audio_player.sim.a \
  -output ios/rust_audio_player.xcframework
```

Integrate the XCFramework into an Xcode project and provide Swift/Obj‑C glue code to drive the Rust API as needed.

## CI/CD

The GitHub Actions workflow `.github/workflows/release.yml` runs on tag pushes (`v*`) and produces:

- Android: `.so` libraries for arm64‑v8a, armeabi‑v7a, x86, x86_64
- iOS: `rust_audio_player.xcframework`

Artifacts are attached to the GitHub release created by the workflow.

## Notes and Limitations

- This branch removes desktop features such as folder pickers, persistent settings, and theme toggles.
- Permissions and file access on mobile are platform‑specific; the simple "music" directory approach is for testing only.
- For a production app, add JNI/Swift wrappers and proper packaging (AAR/Framework), app permissions, and a platform file‑access strategy.

## Credits

- [Slint](https://github.com/slint-ui/slint) for the UI
- [Rodio](https://github.com/RustAudio/rodio) and [Symphonia](https://github.com/pdeljanov/Symphonia) for audio playback and decoding

---

If you run into issues or have ideas for improvements, feel free to open an issue or PR.
