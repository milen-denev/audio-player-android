// Minimal Android entrypoint for cargo-apk using ndk-glue.
// This will be invoked as the NativeActivity entrypoint on Android.

#[cfg_attr(target_os = "android", ndk_glue::main(backtrace = "on"))]
pub fn main() {
    #[cfg(target_os = "android")]
    {
        // Initialize logging to logcat
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::Level::Error)
                .with_tag("rust-audio-player"),
        );
        log::info!("Android main() started");
        // Prefer software renderer to avoid GL issues that can cause a black screen
        unsafe { std::env::set_var("SLINT_RENDERER", "software") };
    }

    if let Err(e) = rust_audio_player_android::run_app() {
        log::error!("App error: {e}");
        eprintln!("App error: {e}");
    }
}
