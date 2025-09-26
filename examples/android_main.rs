// Minimal Android entrypoint for cargo-apk using ndk-glue.
// This will be invoked as the NativeActivity entrypoint on Android.

#[cfg_attr(target_os = "android", ndk_glue::main(backtrace = "on"))]
pub fn main() {
    if let Err(e) = rust_audio_player_android::run_app() {
        eprintln!("App error: {e}");
    }
}
