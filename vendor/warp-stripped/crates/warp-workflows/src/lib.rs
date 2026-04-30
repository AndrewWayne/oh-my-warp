use serde::{Deserialize, Serialize};

/// Minimal local replacement for Warp's hosted workflows catalog.
///
/// The stripped `omw_local` build keeps the workflow data model so local and
/// user-defined workflows still work, but it intentionally does not bundle or
/// fetch Warp's upstream hosted workflow set.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct Workflow {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<Argument>,
    pub source_url: Option<String>,
    pub author: Option<String>,
    pub author_url: Option<String>,
    #[serde(default)]
    pub shells: Vec<Shell>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub struct Argument {
    pub name: String,
    pub description: Option<String>,
    pub default_value: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Shell {
    Bash,
    Fish,
    PowerShell,
    Sh,
    Zsh,
}

/// Returns the bundled set of global workflows.
///
/// In the stripped local build this is intentionally empty: there is no hosted
/// Warp workflows catalog and no dependency on the upstream repo.
pub fn workflows() -> Vec<Workflow> {
    Vec::new()
}
