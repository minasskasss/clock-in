// Commands are added in Phase 5, together with the Kotlin side in `android/`.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
