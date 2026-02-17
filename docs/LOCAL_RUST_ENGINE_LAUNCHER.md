# Local Rust Engine Launcher

Use `scripts/rust-engine-launcher.sh` to run the CLI against a local Rust engine without creating ad-hoc temp launcher scripts.

## What it does
- Reads the Rust adapter request JSON from stdin (`{ action: "build", cwd, configPath? }`)
- Runs `target/debug/engine-core analyze`
- Emits `dist-go/main.go` scaffold under the requested `cwd`
- Prints a valid Rust adapter JSON response to stdout
- Fails with actionable stderr messages when setup is invalid

## Usage
```bash
cargo build -p engine-core
export TSGODOWN_RUST_ENGINE_BIN="$(pwd)/scripts/rust-engine-launcher.sh"
# Optional override for non-default engine-core path:
export TSGODOWN_ENGINE_CORE_BIN="$(pwd)/target/debug/engine-core"
```

Then run any CLI command that invokes the Rust adapter (for example from `examples/fastify-scaffold-real`):

```bash
pnpm install
pnpm run build:go
```
