# Notes on integration with soi-sdk / soi-executor

## Go SDK compatibility

`soi-sdk-rs` targets the same on-wire ABI as the Go SDK's TinyGo runtime:

| Side      | Symbol name                              | Direction         |
|-----------|------------------------------------------|-------------------|
| host → plugin | `execute(ptr: u32, len: u32) -> u64`     | plugin export     |
| plugin → host | `soi_log(level, ptr, len)`               | host import       |
| plugin → host | `soi_now() -> i64`                       | host import       |
| plugin → host | `soi_random(ptr, len)`                   | host import       |
| plugin → host | `soi_sandbox_read(ptr, len) -> u64`      | host import       |
| plugin → host | `soi_sandbox_write(pp, pl, dp, dl) -> i32` | host import     |
| plugin → host | `soi_sandbox_list(ptr, len) -> u64`      | host import       |
| plugin → host | `soi_sandbox_stat(ptr, len) -> u64`      | host import       |
| plugin → host | `soi_sandbox_exec(ptr, len) -> u64`      | host import       |
| plugin → host | `soi_sandbox_http(ptr, len) -> u64`      | host import       |

Variable-length host responses are encoded as `(offset << 32) | length` —
identical to the `vos.Pack` helper in `soi-vos`.

## Manifest / skill.yaml parity

- `build_manifest()` produces the same JSON shape as `go sdk.Manifest`
- `generate_skill_yaml(&SkillConfig { .. })` produces the same YAML shape as
  the Go SDK's `GenerateSkillYAML(..)`. This lets you use a tiny build-script
  to stamp out a `skill.yaml` that the Go side can consume directly.

## Building & testing

```
cargo test --workspace
cargo build --release --target wasm32-unknown-unknown -p hello -p xlsx2md
wasm-opt -Oz target/wasm32-unknown-unknown/release/hello_plugin.wasm \
         -o target/wasm32-unknown-unknown/release/hello_plugin.opt.wasm
```
