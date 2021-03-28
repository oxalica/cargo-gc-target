# cargo-gc-target

A cargo sub-command to delete unused files in your cargo `target` directory.

Warning 1: This project is still under development. **Use it with care!
It may unexpectly delete reachable files (cause a rebuild) or your other files.**
If that happens, please create an issue here.

Warning 2: Simply garbage collecting may not work well with global shared `target`
directory, since it just collect current workspace and will delete artifects from
other workspaces.

## Installation

```shell
cargo install --git https://github.com/oxalica/cargo-gc-target.git --tag v0.1.0
```

## Usage

In your project/workspace directory, simple run: (It's `gc` instead of `gc-target`)

```shell
cargo gc
```

It can also follow custom `target-dir` specified in `.cargo/config` or
environment variable `CARGO_TARGET_DIR`.

When resolved target directory is outside the workspace, an error will be emitted
since user may accidentally try to clean shared target directory, which is not
supported. If you really know what you are doing, pass `-f` to force GC anyway.

## Details

Currently, it cleans:
- Artifects of dependencies: usually under `target/<profile>/deps`
- Build scripts and their outputs: usually under `target/<profile>/build`
- Output artifects: usually to be executables and libraries under `target/<profile>`

It does NOT clean (Not implemented yet):
- Objects produced by incremental compilation: usually under `target/<profile>/incremental`
- Examples artifects: usually under `target/<profile>/examples`
- Documentations: usually under `target/doc`

## License

MIT Licensed.
