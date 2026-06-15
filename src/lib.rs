//! # SOI Rust SDK (soi-sdk-rs)
//!
//! Provides the plugin author-facing API to write SOI plugins in Rust and
//! compile them to WebAssembly (`wasm32-unknown-unknown` / `wasm32-wasip1`).
//!
//! The plugin exports a single `execute` entry point. The host (soi-executor)
//! passes a JSON request like:
//!
//! ```json
//! { "tool": "xlsx_to_md", "args": { "source": "file.xlsx" }, "sandbox_root": "/" }
//! ```
//!
//! and expects a JSON response. The plugin can call back into the host via the
//! well-known WASM imports under the `soi` module — see [`HostApi`].
//!
//! A minimal plugin looks like this:
//!
//! ```ignore
//! use soi_sdk::{soi_plugin, Builder, SandboxContext};
//! use serde::{Deserialize, Serialize};
//! use serde_json::Value;
//!
//! #[derive(Deserialize)]
//! struct Args { source: String }
//!
//! #[derive(Serialize)]
//! struct Out { output_path: String, success: bool }
//!
//! fn xlsx_to_md(args: Value, ctx: &SandboxContext) -> Result<Value, String> {
//!     let args: Args = serde_json::from_value(args).map_err(|e| e.to_string())?;
//!     let data = ctx.host.sandbox_read(&args.source).map_err(|e| e.to_string())?;
//!     // ... parse bytes and produce markdown ...
//!     let md = format!("# converted from {}", args.source);
//!     ctx.host.sandbox_write("out.md", md.as_bytes()).map_err(|e| e.to_string())?;
//!     Ok(serde_json::to_value(Out { output_path: "out.md".into(), success: true }).unwrap())
//! }
//!
//! soi_plugin! {
//!     tools: [
//!         Builder::new("xlsx_to_md")
//!             .desc("读取 Excel 文件，转换为 Markdown")
//!             .param("source", "string", true, "", "沙箱中 Excel 文件的路径")
//!             .with_sandbox(&["sandbox_fs"])
//!             .handler(xlsx_to_md),
//!     ]
//! }
//! ```
//!
//! ## Building for WASM
//!
//! ```text
//! cargo build --release --target wasm32-unknown-unknown
//! # output: target/wasm32-unknown-unknown/release/soi_sdk.wasm
//! ```
//!
//! Or for WASIp1 (preview 1):
//!
//! ```text
//! cargo build --release --target wasm32-wasip1
//! ```

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Sandbox capability constants (mirrors soi-sdk Go API)
// ---------------------------------------------------------------------------

pub const SANDBOX_FS: &str = "sandbox_fs";
pub const HOST_LOG: &str = "host_log";
pub const HOST_NOW: &str = "host_now";
pub const HOST_RANDOM: &str = "host_random";
pub const HOST_HTTP: &str = "host_http";
pub const HOST_ENV: &str = "host_env";
pub const HOST_PROCESS: &str = "host_process";

pub const SDK_VERSION: &str = "2.0.0";
pub const ABI_VERSION: &str = "1.0";

// ---------------------------------------------------------------------------
// Types — mirror vos types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecResult {
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    #[serde(rename = "exit_code")]
    pub exit_code: i32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpResponse {
    #[serde(rename = "status_code")]
    pub status_code: u16,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    pub size: i64,
    #[serde(rename = "is_dir")]
    pub is_dir: bool,
    #[serde(rename = "mod_time")]
    #[serde(default)]
    pub mod_time: i64,
}

// ---------------------------------------------------------------------------
// Host API — mirrors vos.HostFunctions
// ---------------------------------------------------------------------------

/// A trait describing the host capabilities exposed to plugins.
///
/// For WASM plugins, the real implementation uses FFI imports (see
/// [`WasmHostApi`]). Unit tests can use [`MockHost`] or any other
/// implementation.
pub trait HostApi {
    fn log(&self, level: i32, msg: &str);
    fn now(&self) -> i64;
    fn random(&self, buf: &mut [u8]) -> Result<(), String>;
    fn sandbox_read(&self, path: &str) -> Result<Vec<u8>, String>;
    fn sandbox_write(&self, path: &str, data: &[u8]) -> Result<(), String>;
    fn sandbox_list(&self, path: &str) -> Result<Vec<String>, String>;
    fn sandbox_stat(&self, path: &str) -> Result<FileInfo, String>;
    fn sandbox_exec(&self, cmd: &str) -> Result<ExecResult, String>;
    fn sandbox_http(&self, req: &HttpRequest) -> Result<HttpResponse, String>;
}

/// In-WASM FFI implementation backed by the host-provided imports.
pub struct WasmHostApi;

impl WasmHostApi {
    pub fn new() -> Self {
        WasmHostApi
    }
}

impl Default for WasmHostApi {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Raw WASM host imports
// ---------------------------------------------------------------------------
//
// Each function is imported from the `soi` module (see `soi-executor/pkg/soi/abi.go`).
// The ABI packs (ptr, len) pairs as (i64, i64) — where each component is actually a
// 32-bit memory offset/length. Return values of variable-length type are packed into
// a single i64 using `(offset << 32) | length` by the host, and the plugin reads
// the bytes from linear memory.
//
// On the Rust side we use `extern "C"` with the canonical ABI for `wasm32`.

#[cfg(target_family = "wasm")]
extern "C" {
    fn soi_log(level: i32, ptr: u32, len: u32);
    fn soi_now() -> i64;
    fn soi_random(ptr: u32, len: u32) -> i32;

    fn soi_sandbox_read(path_ptr: u32, path_len: u32) -> u64;
    fn soi_sandbox_write(path_ptr: u32, path_len: u32, data_ptr: u32, data_len: u32) -> i32;
    fn soi_sandbox_list(path_ptr: u32, path_len: u32) -> u64;
    fn soi_sandbox_stat(path_ptr: u32, path_len: u32) -> u64;
    fn soi_sandbox_exec(cmd_ptr: u32, cmd_len: u32) -> u64;
    fn soi_sandbox_http(req_ptr: u32, req_len: u32) -> u64;
}

// We provide stubs for non-WASM targets so that `cargo test` still works on the
// host platform (so the plugin crate can still compile, but the real behavior is
// provided by the `soi-executor` runtime).
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_log(_level: i32, _ptr: u32, _len: u32) {}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_now() -> i64 {
    0
}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_random(_ptr: u32, _len: u32) -> i32 {
    1
}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_sandbox_read(_ptr: u32, _len: u32) -> u64 {
    0
}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_sandbox_write(_ptr: u32, _len: u32, _dptr: u32, _dlen: u32) -> i32 {
    1
}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_sandbox_list(_ptr: u32, _len: u32) -> u64 {
    0
}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_sandbox_stat(_ptr: u32, _len: u32) -> u64 {
    0
}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_sandbox_exec(_ptr: u32, _len: u32) -> u64 {
    0
}
#[cfg(not(target_family = "wasm"))]
unsafe fn soi_sandbox_http(_ptr: u32, _len: u32) -> u64 {
    0
}

/// Unpack a `(offset, length)` pair packed into a single u64.
#[inline]
fn unpack_u64(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, (packed & 0xFFFF_FFFF) as u32)
}

/// Read a slice from linear memory at `offset` for `len` bytes — safe for
/// `wasm32` (bounded by the Rust slice of the caller).
#[cfg(target_family = "wasm")]
unsafe fn read_linear(offset: u32, len: u32) -> &'static [u8] {
    if len == 0 {
        return &[];
    }
    core::slice::from_raw_parts(offset as *const u8, len as usize)
}

// On non-WASM, return an empty slice (the imports are stubs).
#[cfg(not(target_family = "wasm"))]
unsafe fn read_linear(_offset: u32, _len: u32) -> &'static [u8] {
    &[]
}

// ---------------------------------------------------------------------------
// WasmHostApi concrete impl
// ---------------------------------------------------------------------------

impl HostApi for WasmHostApi {
    fn log(&self, level: i32, msg: &str) {
        unsafe { soi_log(level, msg.as_ptr() as u32, msg.len() as u32) };
    }

    fn now(&self) -> i64 {
        unsafe { soi_now() }
    }

    fn random(&self, buf: &mut [u8]) -> Result<(), String> {
        if buf.is_empty() {
            return Ok(());
        }
        let rc = unsafe { soi_random(buf.as_mut_ptr() as u32, buf.len() as u32) };
        if rc == 0 {
            Ok(())
        } else {
            Err("host random failed".into())
        }
    }

    fn sandbox_read(&self, path: &str) -> Result<Vec<u8>, String> {
        let packed = unsafe { soi_sandbox_read(path.as_ptr() as u32, path.len() as u32) };
        let (offset, len) = unpack_u64(packed);
        if len == 0 {
            return Err("sandbox_read: empty response".into());
        }
        let bytes = unsafe { read_linear(offset, len) };
        Ok(bytes.to_vec())
    }

    fn sandbox_write(&self, path: &str, data: &[u8]) -> Result<(), String> {
        let rc = unsafe {
            soi_sandbox_write(
                path.as_ptr() as u32,
                path.len() as u32,
                data.as_ptr() as u32,
                data.len() as u32,
            )
        };
        if rc == 0 {
            Ok(())
        } else {
            Err("sandbox_write failed".into())
        }
    }

    fn sandbox_list(&self, path: &str) -> Result<Vec<String>, String> {
        let packed = unsafe { soi_sandbox_list(path.as_ptr() as u32, path.len() as u32) };
        let (offset, len) = unpack_u64(packed);
        let bytes = unsafe { read_linear(offset, len) };
        if bytes.is_empty() {
            return Err("sandbox_list: empty response".into());
        }
        serde_json::from_slice::<Vec<String>>(bytes).map_err(|e| e.to_string())
    }

    fn sandbox_stat(&self, path: &str) -> Result<FileInfo, String> {
        let packed = unsafe { soi_sandbox_stat(path.as_ptr() as u32, path.len() as u32) };
        let (offset, len) = unpack_u64(packed);
        let bytes = unsafe { read_linear(offset, len) };
        if bytes.is_empty() {
            return Err("sandbox_stat: empty response".into());
        }
        serde_json::from_slice::<FileInfo>(bytes).map_err(|e| e.to_string())
    }

    fn sandbox_exec(&self, cmd: &str) -> Result<ExecResult, String> {
        let packed = unsafe { soi_sandbox_exec(cmd.as_ptr() as u32, cmd.len() as u32) };
        let (offset, len) = unpack_u64(packed);
        let bytes = unsafe { read_linear(offset, len) };
        if bytes.is_empty() {
            return Err("sandbox_exec: empty response".into());
        }
        serde_json::from_slice::<ExecResult>(bytes).map_err(|e| e.to_string())
    }

    fn sandbox_http(&self, req: &HttpRequest) -> Result<HttpResponse, String> {
        let payload = serde_json::to_vec(req).map_err(|e| e.to_string())?;
        let packed = unsafe { soi_sandbox_http(payload.as_ptr() as u32, payload.len() as u32) };
        let (offset, len) = unpack_u64(packed);
        let bytes = unsafe { read_linear(offset, len) };
        if bytes.is_empty() {
            return Err("sandbox_http: empty response".into());
        }
        serde_json::from_slice::<HttpResponse>(bytes).map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// SandboxContext — passed to tool handlers
// ---------------------------------------------------------------------------

pub struct SandboxContext {
    pub sandbox_root: String,
    pub host: Box<dyn HostApi>,
}

impl SandboxContext {
    pub fn new(sandbox_root: impl Into<String>, host: impl HostApi + 'static) -> Self {
        Self {
            sandbox_root: sandbox_root.into(),
            host: Box::new(host),
        }
    }
}

// ---------------------------------------------------------------------------
// Tool definitions — mirrors Go SDK's Builder
// ---------------------------------------------------------------------------

/// JSON-friendly parameter definition for a tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParamDef {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub r#enum: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolExample {
    pub input: BTreeMap<String, Value>,
    pub output: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ParamDef>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub returns: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub examples: Vec<ToolExample>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub uses: Vec<String>,
}

pub type ToolHandler = fn(Value, &SandboxContext) -> Result<Value, String>;
pub type SimpleHandler = fn(Value) -> Result<Value, String>;

/// A single registered tool.
pub(crate) struct RegisteredTool {
    pub handler: ToolHandler,
    pub def: ToolDef,
}

// ---------------------------------------------------------------------------
// Global registry
// ---------------------------------------------------------------------------

struct Registry {
    tools: BTreeMap<String, RegisteredTool>,
    plugin_uses: Vec<String>,
    wasm_subdir: String,
    wasm_timeout: String,
    trigger_keywords: Vec<String>,
    trigger_prefix: String,
    trigger_regex: String,
    trigger_events: Vec<String>,
    trigger_priority: i32,
}

impl Registry {
    const fn new() -> Self {
        Registry {
            tools: BTreeMap::new(),
            plugin_uses: Vec::new(),
            wasm_subdir: String::new(),
            wasm_timeout: String::new(),
            trigger_keywords: Vec::new(),
            trigger_prefix: String::new(),
            trigger_regex: String::new(),
            trigger_events: Vec::new(),
            trigger_priority: 0,
        }
    }
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::new());
}

pub fn register_tool(def: ToolDef, handler: ToolHandler) {
    let name = def.name.clone();
    REGISTRY.with(|r| {
        r.borrow_mut().tools.insert(name, RegisteredTool { handler, def });
    });
}

pub fn set_plugin_uses(uses: &[&str]) {
    REGISTRY.with(|r| r.borrow_mut().plugin_uses = uses.iter().map(|s| (*s).to_owned()).collect());
}

pub fn get_plugin_uses() -> Vec<String> {
    REGISTRY.with(|r| r.borrow().plugin_uses.clone())
}

pub fn set_wasm_subdir(subdir: impl Into<String>) {
    REGISTRY.with(|r| r.borrow_mut().wasm_subdir = subdir.into());
}

pub fn get_wasm_subdir() -> String {
    REGISTRY.with(|r| r.borrow().wasm_subdir.clone())
}

pub fn set_wasm_timeout(timeout: impl Into<String>) {
    REGISTRY.with(|r| r.borrow_mut().wasm_timeout = timeout.into());
}

pub fn get_wasm_timeout() -> String {
    REGISTRY.with(|r| r.borrow().wasm_timeout.clone())
}

pub fn set_trigger_keywords(keywords: &[&str]) {
    REGISTRY.with(|r| r.borrow_mut().trigger_keywords = keywords.iter().map(|s| (*s).to_owned()).collect());
}

pub fn get_trigger_keywords() -> Vec<String> {
    REGISTRY.with(|r| r.borrow().trigger_keywords.clone())
}

pub fn set_trigger_prefix(prefix: impl Into<String>) {
    REGISTRY.with(|r| r.borrow_mut().trigger_prefix = prefix.into());
}

pub fn get_trigger_prefix() -> String {
    REGISTRY.with(|r| r.borrow().trigger_prefix.clone())
}

pub fn set_trigger_regex(regex: impl Into<String>) {
    REGISTRY.with(|r| r.borrow_mut().trigger_regex = regex.into());
}

pub fn get_trigger_regex() -> String {
    REGISTRY.with(|r| r.borrow().trigger_regex.clone())
}

pub fn set_trigger_events(events: &[&str]) {
    REGISTRY.with(|r| r.borrow_mut().trigger_events = events.iter().map(|s| (*s).to_owned()).collect());
}

pub fn get_trigger_events() -> Vec<String> {
    REGISTRY.with(|r| r.borrow().trigger_events.clone())
}

pub fn set_trigger_priority(p: i32) {
    REGISTRY.with(|r| r.borrow_mut().trigger_priority = p);
}

pub fn get_trigger_priority() -> i32 {
    REGISTRY.with(|r| r.borrow().trigger_priority)
}

pub fn get_tool_names() -> Vec<String> {
    REGISTRY.with(|r| r.borrow().tools.keys().cloned().collect())
}

pub fn get_tool_defs() -> Vec<ToolDef> {
    REGISTRY.with(|r| r.borrow().tools.values().map(|t| t.def.clone()).collect())
}

/// JSON manifest — matching the Go SDK's Manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(rename = "sdk_version")]
    pub sdk_version: String,
    #[serde(rename = "abi_version")]
    pub abi_version: String,
    pub tools: Vec<ToolDef>,
    #[serde(rename = "build_tag")]
    pub build_tag: String,
}

pub fn build_manifest() -> Manifest {
    Manifest {
        sdk_version: SDK_VERSION.into(),
        abi_version: ABI_VERSION.into(),
        tools: get_tool_defs(),
        build_tag: "rust".into(),
    }
}

// ---------------------------------------------------------------------------
// Builder — idiomatic registration (mirrors Go's `sdk.Builder`)
// ---------------------------------------------------------------------------

pub struct Builder {
    def: ToolDef,
}

impl Builder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            def: ToolDef { name: name.into(), ..Default::default() },
        }
    }

    pub fn desc(mut self, description: impl Into<String>) -> Self {
        self.def.description = description.into();
        self
    }

    pub fn param(
        mut self,
        name: impl Into<String>,
        kind: impl Into<String>,
        required: bool,
        default: impl Into<Option<Value>>,
        description: impl Into<String>,
    ) -> Self {
        self.def.parameters.push(ParamDef {
            name: name.into(),
            kind: kind.into(),
            required,
            default: default.into(),
            description: description.into(),
            r#enum: Vec::new(),
        });
        self
    }

    pub fn returns(mut self, returns: impl Into<String>) -> Self {
        self.def.returns = returns.into();
        self
    }

    pub fn example(mut self, input: BTreeMap<String, Value>, output: impl Into<String>) -> Self {
        self.def.examples.push(ToolExample { input, output: output.into() });
        self
    }

    pub fn with_sandbox(mut self, capabilities: &[&str]) -> Self {
        self.def.uses = capabilities.iter().map(|s| (*s).to_owned()).collect();
        self
    }

    pub fn with_sandbox_fs(self) -> Self {
        self.with_sandbox(&[SANDBOX_FS])
    }

    pub fn with_host_log(self) -> Self {
        self.with_sandbox(&[HOST_LOG])
    }

    pub fn with_host_now(self) -> Self {
        self.with_sandbox(&[HOST_NOW])
    }

    pub fn with_host_random(self) -> Self {
        self.with_sandbox(&[HOST_RANDOM])
    }

    pub fn with_host_http(self) -> Self {
        self.with_sandbox(&[HOST_HTTP])
    }

    pub fn with_host_env(self) -> Self {
        self.with_sandbox(&[HOST_ENV])
    }

    pub fn with_host_process(self) -> Self {
        self.with_sandbox(&[HOST_PROCESS])
    }

    pub fn with_sandbox_subdir(self, subdir: impl Into<String>) -> Self {
        set_wasm_subdir(subdir);
        self
    }

    pub fn with_timeout(self, timeout: impl Into<String>) -> Self {
        set_wasm_timeout(timeout);
        self
    }

    pub fn trigger_keywords(self, keywords: &[&str]) -> Self {
        set_trigger_keywords(keywords);
        self
    }

    pub fn trigger_prefix(self, prefix: impl Into<String>) -> Self {
        set_trigger_prefix(prefix);
        self
    }

    pub fn trigger_regex(self, regex: impl Into<String>) -> Self {
        set_trigger_regex(regex);
        self
    }

    pub fn trigger_events(self, events: &[&str]) -> Self {
        set_trigger_events(events);
        self
    }

    pub fn trigger_priority(self, priority: i32) -> Self {
        set_trigger_priority(priority);
        self
    }

    pub fn register(self, handler: ToolHandler) {
        register_tool(self.def, handler);
    }

    pub fn register_simple(self, simple: SimpleHandler) {
        register_tool(self.def, move |args, _ctx| simple(args));
    }
}

// ---------------------------------------------------------------------------
// Execution dispatcher
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
struct Request {
    tool: String,
    #[serde(default)]
    args: Value,
    #[serde(default, rename = "sandbox_root")]
    sandbox_root: String,
}

/// Dispatch the given JSON request through the tool registry.
///
/// On success, returns the tool's JSON response. On failure, returns a JSON
/// error object: `{ "error": "..." }`.
pub fn execute_request(json_bytes: &[u8]) -> Vec<u8> {
    // Attempt to parse; if parsing fails, return an error.
    let req: Request = match serde_json::from_slice(json_bytes) {
        Ok(r) => r,
        Err(e) => return error_json(&format!("parse input: {e}")),
    };

    let ctx = SandboxContext::new(req.sandbox_root, WasmHostApi);
    let tools = REGISTRY.with(|r| r.borrow().tools.keys().cloned().collect::<Vec<_>>());
    let handler = REGISTRY.with(|r| {
        r.borrow().tools.get(&req.tool).map(|t| (t.handler, t.def.clone()))
    });

    if let Some((handler, _def)) = handler {
        match handler(req.args, &ctx) {
            Ok(v) => match serde_json::to_vec(&v) {
                Ok(bytes) => bytes,
                Err(e) => error_json(&format!("marshal result: {e}")),
            },
            Err(e) => error_json(&e),
        }
    } else {
        error_json(&format!("unknown tool: {} (have: {:?})", req.tool, tools))
    }
}

fn error_json(msg: &str) -> Vec<u8> {
    let mut m = Map::new();
    m.insert("error".into(), Value::String(msg.into()));
    serde_json::to_vec(&Value::Object(m)).unwrap_or_else(|_| br#"{"error":"encode failed"}"#.to_vec())
}

// ---------------------------------------------------------------------------
// WASM export entry point
// ---------------------------------------------------------------------------
//
// The host calls `execute(ptr, length)` with a JSON payload in the plugin's
// linear memory. The plugin should return its response by writing the bytes
// into linear memory and returning a packed `(offset << 32) | length`.

/// Buffer used for the plugin-to-host response. The host reads from this
/// buffer using the packed pointer returned from `execute`.
static mut OUT_BUF: Vec<u8> = Vec::new();

/// Rust-side entry point. Wraps the byte-level dispatcher and returns a packed
/// pointer+length value.
#[cfg(target_family = "wasm")]
#[no_mangle]
pub extern "C" fn execute(ptr: u32, len: u32) -> u64 {
    let input = unsafe { read_linear(ptr, len) };
    let result = execute_request(input);

    unsafe {
        OUT_BUF = result;
        let offset = OUT_BUF.as_ptr() as u32;
        let length = OUT_BUF.len() as u32;
        ((offset as u64) << 32) | (length as u64)
    }
}

// Provide a stub for non-WASM targets so that host-platform tests compile.
#[cfg(not(target_family = "wasm"))]
#[allow(dead_code)]
pub fn execute(ptr: u32, len: u32) -> u64 {
    // On non-WASM, the imports are stubs; just return an empty response.
    let _ = (ptr, len);
    0
}

// ---------------------------------------------------------------------------
// skill.yaml generation helper (convenience for build-time tools)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
pub struct SkillConfig {
    pub name: String,
    pub version: String,
    pub description: String,
    pub runtime_type: String, // wasm / go / soi
    pub entry: String,        // e.g. "wasm/plugin.wasm"
    pub author: String,
    pub tags: Vec<String>,
    pub requires: Vec<(String, String)>, // name, version
}

/// Produce a `skill.yaml` string from the global registry + the provided
/// high-level config. Mirrors `soi-sdk::GenerateSkillYAML`.
pub fn generate_skill_yaml(cfg: &SkillConfig) -> String {
    let tools = get_tool_defs();
    let uses = get_plugin_uses();
    let keywords = get_trigger_keywords();
    let prefix = get_trigger_prefix();
    let regex = get_trigger_regex();
    let events = get_trigger_events();
    let priority = get_trigger_priority();
    let subdir = get_wasm_subdir();
    let timeout = get_wasm_timeout();

    let mut s = String::new();
    s.push_str("apiVersion: v1\n");
    s.push_str("kind: Skill\n");
    s.push_str("metadata:\n");
    s.push_str(&format!("  name: {}\n", cfg.name));
    s.push_str(&format!("  version: \"{}\"\n", cfg.version));
    if !cfg.author.is_empty() {
        s.push_str(&format!("  author: {}\n", cfg.author));
    }
    s.push_str(&format!("  description: \"{}\"\n", cfg.description));
    if !cfg.tags.is_empty() {
        s.push_str("  tags: [");
        for (i, t) in cfg.tags.iter().enumerate() {
            if i > 0 { s.push(','); }
            s.push('"'); s.push_str(t); s.push('"');
        }
        s.push_str("]\n");
    }

    s.push_str("spec:\n");
    s.push_str("  runtime:\n");
    s.push_str(&format!("    type: {}\n", cfg.runtime_type));
    s.push_str(&format!("    entry: {}\n", cfg.entry));
    if !uses.is_empty() {
        s.push_str("    uses:\n");
        for u in &uses { s.push_str(&format!("      - {}\n", u)); }
    }
    if !subdir.is_empty() || !timeout.is_empty() {
        s.push_str("    wasm:\n");
        if !subdir.is_empty() {
            s.push_str("      sandbox:\n");
            s.push_str(&format!("        subdir: \"{}\"\n", subdir));
        }
        if !timeout.is_empty() {
            s.push_str(&format!("      timeout: \"{}\"\n", timeout));
        }
    }

    if !cfg.requires.is_empty() {
        s.push_str("  requires:\n");
        for (name, version) in &cfg.requires {
            s.push_str(&format!("    - name: {}\n", name));
            s.push_str(&format!("      version: \"{}\"\n", version));
        }
    }

    s.push_str("  provides:\n");
    if !keywords.is_empty() || !prefix.is_empty() || !regex.is_empty() || !events.is_empty() || priority != 0 {
        s.push_str("    triggers:\n");
        if !keywords.is_empty() {
            s.push_str("      keywords: [");
            for (i, k) in keywords.iter().enumerate() {
                if i > 0 { s.push(','); }
                s.push('"'); s.push_str(k); s.push('"');
            }
            s.push_str("]\n");
        }
        if !prefix.is_empty() {
            s.push_str(&format!("      prefix: \"{}\"\n", prefix));
        }
        if !regex.is_empty() {
            s.push_str(&format!("      regex: \"{}\"\n", regex));
        }
        if !events.is_empty() {
            s.push_str("      events: [");
            for (i, e) in events.iter().enumerate() {
                if i > 0 { s.push(','); }
                s.push('"'); s.push_str(e); s.push('"');
            }
            s.push_str("]\n");
        }
        if priority != 0 {
            s.push_str(&format!("      priority: {}\n", priority));
        }
    }

    if !tools.is_empty() {
        s.push_str("    tools:\n");
        for t in &tools {
            s.push_str(&format!("    - name: {}\n", t.name));
            s.push_str(&format!("      description: \"{}\"\n", t.description));
            if !t.uses.is_empty() {
                s.push_str("      uses:\n");
                for u in &t.uses { s.push_str(&format!("      - {}\n", u)); }
            }
            if !t.parameters.is_empty() {
                s.push_str("      parameters:\n");
                for p in &t.parameters {
                    s.push_str(&format!("      - name: {}\n", p.name));
                    s.push_str(&format!("        type: {}\n", p.kind));
                    if p.required { s.push_str("        required: true\n"); }
                    if let Some(def) = &p.default {
                        s.push_str(&format!("        default: {}\n", def));
                    }
                    if !p.description.is_empty() {
                        s.push_str(&format!("        description: \"{}\"\n", p.description));
                    }
                    if !p.r#enum.is_empty() {
                        s.push_str("        enum: [");
                        for (i, e) in p.r#enum.iter().enumerate() {
                            if i > 0 { s.push(','); }
                            s.push('"'); s.push_str(e); s.push('"');
                        }
                        s.push_str("]\n");
                    }
                }
            }
            if !t.returns.is_empty() {
                s.push_str(&format!("      returns: \"{}\"\n", t.returns));
            }
        }
    }

    s
}

// ---------------------------------------------------------------------------
// Convenience macro: so plugins can declare themselves declaratively
// ---------------------------------------------------------------------------

/// Declaratively define a plugin. See crate-level docs for usage.
#[macro_export]
macro_rules! soi_plugin {
    ( tools: [$($builder:expr),* $(,)?] $(, uses: [$($uses:expr),* $(,)?] )? $(,)? ) => {
        #[allow(unused_imports)]
        use $crate::{Builder, SandboxContext};

        fn __soi_register_tools() {
            $( { let _ = $builder; } )*
            $( $crate::set_plugin_uses(&[$($uses),*]); )?
        }

        // Run registration once at plugin load.
        #[cfg(target_family = "wasm")]
        #[used]
        #[link_section = ".init_array"]
        static __SOI_INIT: fn() = __soi_register_tools;

        #[cfg(not(target_family = "wasm"))]
        #[allow(dead_code)]
        fn __soi_init_marker() { __soi_register_tools(); }
    };
}

// ---------------------------------------------------------------------------
// Tests (host-side)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_round_trip() {
        Builder::new("hello")
            .desc("simplest tool")
            .param("name", "string", true, Value::Null, "name to greet")
            .returns("greeting string")
            .register(|args, _ctx| {
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("world");
                Ok(Value::String(format!("hello, {name}")))
            });
        let manifest = build_manifest();
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "hello");
    }

    #[test]
    fn execute_unknown_tool_returns_error() {
        let req = br#"{"tool":"missing","args":{}}"#;
        let bytes = execute_request(req);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.get("error").is_some());
    }
}
