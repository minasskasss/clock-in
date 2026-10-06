// The Kotlin commands are called from Rust only (`src/mobile.rs`), never from
// the webview, so no command is exposed through permissions.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
