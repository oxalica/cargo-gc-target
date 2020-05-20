use cargo::{
    core::{
        compiler::{BuildConfig, CompileMode, Context},
        Workspace,
    },
    ops::{prepare_compile_context_for, CompileFilter, CompileOptions, Packages},
    CargoResult, Config,
};
use std::{collections::HashSet, ffi::OsString, path::PathBuf};

#[derive(Default, Debug)]
pub struct Reachable {
    pub fingerprints: HashSet<PathBuf>,
    pub builds: HashSet<PathBuf>,
    pub bin_stems: HashSet<OsString>,
    pub dep_stems: HashSet<OsString>,
}

pub fn collect_workspace_units(
    config: &Config,
    ws: &Workspace,
    target: &Option<String>,
    profile: &str,
    out: &mut Reachable,
) -> CargoResult<()> {
    // https://github.com/rust-lang/cargo/blob/0a4ec2917698ee067b257b580698d7ffb8ccbe2f/src/cargo/util/command_prelude.rs#L361
    let spec = Packages::All;
    let jobs = None;

    for &compile_mode in CompileMode::all_modes() {
        if let CompileMode::RunCustomBuild = compile_mode {
            // Not supported here.
            continue;
        }

        let mut build_config = BuildConfig::new(&config, jobs, target, compile_mode)?;
        build_config.requested_profile = profile.into();

        let compile_opts = CompileOptions {
            config: &config,
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
            export_dir: None,
        };

        collect_units(ws, &compile_opts, out)?;
    }

    Ok(())
}

fn collect_units(
    ws: &Workspace,
    compile_opts: &CompileOptions,
    out: &mut Reachable,
) -> CargoResult<()> {
    prepare_compile_context_for(&ws, &compile_opts, |bcx, units, unit_graph| {
        let all_units: Vec<_> = unit_graph.keys().copied().collect();
        let mut cx = Context::new(
            &compile_opts.config,
            bcx,
            unit_graph,
            compile_opts.build_config.requested_kind,
        )?;
        cx.prepare_units(None, units)?;
        let files = cx.files();

        for unit in &all_units {
            out.fingerprints.insert(files.fingerprint_dir(unit));

            out.dep_stems.insert(files.file_stem(unit).into());
            out.dep_stems
                .insert(format!("lib{}", files.file_stem(unit)).into());

            if unit.target.is_custom_build() {
                if unit.mode.is_run_custom_build() {
                    out.builds.insert(files.build_script_run_dir(unit));
                } else {
                    out.builds.insert(files.build_script_dir(unit));
                }
            }

            if unit.target.is_bin() {
                out.bin_stems.insert(
                    files
                        .bin_link_for_target(&unit.target, unit.kind, &bcx)?
                        .file_name()
                        .unwrap()
                        .to_owned(),
                );
            }

            if unit.target.is_lib() {
                out.bin_stems
                    .insert(format!("lib{}", files.file_stem(unit)).into());
            }
        }
        Ok(())
    })
}
