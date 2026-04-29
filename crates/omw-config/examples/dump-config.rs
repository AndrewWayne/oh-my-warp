//! Tiny diagnostic example. Resolves the omw config path, loads + validates,
//! and prints a one-screen summary. Useful for sanity-checking a hand-edited
//! config file.
//!
//! ```sh
//! cargo run -p omw-config --example dump-config
//! OMW_CONFIG=/tmp/omw.toml cargo run -p omw-config --example dump-config
//! ```

use omw_config::{config_path, Config};

fn main() {
    let path = match config_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("could not resolve config path: {e}");
            std::process::exit(2);
        }
    };
    eprintln!("config path: {}", path.display());

    match Config::load() {
        Ok(cfg) => {
            println!("version: {}", cfg.version.0);
            println!(
                "default_provider: {}",
                cfg.default_provider
                    .as_ref()
                    .map(|p| p.as_str())
                    .unwrap_or("(unset)")
            );
            println!("providers ({}):", cfg.providers.len());
            for id in cfg.providers.keys() {
                println!("  - {id}");
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
