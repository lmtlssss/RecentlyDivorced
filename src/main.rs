use anyhow::Result;
use clap::Parser;
use std::env;
use std::path::PathBuf;
use std::path::Path;
use std::os::unix::process::CommandExt;
use std::process::Command;

#[derive(Parser, Debug)]
#[command(disable_help_subcommand = true)]
struct Bootstrap {
    #[arg(long, hide = true)]
    rd_bootstrap_install: bool,
    #[arg(long, hide = true)]
    rd_bootstrap_uninstall: bool,
    #[arg(long, hide = true)]
    rd_repair: bool,
    #[arg(long, hide = true)]
    rd_root: Option<PathBuf>,
    #[arg(long, hide = true)]
    rd_public_link: Option<PathBuf>,
    #[arg(long, hide = true)]
    rd_target: Option<String>,
    #[arg(long, hide = true)]
    rd_installation_id: Option<String>,
    #[arg(long, hide = true)]
    rd_payload: Option<PathBuf>,
    #[arg(long, hide = true)]
    rd_payload_identity: Option<String>,
}

fn main() -> Result<()> {
    let invoked_as = env::args_os().next().unwrap_or_default();
    if Path::new(&invoked_as).file_name().is_some_and(|name| name == "codex") {
        return run_codex();
    }
    let bootstrap = Bootstrap::parse();
    if bootstrap.rd_bootstrap_install {
        let root = bootstrap.rd_root.ok_or_else(|| anyhow::anyhow!("missing bootstrap root"))?;
        let public_link = bootstrap.rd_public_link.ok_or_else(|| anyhow::anyhow!("missing public codex link"))?;
        let target = bootstrap.rd_target.ok_or_else(|| anyhow::anyhow!("missing target"))?;
        let installation_id = bootstrap.rd_installation_id.ok_or_else(|| anyhow::anyhow!("missing installation id"))?;
        let payload = bootstrap.rd_payload.ok_or_else(|| anyhow::anyhow!("missing patched Codex payload"))?;
        let payload_identity = bootstrap.rd_payload_identity.ok_or_else(|| anyhow::anyhow!("missing payload identity"))?;
        let stock = recentlydivorced::StockLink::capture(&public_link, &root)?;
        let installation = recentlydivorced::Installation { schema: 1, installation_id, public_link, stock_link: stock.dynamic_target.clone(), target };
        recentlydivorced::initialize_installation(&root, &installation, stock.clone())?;
        let manager = recentlydivorced::publish_manager(&root, &env::current_exe()?, env!("CARGO_PKG_VERSION"))?;
        recentlydivorced::publish_codex_payload(&root, &payload, &payload_identity, &installation.target)?;
        recentlydivorced::repair_public_link(&installation, &manager, &stock.original_target)?;
        return Ok(());
    }
    if bootstrap.rd_bootstrap_uninstall {
        let manager = env::current_exe()?;
        let root = recentlydivorced::InstallRoot::from_manager_path(&manager)?;
        let installation = recentlydivorced::Installation::load(&root.root)?;
        let stock = recentlydivorced::load_stock_record(&root.root)?;
        recentlydivorced::restore_stock_link(&installation, &manager, &stock)?;
        return Ok(());
    }
    if bootstrap.rd_repair {
        let manager = env::current_exe()?;
        let root = recentlydivorced::InstallRoot::from_manager_path(&manager)?;
        let installation = recentlydivorced::Installation::load(&root.root)?;
        let stock = recentlydivorced::load_stock_record(&root.root)?;
        recentlydivorced::repair_public_link(&installation, &manager, &stock.original_target)?;
        return Ok(());
    }
    anyhow::bail!("RecentlyDivorced is installed through its curl bootstrap; run codex normally")
}

fn run_codex() -> Result<()> {
    let manager = env::current_exe()?;
    let root = recentlydivorced::InstallRoot::from_manager_path(&manager)?;
    let args: Vec<_> = env::args_os().skip(1).collect();
    let installation = recentlydivorced::Installation::load(&root.root)?;
    if recentlydivorced::intercepts_stock_update(&args) {
        recentlydivorced::reconcile_stock_update(&root.root, &installation, &args[1..])?;
        let stock = recentlydivorced::load_stock_record(&root.root)?;
        recentlydivorced::repair_public_link(&installation, &manager, &stock.original_target)?;
        return Ok(());
    }
    let payload = recentlydivorced::dispatch_target(&root.root, &args)?;
    Err(Command::new(payload).args(args).exec().into())
}
