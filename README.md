# soi-sdk-rs — Rust SDK for SOI WASM plugins

An idiomatic Rust port of the Go `soi-sdk` plugin SDK. It lets you write SOI
plugins in Rust, compile them to WebAssembly with `cargo`, and run them with
`soi-executor` (which uses the same `soi` host-module ABI as the Go SDK).

## Feature parity with soi-sdk (Go)

- ✅ Tool registry + manifest (`ToolDef`, `ParamDef`, `ToolExample`)
- ✅ Chained `Builder` API — `new/desc/param/returns/example/with_sandbox/...`
- ✅ Sandbox capability constants (`SANDBOX_FS`, `HOST_LOG`, `HOST_HTTP`, …)
- ✅ Plugin-level uses / WASM sandbox subdir / timeout
- ✅ Trigger metadata (keywords / prefix / regex / events / priority)
- ✅ `HostFunctions` trait impl via WASM `soi_*` host imports
  - `log`, `now`, `random`, `sandbox_read`, `sandbox_write`
  - `sandbox_list`, `sandbox_stat`, `sandbox_exec`, `sandbox_http`
- ✅ `execute(ptr, len)` export — mirrors the Go SDK TinyGo entry point
- ✅ `generate_skill_yaml()` — same output shape as Go's `GenerateSkillYAML`
- ✅ Host platform (`cargo test`) — host-function imports degrade gracefully

## Build a plugin

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown -p hello
# output: target/wasm32-unknown-unknown/release/hello_plugin.wasm
```

Or for WASIp1 preview 1 (can be executed by any WASI host):

```bash
rustup target add wasm32-wasip1
cargo build --release --target wasm32-wasip1 -p hello
```

## Writing your own plugin

```rust
use soi_sdk::{Builder, SandboxContext};
use serde_json::Value;

fn hello(args: Value, ctx: &SandboxContext) -> Result<Value, String> {
    ctx.host.log(1, "hello from Rust");
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("world");
    Ok(Value::String(format!("hello, {}", name)))
}

// Register the tool (runs automatically when the WASM module is loaded).
fn init() {
    Builder::new("hello")
        .desc("a minimal plugin that greets")
        .param("name", "string", true, Value::Null, "name to greet")
        .returns("greeting string")
        .register(hello);
}

#[cfg(target_family = "wasm")]
#[used]
#[link_section = ".init_array"]
static __SOI_INIT: fn() = init;
```

The exported `execute(ptr, len)` symbol is provided by the `soi-sdk` crate and
handles the full request dispatch.

## Running on soi-executor

Compile your plugin, then hand the `.wasm` file to `soi-executor` through
`SOIABI` — the runtime automatically wires the `soi` host functions
(`soi_log`, `soi_sandbox_read`, etc.) into the module.
