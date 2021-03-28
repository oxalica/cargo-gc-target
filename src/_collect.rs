use anyhow::Context as _;
use cargo::{
    core::{
        compiler::{BuildConfig, CompileMode, Context, CrateType, FileFlavor, UnitInterner},
        Workspace,
    },
    ops::{create_bcx, CompileFilter, CompileOptions, Packages},
    CargoResult, Config,
};
use std::collections::HashSet;

#[derive(Default, Debug)]
pub struct Reachable {
    pub fingerprints: HashSet<String>,
    pub builds: HashSet<String>,
    pub deps: HashSet<String>,
    pub uplifts: HashSet<String>,
}

pub fn collect_workspace_units(
    config: &Config,
    ws: &Workspace,
    targets: &[String],
    profile: &str,
    out: &mut Reachable,
) -> CargoResult<()> {
    // https://github.com/rust-lang/cargo/blob/0a4ec2917698ee067b257b580698d7ffb8ccbe2f/src/cargo/util/command_prelude.rs#L361
    let spec = Packages::All;
    let jobs = None;

    let compile_modes = [
        CompileMode::Test,
        CompileMode::Build,
        CompileMode::Check { test: false },
        CompileMode::Check { test: true },
        CompileMode::Bench,
        // CompileMode::Doc { deps: false },
        // CompileMode::Doc { deps: true },
        // CompileMode::Doctest,
        // CompileMode::RunCustomBuild, // Not supported here.
    ];

    for &compile_mode in &compile_modes {
        log::debug!("Compile mode: {:?}", compile_mode);

        let mut build_config = BuildConfig::new(&config, jobs, targets, compile_mode)?;
        build_config.requested_profile = profile.into();

        let compile_opts = CompileOptions {
            build_config,
            features: Vec::new(),
            all_features: true,
            no_default_features: false,
            spec: spec.clone(),
            filter: CompileFilter::new_all_targets(),
            target_rustdoc_args: None,
            target_rustc_args: None,
            local_rustdoc_args: None,
            rustdoc_document_private_items: false,
            honor_rust_version: false,
        };

        collect_units(ws, &compile_opts, out)?;
    }

    Ok(())
}

fn collect_units(
    ws: &Workspace,
    compile_opts: &CompileOptions,
    reachable: &mut Reachable,
) -> CargoResult<()> {
    let interner = UnitInterner::new();
    log::debug!("Creating BuildContext");
    let bcx = create_bcx(ws, compile_opts, &interner).context("Create BuildContext")?;

    log::debug!("Creating Context");
    let mut cx = Context::new(&bcx).context("Create Context")?;
    log::debug!("Generating lto");
    cx.lto = crate::cargo_lto::generate(cx.bcx)?;
    log::debug!("Preparing units");
    cx.prepare_units().context("Prepare units")?;
    let files = cx.files();

    log::debug!("Scanning units");
    for unit in bcx.unit_graph.keys() {
        let meta = files.metadata(unit).map(|m| m.to_string());

        if let CompileMode::Test
        | CompileMode::Build
        | CompileMode::Bench
        | CompileMode::Check { .. } = unit.mode
        {
            let info = bcx.target_data.info(unit.kind);
            let triple = bcx.target_data.short_name(&unit.kind);
            let (file_types, _unsupported) =
                info.rustc_outputs(unit.mode, unit.target.kind(), triple)?;
            for file_type in &file_types {
                let filename = file_type.output_filename(&unit.target, meta.as_deref());
                reachable.deps.insert(filename.clone());

                // https://github.com/rust-lang/cargo/blob/6ca27ffc857c7ac658fda14a83dfb4905d742315/src/cargo/core/compiler/context/compilation_files.rs#L334
                if unit.mode == CompileMode::Build
                    && file_type.flavor != FileFlavor::Rmeta
                    && (unit.target.is_bin()
                        // || unit.target.is_custom_build() // Build scripts are not uplifted.
                        || file_type.crate_type == Some(CrateType::Dylib)
                        || bcx.roots.contains(unit))
                {
                    let uplift_name = file_type.uplift_filename(&unit.target);
                    let stem = &uplift_name[..uplift_name.rfind('.').unwrap_or(uplift_name.len())];
                    reachable.uplifts.insert(format!("{}.d", stem));
                    reachable.uplifts.insert(uplift_name);
                }
            }
        }

        reachable.deps.insert(match &meta {
            Some(meta) => format!("{}-{}.d", unit.target.crate_name(), &meta),
            None => format!("{}.d", unit.target.crate_name()),
        });

        let pkg_name = unit.pkg.package_id().name();
        let pkg_dir = match &meta {
            Some(meta) => format!("{}-{}", pkg_name, meta),
            None => format!("{}-{}", pkg_name, files.target_short_hash(unit)),
        };

        if unit.target.is_custom_build() {
            reachable.builds.insert(pkg_dir.clone());
        }

        reachable.fingerprints.insert(pkg_dir);
    }
    Ok(())
}
