//! Tests for the canonical-AGENTS.md helpers in `omw_config::lib`.
//!
//! All env-touching assertions are serialised into a single `#[test]`
//! because `std::env` is process-global; mirrors the existing
//! `config_path_resolution` pattern.

use std::path::PathBuf;

use omw_config::{
    agents_md_path, bootstrap_agents_md_if_missing, read_agents_md, sync_agents_md,
    AGENTS_MD_MAX_BYTES, DEFAULT_AGENTS_MD,
};

#[test]
fn agents_md_resolution_and_io() {
    let restore = std::env::var_os("OMW_AGENTS_MD_PATH");
    let dir = tempfile::tempdir().unwrap();

    // -------- agents_md_path env override --------
    let canonical = dir.path().join("subdir").join("AGENTS.md");
    std::env::set_var("OMW_AGENTS_MD_PATH", &canonical);
    assert_eq!(agents_md_path().unwrap(), canonical);

    // Empty value is treated as unset (would resolve to $HOME/...).
    std::env::set_var("OMW_AGENTS_MD_PATH", "");
    let resolved = agents_md_path().unwrap();
    assert_ne!(
        resolved, PathBuf::new(),
        "empty OMW_AGENTS_MD_PATH must fall back, not return empty path"
    );
    // Re-arm for the rest of the test.
    std::env::set_var("OMW_AGENTS_MD_PATH", &canonical);

    // -------- read_agents_md: missing canonical --------
    assert!(!canonical.exists());
    assert_eq!(read_agents_md().unwrap(), None);

    // -------- read_agents_md: happy path --------
    std::fs::create_dir_all(canonical.parent().unwrap()).unwrap();
    std::fs::write(&canonical, "hello agents").unwrap();
    assert_eq!(read_agents_md().unwrap(), Some("hello agents".to_string()));

    // -------- read_agents_md: oversize → None (warn-skip) --------
    let oversize = "x".repeat((AGENTS_MD_MAX_BYTES + 1) as usize);
    std::fs::write(&canonical, &oversize).unwrap();
    assert_eq!(read_agents_md().unwrap(), None);

    // Reset canonical to a small file for sync tests below.
    std::fs::write(&canonical, "preexisting canonical").unwrap();

    // -------- sync_agents_md: source = None → no-op --------
    assert_eq!(sync_agents_md(None).unwrap(), None);
    assert_eq!(
        std::fs::read_to_string(&canonical).unwrap(),
        "preexisting canonical",
        "sync(None) must not touch the canonical file"
    );

    // -------- sync_agents_md: missing source → None (no error) --------
    let missing = dir.path().join("ghost.md");
    assert!(!missing.exists());
    assert_eq!(sync_agents_md(Some(&missing)).unwrap(), None);
    assert_eq!(
        std::fs::read_to_string(&canonical).unwrap(),
        "preexisting canonical",
        "sync(missing) must not touch the canonical file"
    );

    // -------- sync_agents_md: oversize source → None (canonical untouched) --------
    let oversize_src = dir.path().join("oversize.md");
    std::fs::write(&oversize_src, &oversize).unwrap();
    assert_eq!(sync_agents_md(Some(&oversize_src)).unwrap(), None);
    assert_eq!(
        std::fs::read_to_string(&canonical).unwrap(),
        "preexisting canonical",
        "sync(oversize) must not touch the canonical file"
    );

    // -------- sync_agents_md: happy path --------
    let user_src = dir.path().join("my-agents.md");
    std::fs::write(&user_src, "from user file").unwrap();
    let copied = sync_agents_md(Some(&user_src)).unwrap();
    assert_eq!(copied, Some("from user file".to_string()));
    assert_eq!(
        std::fs::read_to_string(&canonical).unwrap(),
        "from user file",
        "sync(happy) must overwrite canonical with source contents"
    );

    // -------- sync_agents_md: parent dir missing → created on demand --------
    let nested_canonical = dir.path().join("brand-new").join("nested").join("AGENTS.md");
    std::env::set_var("OMW_AGENTS_MD_PATH", &nested_canonical);
    assert!(!nested_canonical.parent().unwrap().exists());
    let copied = sync_agents_md(Some(&user_src)).unwrap();
    assert_eq!(copied, Some("from user file".to_string()));
    assert!(
        nested_canonical.exists(),
        "sync must create missing parent directories"
    );
    assert_eq!(
        std::fs::read_to_string(&nested_canonical).unwrap(),
        "from user file"
    );

    // -------- bootstrap_agents_md_if_missing --------
    let bootstrap_target = dir.path().join("first-run").join("AGENTS.md");
    std::env::set_var("OMW_AGENTS_MD_PATH", &bootstrap_target);
    assert!(!bootstrap_target.exists());
    let wrote = bootstrap_agents_md_if_missing().unwrap();
    assert!(wrote, "first call must create the canonical file");
    assert_eq!(
        std::fs::read_to_string(&bootstrap_target).unwrap(),
        DEFAULT_AGENTS_MD,
        "first-run file must contain the bundled baseline"
    );

    // Idempotent: a second call must NOT overwrite a user-edited file.
    std::fs::write(&bootstrap_target, "user edited this").unwrap();
    let wrote_again = bootstrap_agents_md_if_missing().unwrap();
    assert!(!wrote_again, "second call must be a no-op when file exists");
    assert_eq!(
        std::fs::read_to_string(&bootstrap_target).unwrap(),
        "user edited this",
        "bootstrap must not overwrite an existing file"
    );

    // -------- bundled default is non-empty + within cap --------
    assert!(!DEFAULT_AGENTS_MD.is_empty());
    assert!(
        (DEFAULT_AGENTS_MD.len() as u64) <= AGENTS_MD_MAX_BYTES,
        "bundled default must fit under the cap"
    );

    // -------- restore env --------
    match restore {
        Some(v) => std::env::set_var("OMW_AGENTS_MD_PATH", v),
        None => std::env::remove_var("OMW_AGENTS_MD_PATH"),
    }
}
