//! The public app configuration embedded at build time (see `build.rs`).

use clockin_sync::ServerConfig;

/// `dev` or `prod`: which Supabase project this build talks to.
pub const ENVIRONMENT: &str = env!("CLOCKIN_ENV");

/// The Supabase project URL and publishable key of this build, or `None`
/// if the build had no `.env.dev` / `.env.prod`.
#[must_use]
pub fn server_config() -> Option<ServerConfig> {
    let url = env!("CLOCKIN_SUPABASE_URL");
    let publishable_key = env!("CLOCKIN_SUPABASE_PUBLISHABLE_KEY");
    (!url.is_empty() && !publishable_key.is_empty()).then(|| ServerConfig {
        url: url.to_owned(),
        publishable_key: publishable_key.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_is_dev_or_prod() {
        assert!(ENVIRONMENT == "dev" || ENVIRONMENT == "prod");
    }

    #[test]
    fn embedded_config_is_https_when_present() {
        if let Some(config) = server_config() {
            assert!(config.base_url().starts_with("https://"));
            assert!(!config.publishable_key.is_empty());
        }
    }
}
