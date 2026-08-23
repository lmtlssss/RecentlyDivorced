use anyhow::{Context, Result, bail};
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use fs2::FileExt;
use std::fs;
use std::path::{Path, PathBuf};
use std::ffi::OsString;
use std::process::Command;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::Component;
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const INSTALLATION_FILE: &str = "INSTALLATION.toml";
pub const STOCK_RECORD_FILE: &str = "STOCK.toml";
pub const RELEASE_PUBLIC_KEY_B64: &str = "JSEpZCpysWpPUgtMbXPD2uHbu5xwrSlQVzukomD5RVQ=";
pub const RELEASE_BASE_URL: &str = "https://github.com/lmtlssss/RecentlyDivorced/releases/latest/download";

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ReleaseManifest {
    pub schema: u32,
    pub manager_version: String,
    pub payloads: Vec<ReleasePayload>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct ReleasePayload {
    pub stock_version: String,
    pub target: String,
    pub identity: String,
    pub asset: String,
    pub sha256: String,
}

impl ReleaseManifest {
    pub fn parse_verified(manifest: &[u8], detached_signature_b64: &str) -> Result<Self> {
        let key_bytes = base64::engine::general_purpose::STANDARD
            .decode(RELEASE_PUBLIC_KEY_B64)
            .context("decode embedded release public key")?;
        let key = VerifyingKey::from_bytes(
            key_bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("invalid embedded release public key"))?,
        )
        .context("parse embedded release public key")?;
        let signature_bytes = base64::engine::general_purpose::STANDARD
            .decode(detached_signature_b64.trim())
            .context("decode release manifest signature")?;
        let signature = Signature::from_slice(&signature_bytes).context("parse release manifest signature")?;
        key.verify(manifest, &signature)
            .context("release manifest signature verification failed")?;
        let parsed: Self = toml::from_str(std::str::from_utf8(manifest).context("release manifest is not UTF-8")?)
            .context("parse authenticated release manifest")?;
        if parsed.schema != 1
            || parsed.manager_version.is_empty()
            || parsed.payloads.iter().any(|payload| {
                payload.stock_version.is_empty()
                    || payload.target.is_empty()
                    || payload.target.contains('/')
                    || payload.identity.len() != 64
                    || !is_hex(&payload.sha256, 64)
                    || payload.asset.is_empty()
                    || payload.asset.contains('/')
            })
        {
            bail!("invalid authenticated release manifest")
        }
        Ok(parsed)
    }

    pub fn payload_for(&self, stock_version: &str, target: &str) -> Option<&ReleasePayload> {
        self.payloads
            .iter()
            .find(|payload| payload.stock_version == stock_version && payload.target == target)
    }
}

#[derive(Debug, serde::Serialize, Deserialize, PartialEq, Eq)]
pub struct Installation {
    pub schema: u32,
    pub installation_id: String,
    pub public_link: PathBuf,
    pub stock_link: PathBuf,
    pub target: String,
}

impl Installation {
    pub fn load(root: &Path) -> Result<Self> {
        let contents = fs::read_to_string(root.join(INSTALLATION_FILE)).context("read installation marker")?;
        let installation: Self = toml::from_str(&contents).context("parse installation marker")?;
        if installation.schema != 1
            || installation.installation_id.is_empty()
            || !installation.public_link.is_absolute()
            || !installation.stock_link.is_absolute()
            || installation.target.contains('/')
            || installation.target.is_empty()
        {
            bail!("invalid RecentlyDivorced installation marker")
        }
        Ok(installation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StockLink {
    pub original_target: PathBuf,
    pub dynamic_target: PathBuf,
}

#[derive(Debug, serde::Serialize, Deserialize, PartialEq, Eq)]
pub struct StockRecord {
    pub original_target: PathBuf,
    pub dynamic_target: PathBuf,
    pub resolved_target: PathBuf,
}

impl From<StockLink> for StockRecord {
    fn from(value: StockLink) -> Self {
        let resolved_target = fs::canonicalize(&value.dynamic_target).unwrap_or_default();
        Self { original_target: value.original_target, dynamic_target: value.dynamic_target, resolved_target }
    }
}

pub fn stock_changed(record: &StockRecord) -> Result<bool> {
    let current = fs::canonicalize(&record.dynamic_target).context("resolve dynamic stock target")?;
    Ok(current != record.resolved_target)
}

pub fn write_stock_record(root: &Path, record: &StockRecord) -> Result<()> {
    let encoded = toml::to_string(record).context("encode stock record")?;
    let stage = root.join(".STOCK.toml.new");
    fs::write(&stage, encoded).context("write stock record stage")?;
    fs::rename(stage, root.join(STOCK_RECORD_FILE)).context("publish stock record")?;
    Ok(())
}

pub fn load_stock_record(root: &Path) -> Result<StockRecord> {
    let contents = fs::read_to_string(root.join(STOCK_RECORD_FILE)).context("read stock record")?;
    toml::from_str(&contents).context("parse stock record")
}

pub fn initialize_installation(root: &Path, installation: &Installation, stock: StockLink) -> Result<()> {
    fs::create_dir_all(root.join("payloads")).context("create payload store")?;
    fs::create_dir_all(root.join("manager")).context("create manager store")?;
    let marker = toml::to_string(installation).context("encode installation marker")?;
    let stage = root.join(".INSTALLATION.toml.new");
    fs::write(&stage, marker).context("write installation marker stage")?;
    fs::rename(stage, root.join(INSTALLATION_FILE)).context("publish installation marker")?;
    write_stock_record(root, &stock.into())
}

pub fn publish_manager(root: &Path, source: &Path, version: &str) -> Result<PathBuf> {
    if version.is_empty() || version.contains('/') || !source.is_file() {
        bail!("invalid manager payload input")
    }
    let manager_root = root.join("manager");
    let payload = manager_root.join(version);
    fs::create_dir_all(&payload).context("create manager payload directory")?;
    let binary = payload.join("recentlydivorced");
    fs::copy(source, &binary).context("copy manager payload")?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).context("mark manager executable")?;
    let stage = manager_root.join(".current.new");
    symlink(version, &stage).context("stage manager pointer")?;
    fs::rename(stage, manager_root.join("current")).context("publish manager pointer")?;
    Ok(manager_root.join("current/recentlydivorced"))
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

pub fn intercepts_stock_update(args: &[OsString]) -> bool {
    args.first().is_some_and(|arg| arg == "update")
}

pub fn current_payload(root: &Path) -> Result<PathBuf> {
    let payload = fs::canonicalize(root.join("current/bin/codex")).context("resolve current payload")?;
    let payload_root = fs::canonicalize(root.join("payloads")).context("resolve payload root")?;
    if !payload.is_file() || !payload.starts_with(&payload_root) {
        bail!("current payload is outside the owned payload store")
    }
    Ok(payload)
}

pub fn promote_payload(root: &Path, payload_dir: &Path) -> Result<()> {
    let payload_root = fs::canonicalize(root.join("payloads")).context("resolve payload root")?;
    let payload_dir = fs::canonicalize(payload_dir).context("resolve candidate payload")?;
    if !payload_dir.starts_with(&payload_root) || !payload_dir.join("bin/codex").is_file() {
        bail!("candidate payload is not an owned executable")
    }
    let current = root.join("current");
    if let Ok(old) = fs::canonicalize(&current)
        && old != payload_dir
        && old.starts_with(&payload_root)
        && old.join("bin/codex").is_file()
    {
        let previous_stage = root.join(".previous.new");
        symlink(&old, &previous_stage).context("stage previous payload")?;
        fs::rename(previous_stage, root.join("previous")).context("publish previous payload")?;
    }
    let current_stage = root.join(".current.new");
    symlink(&payload_dir, &current_stage).context("stage current payload")?;
    fs::rename(current_stage, current).context("publish current payload")?;
    Ok(())
}

pub fn publish_codex_payload(root: &Path, source: &Path, identity: &str, target: &str) -> Result<PathBuf> {
    if !source.is_file() || identity.len() != 64 || target.is_empty() || target.contains('/') {
        bail!("invalid Codex payload input")
    }
    let payload = root.join("payloads").join(identity).join(target);
    let binary = payload.join("bin/codex");
    fs::create_dir_all(binary.parent().context("payload binary has no parent")?).context("create payload directory")?;
    fs::copy(source, &binary).context("copy Codex payload")?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).context("mark Codex payload executable")?;
    promote_payload(root, &payload)?;
    Ok(root.join("current/bin/codex"))
}

pub fn repair_public_link(installation: &Installation, manager: &Path, original_stock_target: &Path) -> Result<()> {
    let existing = fs::read_link(&installation.public_link).context("read public codex link")?;
    if existing != original_stock_target && !same_target(&installation.public_link, manager) {
        bail!("public codex link is owned by another target")
    }
    let parent = installation.public_link.parent().context("public link has no parent")?;
    let stage = parent.join(".codex-recentlydivorced.new");
    symlink(manager, &stage).context("stage public manager link")?;
    fs::rename(stage, &installation.public_link).context("publish public manager link")?;
    Ok(())
}

pub fn restore_stock_link(installation: &Installation, manager: &Path, stock: &StockRecord) -> Result<()> {
    fs::read_link(&installation.public_link).context("read public codex link")?;
    if !same_target(&installation.public_link, manager) {
        bail!("public codex link is no longer RecentlyDivorced-owned")
    }
    let parent = installation.public_link.parent().context("public link has no parent")?;
    let stage = parent.join(".codex-stock.new");
    symlink(&stock.original_target, &stage).context("stage stock link restore")?;
    fs::rename(stage, &installation.public_link).context("restore stock codex link")?;
    Ok(())
}

fn same_target(link: &Path, target: &Path) -> bool {
    fs::canonicalize(link).ok() == fs::canonicalize(target).ok()
}

pub fn dispatch_target(root: &Path, args: &[OsString]) -> Result<PathBuf> {
    Installation::load(root)?;
    if intercepts_stock_update(args) {
        bail!("stock update reconciliation is not installed yet")
    }
    current_payload(root)
}

pub fn run_stock_update(installation: &Installation, args_after_update: &[OsString]) -> Result<()> {
    let status = Command::new(&installation.stock_link)
        .arg("update")
        .args(args_after_update)
        .status()
        .context("run stock codex update")?;
    if !status.success() {
        bail!("stock codex update failed: {status}")
    }
    Ok(())
}

pub fn reconcile_stock_update(root: &Path, installation: &Installation, args_after_update: &[OsString]) -> Result<()> {
    let lock_path = root.join("lifecycle.lock");
    let lock = fs::OpenOptions::new().create(true).read(true).write(true).open(&lock_path)
        .context("open RecentlyDivorced lifecycle lock")?;
    lock.try_lock_exclusive().context("RecentlyDivorced update already in progress")?;
    run_stock_update(installation, args_after_update)?;
    let stock_version = read_stock_version(&installation.stock_link)?;
    let payload = download_matching_payload(root, &stock_version, &installation.target)?;
    let identity = payload.0;
    let source = payload.1;
    publish_codex_payload(root, &source, &identity, &installation.target)?;
    let stock = load_stock_record(root)?;
    write_stock_record(root, &StockRecord {
        resolved_target: fs::canonicalize(&installation.stock_link).context("resolve updated stock target")?,
        ..stock
    })?;
    Ok(())
}

fn read_stock_version(stock: &Path) -> Result<String> {
    let output = Command::new(stock).arg("--version").output().context("read updated stock Codex version")?;
    if !output.status.success() {
        bail!("updated stock Codex refused --version")
    }
    let text = String::from_utf8(output.stdout).context("updated stock version is not UTF-8")?;
    text.split_whitespace()
        .find(|word| word.chars().next().is_some_and(|ch| ch.is_ascii_digit()) && word.chars().all(|ch| ch.is_ascii_digit() || ch == '.'))
        .map(str::to_owned)
        .context("could not parse updated stock Codex version")
}

fn download_matching_payload(root: &Path, stock_version: &str, target: &str) -> Result<(String, PathBuf)> {
    let incoming = root.join(format!(".incoming-{}", std::process::id()));
    fs::create_dir_all(&incoming).context("create release staging directory")?;
    let manifest = incoming.join("release.toml");
    let signature = incoming.join("release.toml.sig");
    curl_to(&format!("{RELEASE_BASE_URL}/release.toml"), &manifest)?;
    curl_to(&format!("{RELEASE_BASE_URL}/release.toml.sig"), &signature)?;
    let manifest_bytes = fs::read(&manifest).context("read downloaded release manifest")?;
    let release = ReleaseManifest::parse_verified(&manifest_bytes, &fs::read_to_string(&signature).context("read release signature")?)?;
    let payload = release.payload_for(stock_version, target)
        .with_context(|| format!("no compatible RecentlyDivorced payload for Codex {stock_version} on {target}; keeping last known-good patched Codex"))?;
    let staged = incoming.join(&payload.asset);
    curl_to(&format!("{RELEASE_BASE_URL}/{}", payload.asset), &staged)?;
    let bytes = fs::read(&staged).context("read downloaded payload")?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    if format!("{:x}", hasher.finalize()) != payload.sha256 {
        bail!("downloaded payload hash does not match authenticated release manifest")
    }
    fs::set_permissions(&staged, fs::Permissions::from_mode(0o755)).context("mark downloaded payload executable")?;
    if read_stock_version(&staged)? != stock_version {
        bail!("downloaded payload version does not match updated stock Codex")
    }
    Ok((payload.identity.clone(), staged))
}

fn curl_to(url: &str, destination: &Path) -> Result<()> {
    let status = Command::new("curl").args(["--fail", "--silent", "--show-error", "--location", "--output"])
        .arg(destination).arg(url).status().context("run curl for RecentlyDivorced release")?;
    if !status.success() { bail!("could not download RecentlyDivorced release artifact") }
    Ok(())
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
        Installation::load(&root)?;
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
        fs::write(
            root.join(INSTALLATION_FILE),
            format!(
                "schema=1\ninstallation_id='test'\npublic_link='{}'\nstock_link='{}'\ntarget='x86_64-unknown-linux-gnu'\n",
                root.join("bin/codex").display(),
                root.join("stock/codex").display(),
            ),
        ).unwrap();
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
    fn release_manifest_rejects_an_unsigned_payload_map() {
        let manifest = b"schema=1\nmanager_version='0.1.0'\npayloads=[]\n";
        assert!(ReleaseManifest::parse_verified(manifest, "not-a-signature").is_err());
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

    #[test]
    fn installation_marker_rejects_relative_public_paths() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join(INSTALLATION_FILE),
            "schema=1\ninstallation_id='abc'\npublic_link='bin/codex'\nstock_link='/tmp/stock'\ntarget='x86_64-unknown-linux-gnu'\n",
        ).unwrap();
        assert!(Installation::load(temp.path()).is_err());
    }

    #[test]
    fn only_exact_update_is_intercepted() {
        assert!(intercepts_stock_update(&[OsString::from("update")]));
        assert!(!intercepts_stock_update(&[OsString::from("exec"), OsString::from("update")]));
        assert!(!intercepts_stock_update(&[OsString::from("--version")]));
    }

    #[test]
    fn current_payload_must_stay_in_owned_store() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("recentlydivorced");
        let payload = root.join("payloads/a/bin/codex");
        fs::create_dir_all(payload.parent().unwrap()).unwrap();
        fs::write(&payload, "payload").unwrap();
        fs::create_dir_all(root.join("current").parent().unwrap()).unwrap();
        symlink("payloads/a", root.join("current")).unwrap();
        assert_eq!(current_payload(&root).unwrap(), payload);
    }

    #[test]
    fn normal_dispatch_preserves_non_update_path() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("recentlydivorced");
        let payload = root.join("payloads/a/bin/codex");
        fs::create_dir_all(payload.parent().unwrap()).unwrap();
        fs::write(&payload, "payload").unwrap();
        symlink("payloads/a", root.join("current")).unwrap();
        fs::write(
            root.join(INSTALLATION_FILE),
            format!("schema=1\ninstallation_id='test'\npublic_link='{}'\nstock_link='{}'\ntarget='x86_64-unknown-linux-gnu'\n", root.join("bin/codex").display(), root.join("stock/codex").display()),
        ).unwrap();
        assert_eq!(dispatch_target(&root, &[OsString::from("exec")]).unwrap(), payload);
        assert!(dispatch_target(&root, &[OsString::from("update")]).is_err());
    }

    #[test]
    fn stock_update_runs_only_recorded_stock_binary() {
        let installation = Installation {
            schema: 1,
            installation_id: "test".into(),
            public_link: PathBuf::from("/tmp/public"),
            stock_link: PathBuf::from("/bin/true"),
            target: "x86_64-unknown-linux-gnu".into(),
        };
        run_stock_update(&installation, &[OsString::from("--check")]).unwrap();
    }

    #[test]
    fn stock_record_round_trips_without_resolving_current() {
        let temp = tempfile::tempdir().unwrap();
        let record = StockRecord {
            original_target: PathBuf::from("../stock/current/codex"),
            dynamic_target: temp.path().join("stock/current/codex"),
            resolved_target: temp.path().join("stock/releases/0.1/codex"),
        };
        write_stock_record(temp.path(), &record).unwrap();
        let decoded: StockRecord = toml::from_str(&fs::read_to_string(temp.path().join(STOCK_RECORD_FILE)).unwrap()).unwrap();
        assert_eq!(decoded, record);
        assert_eq!(load_stock_record(temp.path()).unwrap(), record);
    }

    #[test]
    fn stock_change_detects_nested_current_move() {
        let temp = tempfile::tempdir().unwrap();
        let release = temp.path().join("releases/0.1/codex");
        fs::create_dir_all(release.parent().unwrap()).unwrap();
        fs::write(&release, "stock").unwrap();
        let current = temp.path().join("current");
        symlink("releases/0.1", &current).unwrap();
        let mut record = StockRecord {
            original_target: PathBuf::from("current/codex"), dynamic_target: current.join("codex"), resolved_target: fs::canonicalize(current.join("codex")).unwrap(),
        };
        assert!(!stock_changed(&record).unwrap());
        let next = temp.path().join("releases/0.2/codex");
        fs::create_dir_all(next.parent().unwrap()).unwrap(); fs::write(&next, "stock").unwrap();
        fs::remove_file(&current).unwrap(); symlink("releases/0.2", &current).unwrap();
        assert!(stock_changed(&record).unwrap());
        record.resolved_target = fs::canonicalize(current.join("codex")).unwrap();
        assert!(!stock_changed(&record).unwrap());
    }

    #[test]
    fn bootstrap_writes_only_owned_manager_state() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("rd");
        let stock_path = temp.path().join("stock/codex");
        fs::create_dir_all(stock_path.parent().unwrap()).unwrap(); fs::write(&stock_path, "stock").unwrap();
        let installation = Installation { schema: 1, installation_id: "test".into(), public_link: temp.path().join("bin/codex"), stock_link: stock_path.clone(), target: "x86_64-unknown-linux-gnu".into() };
        initialize_installation(&root, &installation, StockLink { original_target: stock_path.clone(), dynamic_target: stock_path.clone() }).unwrap();
        assert_eq!(Installation::load(&root).unwrap(), installation);
        assert!(root.join("payloads").is_dir());
        assert!(root.join(STOCK_RECORD_FILE).is_file());
    }

    #[test]
    fn promotion_keeps_last_good_payload() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("rd");
        let first = root.join("payloads/first"); let second = root.join("payloads/second");
        for payload in [&first, &second] { fs::create_dir_all(payload.join("bin")).unwrap(); fs::write(payload.join("bin/codex"), "payload").unwrap(); }
        promote_payload(&root, &first).unwrap();
        promote_payload(&root, &second).unwrap();
        assert_eq!(fs::canonicalize(root.join("current")).unwrap(), fs::canonicalize(&second).unwrap());
        assert_eq!(fs::canonicalize(root.join("previous")).unwrap(), fs::canonicalize(&first).unwrap());
    }

    #[test]
    fn repair_reclaims_only_original_stock_link() {
        let temp = tempfile::tempdir().unwrap();
        let public = temp.path().join("bin/codex"); fs::create_dir_all(public.parent().unwrap()).unwrap();
        let stock = temp.path().join("stock/current/codex"); fs::create_dir_all(stock.parent().unwrap()).unwrap(); fs::write(&stock, "stock").unwrap();
        let manager = temp.path().join("rd/manager/current/recentlydivorced"); fs::create_dir_all(manager.parent().unwrap()).unwrap(); fs::write(&manager, "manager").unwrap();
        symlink("../stock/current/codex", &public).unwrap();
        let installation = Installation { schema: 1, installation_id: "test".into(), public_link: public.clone(), stock_link: stock, target: "x86_64-unknown-linux-gnu".into() };
        repair_public_link(&installation, &manager, &PathBuf::from("../stock/current/codex")).unwrap();
        assert_eq!(fs::read_link(public).unwrap(), manager);
    }

    #[test]
    fn manager_publication_is_versioned_and_executable() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("rd");
        fs::create_dir_all(root.join("manager")).unwrap();
        let source = temp.path().join("source"); fs::write(&source, "manager").unwrap();
        let published = publish_manager(&root, &source, "0.1.0").unwrap();
        assert!(published.is_file());
        assert_eq!(fs::canonicalize(published).unwrap(), root.join("manager/0.1.0/recentlydivorced"));
    }

    #[test]
    fn codex_payload_is_owned_before_promotion() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("rd"); fs::create_dir_all(root.join("payloads")).unwrap();
        let source = temp.path().join("source"); fs::write(&source, "payload").unwrap();
        let identity = "a".repeat(64);
        let current = publish_codex_payload(&root, &source, &identity, "x86_64-unknown-linux-gnu").unwrap();
        assert!(current.is_file());
        assert!(current.starts_with(&root));
    }

    #[test]
    fn uninstall_restores_only_owned_public_link() {
        let temp = tempfile::tempdir().unwrap();
        let public = temp.path().join("bin/codex"); fs::create_dir_all(public.parent().unwrap()).unwrap();
        let manager = temp.path().join("rd/manager/current/recentlydivorced"); fs::create_dir_all(manager.parent().unwrap()).unwrap(); fs::write(&manager, "manager").unwrap();
        symlink(&manager, &public).unwrap();
        let installation = Installation { schema: 1, installation_id: "test".into(), public_link: public.clone(), stock_link: temp.path().join("stock"), target: "x86_64-unknown-linux-gnu".into() };
        let stock = StockRecord { original_target: PathBuf::from("../stock/current/codex"), dynamic_target: temp.path().join("stock/current/codex"), resolved_target: temp.path().join("stock/releases/one/codex") };
        restore_stock_link(&installation, &manager, &stock).unwrap();
        assert_eq!(fs::read_link(public).unwrap(), stock.original_target);
    }
}
