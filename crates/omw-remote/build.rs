//! Build script for `omw-remote`.
//!
//! Wiring sub-phase 3: builds the Web Controller SPA bundle so it can be
//! embedded into the daemon binary via `include_dir!`. Runs `npm install`
//! (idempotent, fast when `node_modules` already exists) followed by
//! `npm run build` in `apps/web-controller/` when the bundle is missing or
//! stale relative to its sources.
//!
//! Skipped when the `embedded-web-controller` feature is OFF, or when the
//! `OMW_SKIP_WEB_BUILD` env var is set (escape hatch for CI hosts without
//! Node — pair with `--no-default-features`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

fn main() {
    println!("cargo:rerun-if-env-changed=OMW_SKIP_WEB_BUILD");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_EMBEDDED_WEB_CONTROLLER");

    // Feature-off: nothing to do. `web_assets.rs` will not be compiled.
    if std::env::var_os("CARGO_FEATURE_EMBEDDED_WEB_CONTROLLER").is_none() {
        return;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root (two levels up from crate dir)");
    let wc_dir = workspace_root.join("apps").join("web-controller");
    let dist_dir = wc_dir.join("dist");
    let dist_index = dist_dir.join("index.html");

    // Watch the SPA source tree so cargo reruns this script when it changes.
    // We watch directories rather than every file — cargo picks up changes to
    // any file inside.
    for rel in [
        "apps/web-controller/src",
        "apps/web-controller/public",
        "apps/web-controller/index.html",
        "apps/web-controller/package.json",
        "apps/web-controller/package-lock.json",
        "apps/web-controller/vite.config.ts",
        "apps/web-controller/tsconfig.json",
        "apps/web-controller/tsconfig.node.json",
        "apps/web-controller/postcss.config.js",
        "apps/web-controller/tailwind.config.ts",
    ] {
        let p = workspace_root.join(rel);
        if p.exists() {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }

    // Escape hatch: explicit opt-out (e.g., CI hosts without Node).
    if std::env::var_os("OMW_SKIP_WEB_BUILD").is_some() {
        if !dist_index.exists() {
            // Without dist/, include_dir! will fail to compile. Surface a
            // clear error rather than letting that explode opaquely.
            panic!(
                "OMW_SKIP_WEB_BUILD is set but {} does not exist; \
                 either build the SPA manually (cd apps/web-controller && npm install && npm run build) \
                 or build with --no-default-features",
                dist_index.display()
            );
        }
        return;
    }

    if needs_rebuild(&dist_index, &wc_dir) {
        run_npm_build(&wc_dir);
    }

    if !dist_index.exists() {
        panic!(
            "expected {} after npm build, but it is missing",
            dist_index.display()
        );
    }
}

/// Check whether `dist/index.html` is missing or older than any tracked source.
fn needs_rebuild(dist_index: &Path, wc_dir: &Path) -> bool {
    let dist_mtime = match dist_index.metadata().and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return true,
    };

    // If any tracked source file is newer than dist/index.html, rebuild.
    let inputs = [
        wc_dir.join("package.json"),
        wc_dir.join("package-lock.json"),
        wc_dir.join("vite.config.ts"),
        wc_dir.join("tsconfig.json"),
        wc_dir.join("tsconfig.node.json"),
        wc_dir.join("index.html"),
    ];
    for p in &inputs {
        if newer_than(p, dist_mtime) {
            return true;
        }
    }
    if dir_has_newer_file(&wc_dir.join("src"), dist_mtime) {
        return true;
    }
    if dir_has_newer_file(&wc_dir.join("public"), dist_mtime) {
        return true;
    }
    false
}

fn newer_than(p: &Path, baseline: SystemTime) -> bool {
    match p.metadata().and_then(|m| m.modified()) {
        Ok(t) => t > baseline,
        Err(_) => false,
    }
}

fn dir_has_newer_file(dir: &Path, baseline: SystemTime) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ft = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };
        if ft.is_dir() {
            if dir_has_newer_file(&path, baseline) {
                return true;
            }
        } else if newer_than(&path, baseline) {
            return true;
        }
    }
    false
}

fn run_npm_build(wc_dir: &Path) {
    // `npm install` is idempotent and fast when node_modules is already
    // populated; running it unconditionally is the simplest way to make a
    // fresh checkout build cleanly.
    run_npm(
        wc_dir,
        &[
            "install",
            "--prefer-offline",
            "--no-audit",
            "--no-fund",
            "--progress=false",
        ],
    );
    run_npm(wc_dir, &["run", "build"]);
}

fn run_npm(wc_dir: &Path, args: &[&str]) {
    // `npm` on Windows is `npm.cmd`; spawning the wrong one yields ENOENT.
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm).args(args).current_dir(wc_dir).status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => panic!(
            "`{} {}` in {} exited with status {}",
            npm,
            args.join(" "),
            wc_dir.display(),
            s
        ),
        Err(e) => panic!(
            "failed to spawn `{} {}` in {}: {} \
             (if Node isn't available, set OMW_SKIP_WEB_BUILD=1 with a prebuilt dist/, \
             or build with --no-default-features)",
            npm,
            args.join(" "),
            wc_dir.display(),
            e
        ),
    }
}
