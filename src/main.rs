use anyhow::Result;
use cargo::{
    core::Workspace, util::important_paths::find_root_manifest_for_wd, CargoResult, Config,
};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};
use structopt::{clap::AppSettings, StructOpt};

mod collect;

#[derive(StructOpt)]
#[structopt(bin_name = "cargo")]
enum CliOpts {
    #[structopt(
        name = "gc-target",
        setting = AppSettings::UnifiedHelpMessage,
    )]
    GcTarget(CliArgs),
}

#[derive(StructOpt)]
struct CliArgs {
    /// Path to Cargo.toml
    #[structopt(long = "manifest-path", value_name = "PATH", parse(from_os_str))]
    manifest_path: Option<PathBuf>,
    /// Path to target directory to clean
    #[structopt(long = "target-dir", value_name = "DIR", parse(from_os_str))]
    target_dir: Option<PathBuf>,

    /// Increase verbosity
    #[structopt(long = "verbose", short = "v", parse(from_occurrences))]
    verbose: u32,
    /// Do not output anything
    #[structopt(long = "quiet", short = "q")]
    quiet: bool,
    /// Output coloring
    #[structopt(long = "color", value_name = "WHEN")]
    color: Option<String>,
    /// Require Cargo.lock and cache are up to date
    #[structopt(long = "frozen")]
    frozen: bool,
    /// Require Cargo.lock is up to date
    #[structopt(long = "locked")]
    locked: bool,
    /// Do not access the network
    #[structopt(long = "offline")]
    offline: bool,
}

fn main() -> Result<()> {
    let CliOpts::GcTarget(args) = CliOpts::from_args();

    let mut config = Config::default()?;
    config.configure(
        args.verbose,
        args.quiet,
        args.color.as_deref(),
        args.frozen,
        args.locked,
        args.offline,
        &args.target_dir,
        &[],
        &[],
    )?;

    let root_manifest_path = match args.manifest_path {
        Some(p) => p,
        None => find_root_manifest_for_wd(&env::current_dir()?)?,
    };
    let ws = Workspace::new(&root_manifest_path, &config)?;

    let bytes = gc_workspace(&ws)?;
    let bytes_human = bytesize::ByteSize(bytes).to_string_as(true);
    config
        .shell()
        .status("Finished", format_args!("{} freed", bytes_human))?;

    Ok(())
}

fn gc_workspace(ws: &Workspace) -> CargoResult<u64> {
    let target_dir = ws.target_dir().into_path_unlocked();
    let mut collected_bytes = 0u64;

    let mut check = |target: &Option<String>, dir: &Path| -> CargoResult<()> {
        let p = dir.join("debug");
        if p.is_dir() {
            collected_bytes += gc_artifects(ws, target, "dev", &p)?;
        }
        let p = dir.join("release");
        if p.is_dir() {
            collected_bytes += gc_artifects(ws, target, "release", &p)?;
        }
        Ok(())
    };

    check(&None, &target_dir)?;
    for entry in fs::read_dir(target_dir)? {
        let entry = entry?;
        if let Some(file_name) = entry.file_name().to_str() {
            // A rough but easy way to detect target triples like `x86_64-unknown-linux-gnu`.
            if file_name.contains('-') {
                check(&Some(file_name.to_owned()), &entry.path())?;
            }
        }
    }

    Ok(collected_bytes)
}

fn gc_artifects(
    ws: &Workspace,
    target: &Option<String>,
    profile: &str,
    dir: &Path,
) -> CargoResult<u64> {
    match target {
        Some(target) => ws
            .config()
            .shell()
            .status("Collecting", format_args!("{}/{}", target, profile))?,
        None => ws.config().shell().status("Collecting", profile)?,
    }

    let mut reachable = collect::Reachable::default();
    collect::collect_workspace_units(ws.config(), &ws, &target, profile, &mut reachable)?;

    let mut collected_bytes = 0u64;
    let mut remove = |path: &Path| -> Result<()> {
        ws.config()
            .shell()
            .verbose(|s| s.status("Removing", path.display()))?;
        collected_bytes += remove_recursive(&path)?;
        Ok(())
    };

    fn file_stem(p: &OsStr) -> &OsStr {
        if let Some(s) = p.to_str() {
            if let Some(idx) = s.rfind('.') {
                return OsStr::new(&s[..idx]);
            }
        }
        p
    }

    // Collect `.fingerprints`.
    for entry in fs::read_dir(dir.join(".fingerprint"))? {
        let path = entry?.path();
        if !reachable.fingerprints.contains(&path) {
            remove(&path)?;
        }
    }

    // Collect `build`.
    for entry in fs::read_dir(dir.join("build"))? {
        let path = entry?.path();
        if !reachable.builds.contains(&path) {
            remove(&path)?;
        }
    }

    // Collect `deps`.
    for entry in fs::read_dir(dir.join("deps"))? {
        let entry = entry?;
        if !reachable.dep_stems.contains(file_stem(&entry.file_name())) {
            remove(&entry.path())?;
        }
    }

    // Collect binary and test binary outputs.
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_name = entry.file_name();
        // Exclude directory and `.cargo-lock`.
        if entry.file_type()?.is_file()
            && file_name != OsStr::new(".cargo-lock")
            && !reachable.dep_stems.contains(file_stem(&file_name))
        {
            remove(&entry.path())?;
        }
    }

    Ok(collected_bytes)
}

fn remove_recursive(path: &Path) -> Result<u64> {
    let meta = path.symlink_metadata()?;
    let mut ret = meta.len();
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            ret += remove_recursive(&entry?.path())?;
        }
        fs::remove_dir(path)?;
    } else {
        fs::remove_file(path)?;
    }
    Ok(ret)
}
