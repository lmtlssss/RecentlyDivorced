use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

pub const INSTALLATION_FILE: &str = "INSTALLATION.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRoot {
    pub root: PathBuf,
    pub manager: PathBuf,
}

impl InstallRoot {
    pub fn from_manager_path(path: &Path) -> Result<Self> {
        let manager = fs::canonicalize(path).context("resolve manager executable")?;
        let root = manager
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("manager is not stored beneath manager/current")?
            .to_path_buf();
        let marker = root.join(INSTALLATION_FILE);
        if !marker.is_file() {
            bail!("RecentlyDivorced installation marker is missing: {}", marker.display());
        }
        Ok(Self { root, manager })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn discovers_root_only_from_owned_manager_layout() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("recentlydivorced");
        let version = root.join("manager/0.1.0");
        fs::create_dir_all(&version).unwrap();
        fs::write(root.join(INSTALLATION_FILE), "schema = 1\n").unwrap();
        let binary = version.join("recentlydivorced");
        fs::write(&binary, "manager").unwrap();
        let current = root.join("manager/current");
        symlink("0.1.0", &current).unwrap();
        let discovered = InstallRoot::from_manager_path(&current.join("recentlydivorced")).unwrap();
        assert_eq!(discovered.root, root);
    }
}
