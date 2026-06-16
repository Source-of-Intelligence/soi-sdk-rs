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

---

## 通过 soi-create / soi-package 使用（推荐）

本 crate 与 `soi-sdk` 目录中的脚手架/打包工具完全集成：

```bash
# 进入 Go SDK 目录
cd e:\code\soi\soi-sdk

# 使用脚手架一键生成 Rust 插件项目
go run ./cmd/soi-create scaffold --name my-plugin --type wasm --compiler rust

# 直接打包（会自动调用 cargo build --release --target wasm32-wasip1）
go run ./cmd/soi-package --dir ../soi-plugin/my-plugin --compiler rust --skip-sync
# 输出: dist/my-plugin-1.0.0.zip  （内含 wasm/plugin.wasm + skill.yaml + README）
```

生成的 `Cargo.toml` 默认从 GitHub 拉取 `soi-sdk`（如本地同时存在
`soi-sdk-rs/`，`soi-package` 会自动将 git 依赖替换为 path 依赖以启用
本地调试）。

### 完整命令对照

| 操作 | 命令 |
|---|---|
| 生成 Rust 插件脚手架 | `soi-create scaffold --name foo --compiler rust` |
| 运行单元测试 | `cargo test` |
| 编译 WASM | `cargo build --release --target wasm32-wasip1` |
| 打包分发 | `soi-package --dir ./foo --compiler rust` |

### Rust 插件结构

```
my-plugin/
├── src/
│   └── lib.rs         # 工具注册与 handler 函数
├── Cargo.toml         # [dependencies] soi-sdk = { git = "..." }
├── skill.yaml         # 插件元数据（名称 / 版本 / runtime 等）
└── README.md
```

`src/lib.rs` 中使用 Builder / `soi_plugin!` 宏注册工具：

```rust
use soi_sdk::{soi_plugin, Builder, SandboxContext};
use serde_json::Value;

fn hello(args: Value, _ctx: &SandboxContext) -> Result<Value, String> {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("world");
    Ok(Value::String(format!("Hello, {}!", name)))
}

soi_plugin! {
    tools: [
        Builder::new("hello")
            .desc("Say hello")
            .param("name", "string", true, Value::Null, "Your name")
            .returns("greeting string")
            .register(hello),
    ]
}
```

`soi_plugin!` 宏会把 `Builder` 的调用放到 `.init_array` 段中，使插件
被宿主加载时自动注册工具；`execute(ptr, len)` 导出符号由 crate 自动
提供，不需要手写。

## Running on soi-executor

Compile your plugin, then hand the `.wasm` file to `soi-executor` through
`SOIABI` — the runtime automatically wires the `soi` host functions
(`soi_log`, `soi_sandbox_read`, etc.) into the module.
