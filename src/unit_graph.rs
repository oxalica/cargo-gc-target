use serde::{de, Deserialize};

pub const VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
pub struct UnitGraphV1 {
    pub version: u32,
    pub units: Vec<Unit>,
    pub roots: Vec<usize>,
}

#[derive(Debug, Deserialize)]
pub struct Unit {
    pub pkg_id: PackageId,
    pub target: Target,
    pub profile: Profile,
    pub platform: CompileKind,
    pub mode: CompileMode,
    pub features: Vec<String>,
    // #[serde(skip_serializing_if = "std::ops::Not::not")] // hide for unstable build-std
    #[serde(default)]
    pub is_std: bool,
    pub dependencies: Vec<UnitDep>,
}

#[derive(Debug, Deserialize)]
pub struct UnitDep {
    pub index: usize,
    pub extern_crate_name: String,
    // This is only set on nightly since it is unstable.
    // #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub public: Option<bool>,
    // This is only set on nightly since it is unstable.
    // #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub noprelude: Option<bool>,
    // Intentionally not including `unit_for` because it is a low-level // internal detail that is mostly used for building the graph.
}

/// Opaque identifier for a specific version of a package in a specific source.
#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub struct PackageId {
    pub repr: String,
}

/// Information about a binary, a library, an example, etc. that is part of the
/// package.
#[derive(Debug, Deserialize)]
pub struct Target {
    pub kind: TargetKind,
    pub name: String,
    pub src_path: String,
    pub required_features: Option<Vec<String>>,
    pub tested: bool,
    pub benched: bool,
    pub doc: bool,
    pub doctest: bool,
    pub harness: bool,
    pub for_host: bool,
    pub proc_macro: bool,
    pub edition: Edition,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub enum TargetKind {
    Lib(Vec<CrateType>),
    Bin,
    Test,
    Bench,
    ExampleLib(Vec<CrateType>),
    ExampleBin,
    CustomBuild,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
pub enum CrateType {
    Bin,
    Lib,
    Rlib,
    Dylib,
    Cdylib,
    Staticlib,
    ProcMacro,
    Other(String),
}

/// The edition of the compiler (RFC 2052)
#[derive(Clone, Copy, Debug, Hash, PartialOrd, Ord, Eq, PartialEq, Deserialize)]
pub enum Edition {
    /// The 2015 edition
    Edition2015,
    /// The 2018 edition
    Edition2018,
    /// The 2021 edition
    Edition2021,
}

/// Profile settings used to determine which compiler flags to use for a
/// target.
#[derive(PartialEq, Eq, Hash, Debug, Clone, Deserialize)]
pub struct Profile {
    pub name: String,
    pub opt_level: String,
    // #[serde(skip)] // named profiles are unstable
    // pub root: ProfileRoot,
    pub lto: Lto,
    // `None` means use rustc default.
    pub codegen_units: Option<u32>,
    pub debuginfo: Option<u32>,
    pub split_debuginfo: Option<String>,
    pub debug_assertions: bool,
    pub overflow_checks: bool,
    pub rpath: bool,
    pub incremental: bool,
    pub panic: PanicStrategy,
    pub strip: Strip,
}

/// The link-time-optimization setting.
#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord)]
pub enum Lto {
    /// Explicitly no LTO, disables thin-LTO.
    Off,
    /// True = "Fat" LTO
    /// False = rustc default (no args), currently "thin LTO"
    Bool(bool),
    /// Named LTO settings like "thin".
    Named(String),
}

impl<'de> de::Deserialize<'de> for Lto {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match &*s {
            "off" => Self::Off,
            "true" => Self::Bool(true),
            "false" => Self::Bool(false),
            _ => Self::Named(s),
        })
    }
}

/// The `panic` setting.
#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PanicStrategy {
    Unwind,
    Abort,
}

/// The setting for choosing which symbols to strip
#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strip {
    /// Only strip debugging symbols
    DebugInfo,
    /// Don't remove any symbols
    None,
    /// Strip all non-exported symbols from the final binary
    Symbols,
}

/// Indicator for how a unit is being compiled.
///
/// This is used primarily for organizing cross compilations vs host
/// compilations, where cross compilations happen at the request of `--target`
/// and host compilations happen for things like build scripts and procedural
/// macros.
#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord)]
pub enum CompileKind {
    /// Attached to a unit that is compiled for the "host" system or otherwise
    /// is compiled without a `--target` flag. This is used for procedural
    /// macros and build scripts, or if the `--target` flag isn't passed.
    Host,

    /// Attached to a unit to be compiled for a particular target. This is used
    /// for units when the `--target` flag is passed.
    Target(CompileTarget),
}

impl<'de> de::Deserialize<'de> for CompileKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        Ok(match <Option<String>>::deserialize(deserializer)? {
            None => Self::Host,
            Some(name) => Self::Target(CompileTarget { name }),
        })
    }
}

#[derive(PartialEq, Eq, Hash, Debug, Clone, PartialOrd, Ord)]
pub struct CompileTarget {
    pub name: String,
}

/// The general "mode" for what to do.
/// This is used for two purposes. The commands themselves pass this in to
/// `compile_ws` to tell it the general execution strategy. This influences
/// the default targets selected. The other use is in the `Unit` struct
/// to indicate what is being done with a specific target.
#[derive(Clone, Copy, PartialEq, Debug, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompileMode {
    /// A target being built for a test.
    Test,
    /// Building a target with `rustc` (lib or bin).
    Build,
    /// Building a target with `rustc` to emit `rmeta` metadata only. If
    /// `test` is true, then it is also compiled with `--test` to check it like
    /// a test.
    // Check { test: bool }, // `test` is not available in serialized result.
    Check,
    /// Used to indicate benchmarks should be built. This is not used in
    /// `Unit`, because it is essentially the same as `Test` (indicating
    /// `--test` should be passed to rustc) and by using `Test` instead it
    /// allows some de-duping of Units to occur.
    Bench,
    /// A target that will be documented with `rustdoc`.
    /// If `deps` is true, then it will also document all dependencies.
    // Doc { deps: bool }, // `deps` is not available in serialized result.
    Doc,
    /// A target that will be tested with `rustdoc`.
    Doctest,
    /// A marker for Units that represent the execution of a `build.rs` script.
    RunCustomBuild,
}
