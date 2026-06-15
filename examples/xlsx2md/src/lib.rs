//! Rust port of the xlsx2md Go plugin.
//!
//! Reads an Excel (.xlsx) file from the sandbox, extracts the sheet table
//! as a Markdown document, and writes the result back into the sandbox.

use soi_sdk::{Builder, SandboxContext, SANDBOX_FS};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::io::{Cursor, Read};
use zip::ZipArchive;

#[derive(Deserialize)]
struct Args {
    source: String,
    #[serde(default)]
    output: Option<String>,
}

#[derive(Serialize)]
struct Out {
    #[serde(rename = "output_path")]
    output_path: String,
    success: bool,
    #[serde(rename = "tables_count")]
    tables_count: usize,
}

fn xlsx_to_md(args: Value, ctx: &SandboxContext) -> Result<Value, String> {
    let args: Args = serde_json::from_value(args).map_err(|e| e.to_string())?;
    let bytes = ctx.host.sandbox_read(&args.source)?;
    let md = convert(&bytes).map_err(|e| e.to_string())?;
    let out_path = match &args.output {
        Some(p) if !p.is_empty() => p.clone(),
        _ => {
            let mut p = args.source.clone();
            if p.to_ascii_lowercase().ends_with(".xlsx") {
                p.truncate(p.len() - 5);
            }
            p + ".md"
        }
    };
    ctx.host.sandbox_write(&out_path, md.as_bytes())?;
    let out = Out { output_path: out_path, success: true, tables_count: 1 };
    serde_json::to_value(&out).map_err(|e| e.to_string())
}

fn convert(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut sheet_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let name = {
            let f = archive.by_index(i)?;
            f.name().to_string()
        };
        if name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml") {
            sheet_names.push(name);
        }
    }

    let mut out = String::new();
    let mut tables_count = 0usize;
    for name in &sheet_names {
        let mut f = archive.by_name(name)?;
        let mut xml = String::new();
        f.read_to_string(&mut xml)?;
        if let Some(table) = parse_sheet(&xml) {
            tables_count += 1;
            out.push_str(&format!("## 工作表: {}\n\n", name));
            out.push_str(&table);
            out.push_str("\n\n");
        }
    }

    if tables_count == 0 {
        return Err("no worksheets found in xlsx".into());
    }
    Ok(out)
}

/// A very small parser that only supports the basic xlsx cell format, enough
/// to turn simple sheets into Markdown tables without pulling in an XML DOM.
fn parse_sheet(xml: &str) -> Option<String> {
    // Find every <row> ... </row> block; then inside each row find every
    // <v>...</v> text-node and emit a pipe-separated row.
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut scan = xml;
    while let Some(row_start) = scan.find("<row") {
        let after_open = scan[..row_start].len() + "<row".len();
        let row_tag_end = scan[after_open..].find('>')? + after_open;
        let body_end = scan[row_tag_end..].find("</row>")? + row_tag_end;
        let body = &scan[row_tag_end + 1..body_end];
        let mut cells: Vec<String> = Vec::new();
        let mut s = body;
        while let Some(v_start) = s.find("<v>") {
            let after = v_start + "<v>".len();
            let v_end = s[after..].find("</v>")? + after;
            cells.push(s[after..v_end].to_string());
            s = &s[v_end + "</v>".len()..];
        }
        if !cells.is_empty() {
            rows.push(cells);
        }
        scan = &scan[body_end + "</row>".len()..];
    }

    if rows.is_empty() {
        return None;
    }
    let columns = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut md = String::new();
    for (idx, row) in rows.iter().enumerate() {
        md.push('|');
        for c in 0..columns {
            md.push(' ');
            md.push_str(row.get(c).map(|s| s.as_str()).unwrap_or(""));
            md.push_str(" |");
        }
        md.push('\n');
        if idx == 0 {
            md.push('|');
            for _ in 0..columns {
                md.push_str(" --- |");
            }
            md.push('\n');
        }
    }
    Some(md)
}

fn init() {
    let mut ex_input = Map::new();
    ex_input.insert("source".into(), Value::String("in.xlsx".into()));
    Builder::new("xlsx_to_md")
        .desc("读取 Excel 文件 (.xlsx)，转换为 Markdown 格式并写入输出文件")
        .param("source", "string", true, Value::Null, "沙箱中 Excel 文件的路径")
        .param("output", "string", false, Value::Null, "输出的 .md 文件路径（默认与源文件同名）")
        .returns(r#"object with output_path and success status"#.into())
        .with_sandbox(&[SANDBOX_FS])
        .with_sandbox_subdir("/")
        .with_timeout("30s")
        .trigger_keywords(&["xlsx", "document", "转换表格"])
        .trigger_prefix("/转换表格")
        .trigger_priority(100)
        .example(ex_input, "out.md".into())
        .register(xlsx_to_md);
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
