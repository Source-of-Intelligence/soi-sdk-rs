//! Example plugin that greets.
//!
//! Build with:
//!   cargo build --release --target wasm32-unknown-unknown -p hello

use soi_sdk::{Builder, SandboxContext};
use serde_json::Value;

fn hello(args: Value, ctx: &SandboxContext) -> Result<Value, String> {
    ctx.host.log(1, "greeting from Rust plugin");
    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("world")
        .to_string();
    Ok(Value::String(format!("hello, {}", name)))
}

fn init() {
    Builder::new("hello")
        .desc("a minimal Rust plugin that greets")
        .param("name", "string", true, Value::Null, "name to greet")
        .returns("greeting string")
        .register(hello);
}

#[cfg(target_family = "wasm")]
#[used]
#[link_section = ".init_array"]
static __SOI_INIT: fn() = init;

#[cfg(not(target_family = "wasm"))]
#[allow(dead_code)]
fn __soi_init_stub() {
    init();
}
