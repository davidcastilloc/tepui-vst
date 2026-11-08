use std::env;
use std::fs;
use std::path::Path;

/// Derives a deterministic 16-byte VST3 Class ID (UUID v5-style FNV-1a 128-bit)
/// from a Reverse-DNS `CLAP_ID` so template users get a collision-free VST3 identity.
pub fn derive_vst3_class_id_from_clap_id(clap_id: &str) -> [u8; 16] {
    let mut h1: u64 = 0xcbf29ce484222325;
    let mut h2: u64 = 0x84222325cbf29ce4;
    for (idx, b) in clap_id.bytes().enumerate() {
        h1 ^= u64::from(b);
        h1 = h1.wrapping_mul(0x100000001b3);
        h2 ^= u64::from(b.wrapping_add(idx as u8));
        h2 = h2.wrapping_mul(0x100000001b3);
    }
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&h1.to_be_bytes());
    out[8..].copy_from_slice(&h2.to_be_bytes());
    out[6] = (out[6] & 0x0f) | 0x50;
    out[8] = (out[8] & 0x3f) | 0x80;
    out
}

fn format_vst3_bytes(vst3_id: &[u8; 16]) -> String {
    vst3_id
        .iter()
        .map(|b| format!("0x{:02x}", b))
        .collect::<Vec<_>>()
        .join(", ")
}

fn parse_scaffold_args(args: &[String]) -> (String, String, String) {
    let mut plugin_name = String::from("MyAudioPlugin");
    let mut vendor_name = String::from("OpenAudioStack");
    let mut clap_id = String::from("org.openaudiostack.myaudioplugin");

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" if i + 1 < args.len() => {
                plugin_name = args[i + 1].clone();
                i += 2;
            }
            "--vendor" if i + 1 < args.len() => {
                vendor_name = args[i + 1].clone();
                i += 2;
            }
            "--id" if i + 1 < args.len() => {
                clap_id = args[i + 1].clone();
                i += 2;
            }
            _ => {
                i += 1;
            }
        }
    }
    (plugin_name, vendor_name, clap_id)
}

fn run_init_template(args: &[String]) -> nih_plug_xtask::Result<()> {
    let (plugin_name, vendor_name, clap_id) = parse_scaffold_args(args);
    let vst3_id = derive_vst3_class_id_from_clap_id(&clap_id);
    let bytes_fmt = format_vst3_bytes(&vst3_id);

    println!("==================================================================");
    println!(" OPEN AUDIO PLUGIN STACK — VST3 / CLAP IDENTITY GENERATOR");
    println!("==================================================================");
    println!("  Plugin Name   : {}", plugin_name);
    println!("  Vendor        : {}", vendor_name);
    println!("  CLAP_ID       : {}", clap_id);
    println!("  VST3_CLASS_ID : [{}]", bytes_fmt);
    println!("------------------------------------------------------------------");
    println!("  const NAME: &'static str = \"{}\";", plugin_name);
    println!("  const VENDOR: &'static str = \"{}\";", vendor_name);
    println!("  const CLAP_ID: &'static str = \"{}\";", clap_id);
    println!("  const VST3_CLASS_ID: [u8; 16] = [{}];", bytes_fmt);
    println!("==================================================================");
    Ok(())
}

fn to_snake_slug(input: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 && !out.ends_with('_') {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn run_new_plugin(args: &[String]) -> nih_plug_xtask::Result<()> {
    let (plugin_name, vendor_name, clap_id) = parse_scaffold_args(args);
    let slug = to_snake_slug(&plugin_name);
    let crate_dir = Path::new("crates").join(&slug);
    if crate_dir.exists() {
        println!("[XTASK] El directorio {:?} ya existe.", crate_dir);
        return Ok(());
    }

    fs::create_dir_all(crate_dir.join("src").join("bin"))?;
    let vst3_id = derive_vst3_class_id_from_clap_id(&clap_id);
    let bytes_fmt = format_vst3_bytes(&vst3_id);

    // Copy studio_bus_comp as the starter template and customize identifiers
    let ref_lib = fs::read_to_string("crates/studio_bus_comp/src/lib.rs")?;
    let customized_lib = ref_lib
        .replace(
            "const NAME: &'static str = \"Studio VCA Bus Compressor\";",
            &format!("const NAME: &'static str = \"{}\";", plugin_name),
        )
        .replace(
            "const VENDOR: &'static str = \"Open Audio Plugin Stack\";",
            &format!("const VENDOR: &'static str = \"{}\";", vendor_name),
        )
        .replace(
            "const CLAP_ID: &'static str = \"org.openaudiostack.studio-bus-comp\";",
            &format!("const CLAP_ID: &'static str = \"{}\";", clap_id),
        )
        .replace(
            "*b\"StBsCmpOpenAudv1\"",
            &format!("[{}]", bytes_fmt),
        );

    let cargo_toml = format!(
        r#"[package]
name = "{slug}"
version = "0.1.0"
edition = "2021"
license = "MIT"
description = "{plugin_name} — VST3/CLAP/Standalone plugin built on base_plugin_core + dsp_core"
default-run = "{slug}_app"

[lib]
crate-type = ["cdylib", "lib"]

[[bin]]
name = "{slug}_app"
path = "src/bin/{slug}_app.rs"

[dependencies]
dsp_core = {{ path = "../dsp_core" }}
base_plugin_core = {{ path = "../base_plugin_core" }}
base_plugin_webview = {{ path = "../base_plugin_webview" }}
base_plugin_standalone = {{ path = "../base_plugin_standalone" }}
nih_plug = {{ git = "https://github.com/robbert-vdh/nih-plug.git", features = ["assert_process_allocs", "standalone"] }}
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
"#
    );

    let bin_main = format!(
        r#"#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use {slug}::{{set_standalone_mode, StudioBusCompPlugin}};

fn main() {{
    set_standalone_mode(true);
    let mut args: Vec<String> = std::env::args().collect();
    if cfg!(windows) && !args.iter().any(|a| a == "-b" || a == "--backend") {{
        args.push("-b".to_string());
        args.push("wasapi".to_string());
    }}
    if !args.iter().any(|a| a == "-l" || a == "--audio-layout") {{
        args.push("-l".to_string());
        args.push("2".to_string());
    }}
    if cfg!(windows) && !args.iter().any(|a| a == "-p" || a == "--period-size") {{
        args.push("-p".to_string());
        args.push("1056".to_string());
    }}
    nih_plug::wrapper::standalone::nih_export_standalone_with_args::<StudioBusCompPlugin, _>(args);
}}
"#
    );

    fs::write(crate_dir.join("Cargo.toml"), cargo_toml)?;
    fs::write(crate_dir.join("src").join("lib.rs"), customized_lib)?;
    fs::write(
        crate_dir
            .join("src")
            .join("bin")
            .join(format!("{}_app.rs", slug)),
        bin_main,
    )?;

    // Register in root Cargo.toml
    let root_toml_path = Path::new("Cargo.toml");
    if root_toml_path.exists() {
        let root_toml = fs::read_to_string(root_toml_path)?;
        let member_entry = format!("    \"crates/{}\",", slug);
        if !root_toml.contains(&member_entry) {
            let updated = root_toml.replace(
                "    \"crates/xtask\",",
                &format!("    \"crates/{}\",\n    \"crates/xtask\",", slug),
            );
            fs::write(root_toml_path, updated)?;
        }
    }

    println!("==================================================================");
    println!(" NUEVO PLUGIN CREADO EXITOSAMENTE: crates/{}", slug);
    println!("==================================================================");
    println!("  Compilar Standalone : cargo build --release -p {} --bin {}_app", slug, slug);
    println!("  Empaquetar VST3/CLAP: cargo run --package xtask -- bundle {} --release", slug);
    println!("  Ejecutar Tests      : cargo test -p {}", slug);
    println!("==================================================================");
    Ok(())
}

fn update_cargo_toml_version_str(content: &str, new_version: &str) -> String {
    let mut in_pkg = false;
    let mut out = String::with_capacity(content.len() + 16);
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_pkg = trimmed == "[package]" || trimmed == "[workspace.package]";
        }
        if in_pkg && trimmed.starts_with("version") && trimmed.contains('=') {
            out.push_str(&format!("version = \"{}\"\n", new_version));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn run_set_version(new_version: &str) -> nih_plug_xtask::Result<()> {
    let clean_ver = new_version.trim().trim_start_matches('v');
    let manifests = [
        "Cargo.toml",
        "crates/dsp_core/Cargo.toml",
        "crates/base_plugin_core/Cargo.toml",
        "crates/base_plugin_webview/Cargo.toml",
        "crates/base_plugin_standalone/Cargo.toml",
        "crates/studio_bus_comp/Cargo.toml",
        "crates/xtask/Cargo.toml",
    ];
    for manifest in manifests {
        let path = Path::new(manifest);
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let updated = update_cargo_toml_version_str(&content, clean_ver);
            fs::write(path, updated)?;
            println!("[XTASK VERSION] Updated {} -> {}", manifest, clean_ver);
        }
    }
    let pkg_json = Path::new("packages/base-plugin-ui/package.json");
    if pkg_json.exists() {
        let content = fs::read_to_string(pkg_json)?;
        let mut out = String::new();
        for line in content.lines() {
            if line.trim_start().starts_with("\"version\":") {
                out.push_str(&format!("  \"version\": \"{}\",\n", clean_ver));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        fs::write(pkg_json, out)?;
        println!("[XTASK VERSION] Updated packages/base-plugin-ui/package.json -> {}", clean_ver);
    }
    Ok(())
}

fn main() -> nih_plug_xtask::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("set-version") {
        if let Some(ver) = args.get(1) {
            return run_set_version(ver);
        }
    }
    if args.first().map(|s| s.as_str()) == Some("init-template") {
        return run_init_template(&args[1..]);
    }
    if args.first().map(|s| s.as_str()) == Some("new-plugin") {
        return run_new_plugin(&args[1..]);
    }
    nih_plug_xtask::main()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vst3_class_id_derivation_is_deterministic_and_unique() {
        let id_a = derive_vst3_class_id_from_clap_id("org.openaudiostack.buscomp");
        let id_b = derive_vst3_class_id_from_clap_id("org.openaudiostack.parameq");
        assert_ne!(id_a, id_b);
        assert_eq!(
            id_a,
            derive_vst3_class_id_from_clap_id("org.openaudiostack.buscomp")
        );
        assert_eq!(id_a[6] & 0xf0, 0x50);
        assert_eq!(id_a[8] & 0xc0, 0x80);
    }
}
