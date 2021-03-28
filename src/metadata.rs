pub struct Metadata(pub u64);

fn metadata_of(
    unit: &Unit,
    cx: &Context<'_, '_>,
    metas: &mut HashMap<Unit, Option<Metadata>>,
) -> Option<Metadata> {
    if !metas.contains_key(unit) {
        let meta = compute_metadata(unit, cx, metas);
        metas.insert(unit.clone(), meta);
        for dep in cx.unit_deps(unit) {
            metadata_of(&dep.unit, cx, metas);
        }
    }
    metas[unit]
}

fn compute_metadata(
    unit: &Unit,
    cx: &Context<'_, '_>,
    metas: &mut HashMap<Unit, Option<Metadata>>,
) -> Option<Metadata> {
    let bcx = &cx.bcx;
    if !should_use_metadata(bcx, unit) {
        return None;
    }
    let mut hasher = StableHasher::new();

    METADATA_VERSION.hash(&mut hasher);

    // Unique metadata per (name, source, version) triple. This'll allow us
    // to pull crates from anywhere without worrying about conflicts.
    unit.pkg
        .package_id()
        .stable_hash(bcx.ws.root())
        .hash(&mut hasher);

    // Also mix in enabled features to our metadata. This'll ensure that
    // when changing feature sets each lib is separately cached.
    unit.features.hash(&mut hasher);

    // Mix in the target-metadata of all the dependencies of this target.
    let mut deps_metadata = cx
        .unit_deps(unit)
        .iter()
        .map(|dep| metadata_of(&dep.unit, cx, metas))
        .collect::<Vec<_>>();
    deps_metadata.sort();
    deps_metadata.hash(&mut hasher);

    // Throw in the profile we're compiling with. This helps caching
    // `panic=abort` and `panic=unwind` artifacts, additionally with various
    // settings like debuginfo and whatnot.
    unit.profile.hash(&mut hasher);
    unit.mode.hash(&mut hasher);
    cx.lto[unit].hash(&mut hasher);

    // Artifacts compiled for the host should have a different metadata
    // piece than those compiled for the target, so make sure we throw in
    // the unit's `kind` as well
    unit.kind.hash(&mut hasher);

    // Finally throw in the target name/kind. This ensures that concurrent
    // compiles of targets in the same crate don't collide.
    unit.target.name().hash(&mut hasher);
    unit.target.kind().hash(&mut hasher);

    hash_rustc_version(bcx, &mut hasher);

    if cx.bcx.ws.is_member(&unit.pkg) {
        // This is primarily here for clippy. This ensures that the clippy
        // artifacts are separate from the `check` ones.
        if let Some(path) = &cx.bcx.rustc().workspace_wrapper {
            path.hash(&mut hasher);
        }
    }

    // Seed the contents of `__CARGO_DEFAULT_LIB_METADATA` to the hasher if present.
    // This should be the release channel, to get a different hash for each channel.
    if let Ok(ref channel) = env::var("__CARGO_DEFAULT_LIB_METADATA") {
        channel.hash(&mut hasher);
    }

    // std units need to be kept separate from user dependencies. std crates
    // are differentiated in the Unit with `is_std` (for things like
    // `-Zforce-unstable-if-unmarked`), so they are always built separately.
    // This isn't strictly necessary for build dependencies which probably
    // don't need unstable support. A future experiment might be to set
    // `is_std` to false for build dependencies so that they can be shared
    // with user dependencies.
    unit.is_std.hash(&mut hasher);

    Some(Metadata(hasher.finish()))
}

fn hash_rustc_version(bcx: &BuildContext<'_, '_>, hasher: &mut StableHasher) {
    let vers = &bcx.rustc().version;
    if vers.pre.is_empty() || bcx.config.cli_unstable().separate_nightlies {
        // For stable, keep the artifacts separate. This helps if someone is
        // testing multiple versions, to avoid recompiles.
        bcx.rustc().verbose_version.hash(hasher);
        return;
    }
    // On "nightly"/"beta"/"dev"/etc, keep each "channel" separate. Don't hash
    // the date/git information, so that whenever someone updates "nightly",
    // they won't have a bunch of stale artifacts in the target directory.
    //
    // This assumes that the first segment is the important bit ("nightly",
    // "beta", "dev", etc.). Skip other parts like the `.3` in `-beta.3`.
    vers.pre[0].hash(hasher);
    // Keep "host" since some people switch hosts to implicitly change
    // targets, (like gnu vs musl or gnu vs msvc). In the future, we may want
    // to consider hashing `unit.kind.short_name()` instead.
    bcx.rustc().host.hash(hasher);
    // None of the other lines are important. Currently they are:
    // binary: rustc  <-- or "rustdoc"
    // commit-hash: 38114ff16e7856f98b2b4be7ab4cd29b38bed59a
    // commit-date: 2020-03-21
    // host: x86_64-apple-darwin
    // release: 1.44.0-nightly
    // LLVM version: 9.0
    //
    // The backend version ("LLVM version") might become more relevant in
    // the future when cranelift sees more use, and people want to switch
    // between different backends without recompiling.
}

/// Returns whether or not this unit should use a metadata hash.
fn should_use_metadata(bcx: &BuildContext<'_, '_>, unit: &Unit) -> bool {
    if unit.mode.is_doc_test() {
        // Doc tests do not have metadata.
        return false;
    }
    if unit.mode.is_any_test() || unit.mode.is_check() {
        // These always use metadata.
        return true;
    }
    // No metadata in these cases:
    //
    // - dylibs:
    //   - macOS encodes the dylib name in the executable, so it can't be renamed.
    //   - TODO: Are there other good reasons? If not, maybe this should be macos specific?
    // - Windows MSVC executables: The path to the PDB is embedded in the
    //   executable, and we don't want the PDB path to include the hash in it.
    // - wasm32 executables: When using emscripten, the path to the .wasm file
    //   is embedded in the .js file, so we don't want the hash in there.
    //   TODO: Is this necessary for wasm32-unknown-unknown?
    // - apple executables: The executable name is used in the dSYM directory
    //   (such as `target/debug/foo.dSYM/Contents/Resources/DWARF/foo-64db4e4bf99c12dd`).
    //   Unfortunately this causes problems with our current backtrace
    //   implementation which looks for a file matching the exe name exactly.
    //   See https://github.com/rust-lang/rust/issues/72550#issuecomment-638501691
    //   for more details.
    //
    // This is only done for local packages, as we don't expect to export
    // dependencies.
    //
    // The __CARGO_DEFAULT_LIB_METADATA env var is used to override this to
    // force metadata in the hash. This is only used for building libstd. For
    // example, if libstd is placed in a common location, we don't want a file
    // named /usr/lib/libstd.so which could conflict with other rustc
    // installs. TODO: Is this still a realistic concern?
    // See https://github.com/rust-lang/cargo/issues/3005
    let short_name = bcx.target_data.short_name(&unit.kind);
    if (unit.target.is_dylib()
        || unit.target.is_cdylib()
        || (unit.target.is_executable() && short_name.starts_with("wasm32-"))
        || (unit.target.is_executable() && short_name.contains("msvc"))
        || (unit.target.is_executable() && short_name.contains("-apple-")))
        && unit.pkg.package_id().source_id().is_path()
        && env::var("__CARGO_DEFAULT_LIB_METADATA").is_err()
    {
        return false;
    }
    true
}
