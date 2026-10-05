//! Embeds the public app configuration (Supabase project URL and publishable
//! key) from `.env.dev` or `.env.prod` at the repo root.
//!
//! - Debug builds use `.env.dev`, release builds `.env.prod`.
//! - `CLOCKIN_ENV=dev` or `CLOCKIN_ENV=prod` overrides that.
//! - A missing file embeds empty values: the app then reports that it is not
//!   configured (CI builds have no `.env` files).
//!
//! Only public values belong in these files (CLAUDE.md); never a database
//! URL, password or secret key.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=CLOCKIN_ENV");
    // tauri-build embeds this into the .exe but doesn't watch it (`pnpm icons`).
    println!("cargo:rerun-if-changed=icons/icon.ico");
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let env = std::env::var("CLOCKIN_ENV")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| if profile == "release" { "prod" } else { "dev" }.to_owned());
    assert!(
        env == "dev" || env == "prod",
        "CLOCKIN_ENV must be `dev` or `prod`"
    );

    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(format!(".env.{env}"));
    println!("cargo:rerun-if-changed={}", file.display());
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let value = |name: &str| {
        text.lines()
            .filter_map(|l| l.trim().strip_prefix(name)?.trim_start().strip_prefix('='))
            .map(str::trim)
            .find(|v| !v.is_empty())
            .unwrap_or_default()
            .to_owned()
    };
    println!("cargo:rustc-env=CLOCKIN_ENV={env}");
    println!(
        "cargo:rustc-env=CLOCKIN_SUPABASE_URL={}",
        value("SUPABASE_URL")
    );
    println!(
        "cargo:rustc-env=CLOCKIN_SUPABASE_PUBLISHABLE_KEY={}",
        value("SUPABASE_PUBLISHABLE_KEY")
    );

    tauri_build::build();
}
