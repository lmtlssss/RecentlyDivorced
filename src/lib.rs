use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::path::Component;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const INSTALLATION_FILE: &str = "INSTALLATION.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockLink {
    pub original_target: PathBuf,
    pub dynamic_target: PathBuf,
}

impl StockLink {
    pub fn capture(public_link: &Path, install_root: &Path) -> Result<Self> {
        let original_target = fs::read_link(public_link).context("read stock codex link")?;
        let dynamic_target = if original_target.is_absolute() {
            original_target.clone()
        } else {
            public_link.parent().context("stock link has no parent")?.join(&original_target)
        };
        let dynamic_target = normalize_lexical(&dynamic_target);
        if !dynamic_target.is_file() || dynamic_target.starts_with(install_root) {
            bail!("stock Codex link is not an external executable")
        }
        Ok(Self { original_target, dynamic_target })
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => { normalized.pop(); }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct UpstreamLock {
    pub schema: u32,
    pub repo: String,
    pub commit: String,
    pub stock_version: String,
    pub target: String,
    pub patches: Vec<String>,
}

impl UpstreamLock {
    pub fn parse(input: &str) -> Result<Self> {
        let lock: Self = toml::from_str(input).context("parse upstream.lock")?;
        if lock.schema != 1
            || !is_hex(&lock.commit, 40)
            || lock.stock_version.is_empty()
            || !lock.target.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            || lock.patches.is_empty()
            || lock.patches.iter().any(|hash| !is_hex(hash, 64))
        {
            bail!("invalid RecentlyDivorced upstream lock")
        }
        Ok(lock)
    }

    pub fn identity(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.commit.as_bytes());
        for patch in &self.patches {
            hasher.update([0]);
            hasher.update(patch.as_bytes());
        }
        format!("{:x}", hasher.finalize())
    }
}

fn is_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

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

    #[test]
    fn lock_identity_binds_commit_and_ordered_patch_set() {
        let lock = UpstreamLock::parse(
            "schema=1\nrepo='https://github.com/openai/codex.git'\ncommit='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\nstock_version='0.149.0'\ntarget='x86_64-unknown-linux-gnu'\npatches=['bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb']\n",
        ).unwrap();
        assert_eq!(lock.identity().len(), 64);
        assert!(UpstreamLock::parse("schema=2\nrepo='x'\ncommit='bad'\nstock_version='x'\ntarget='../bad'\npatches=[]\n").is_err());
    }

    #[test]
    fn stock_capture_preserves_dynamic_symlink_target() {
        let temp = tempfile::tempdir().unwrap();
        let stock = temp.path().join("stock/current/codex");
        fs::create_dir_all(stock.parent().unwrap()).unwrap();
        fs::write(&stock, "stock").unwrap();
        let public = temp.path().join("bin/codex");
        fs::create_dir_all(public.parent().unwrap()).unwrap();
        symlink("../stock/current/codex", &public).unwrap();
        let link = StockLink::capture(&public, &temp.path().join("recentlydivorced")).unwrap();
        assert_eq!(link.original_target, PathBuf::from("../stock/current/codex"));
        assert_eq!(link.dynamic_target, stock);
    }
}
