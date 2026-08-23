use anyhow::Result;
use clap::Parser;
use std::env;
use std::os::unix::process::CommandExt;
use std::process::Command;

#[derive(Parser, Debug)]
#[command(disable_help_subcommand = true)]
struct Bootstrap {
    #[arg(long, hide = true)]
    rd_bootstrap_install: bool,
    #[arg(long, hide = true)]
    rd_bootstrap_uninstall: bool,
}

fn main() -> Result<()> {
    let invoked_as = env::args_os().next().unwrap_or_default();
    if invoked_as.to_string_lossy() == "codex" {
        return run_codex();
    }
    let bootstrap = Bootstrap::parse();
    if bootstrap.rd_bootstrap_install || bootstrap.rd_bootstrap_uninstall {
        anyhow::bail!("bootstrap implementation is not installed yet")
    }
    anyhow::bail!("RecentlyDivorced is installed through its curl bootstrap; run codex normally")
}

fn run_codex() -> Result<()> {
    let manager = env::current_exe()?;
    let root = recentlydivorced::InstallRoot::from_manager_path(&manager)?;
    let args: Vec<_> = env::args_os().skip(1).collect();
    let payload = recentlydivorced::dispatch_target(&root.root, &args)?;
    Err(Command::new(payload).args(args).exec().into())
}
