use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SFX_MAGIC: &[u8; 12] = b"OAPS_SFX_V1!";
const SFX_SEARCH_WINDOW_BYTES: usize = 524_288;

const UPSTREAM_SDK_CRATE_PATHS: &[&str] = &[
    "crates/dsp_core",
    "crates/base_plugin_core",
    "crates/base_plugin_webview",
    "crates/base_plugin_standalone",
    "crates/studio_bus_comp",
];

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

fn parse_quoted_toml_value(line: &str) -> Option<String> {
    let (_, rhs) = line.split_once('=')?;
    let trimmed = rhs.trim();
    let rest = trimmed.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub fn resolve_bundle_name_from_str(bundler_toml: &str, package: &str) -> String {
    let mut current_section = "";
    for raw_line in bundler_toml.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(sec) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            current_section = sec.trim();
            continue;
        }
        if current_section == package && line.starts_with("name") {
            if let Some(val) = parse_quoted_toml_value(line) {
                if !val.is_empty() {
                    return val;
                }
            }
        }
    }
    package.to_string()
}

fn first_configured_package_from_bundler_str(bundler_toml: &str) -> Option<String> {
    for raw_line in bundler_toml.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if let Some(sec) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let trimmed = sec.trim();
            if !trimmed.is_empty() && !trimmed.contains('.') {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

pub fn resolve_default_bin_from_cargo_toml(cargo_toml: &str, fallback_pkg: &str) -> String {
    let mut section = "";
    let mut first_bin_name: Option<String> = None;

    for raw_line in cargo_toml.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line;
            continue;
        }
        if section == "[package]" && line.starts_with("default-run") {
            if let Some(val) = parse_quoted_toml_value(line) {
                if !val.is_empty() {
                    return val;
                }
            }
        } else if section == "[[bin]]" && first_bin_name.is_none() && line.starts_with("name") {
            first_bin_name = parse_quoted_toml_value(line);
        }
    }

    first_bin_name.unwrap_or_else(|| fallback_pkg.to_string())
}

fn parse_workspace_members(root_cargo_toml: &str) -> Vec<String> {
    let mut in_workspace = false;
    let mut in_members = false;
    let mut members = Vec::new();

    for raw_line in root_cargo_toml.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') && !in_members {
            in_workspace = line == "[workspace]";
            continue;
        }
        if in_workspace && !in_members && line.starts_with("members") {
            if let Some((_, rhs)) = line.split_once('=') {
                in_members = true;
                extract_quoted_strings(rhs, &mut members);
                if rhs.contains(']') {
                    break;
                }
            }
            continue;
        }
        if in_members {
            extract_quoted_strings(line, &mut members);
            if line.contains(']') {
                break;
            }
        }
    }
    members
}

fn extract_quoted_strings(s: &str, out: &mut Vec<String>) {
    let mut rest = s;
    while let Some(start) = rest.find('"') {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_string());
            rest = &rest[end + 1..];
        } else {
            break;
        }
    }
}

pub fn collect_version_target_manifests(
    workspace_root: &Path,
    include_sdk: bool,
) -> std::io::Result<Vec<String>> {
    let root_toml_path = workspace_root.join("Cargo.toml");
    let root_toml = fs::read_to_string(&root_toml_path)?;
    let members = parse_workspace_members(&root_toml);

    let mut manifests = vec!["Cargo.toml".to_string()];
    for member in members {
        if !include_sdk && UPSTREAM_SDK_CRATE_PATHS.contains(&member.as_str()) {
            continue;
        }
        manifests.push(format!("{}/Cargo.toml", member));
    }
    Ok(manifests)
}

fn extract_bundled_packages(args: &[String]) -> Vec<String> {
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");
    if cmd != "bundle" && cmd != "bundle-universal" {
        return Vec::new();
    }

    let mut packages = Vec::new();
    let mut i = 1;
    let mut saw_p_flag = false;
    while i < args.len() {
        match args[i].as_str() {
            "-p" | "--package" if i + 1 < args.len() => {
                saw_p_flag = true;
                packages.push(args[i + 1].clone());
                i += 2;
            }
            arg if !saw_p_flag && packages.is_empty() && !arg.starts_with('-') => {
                packages.push(arg.to_string());
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    packages
}

pub fn pack_sfx_entries(entries: &[(&str, &[u8])], stub_bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(stub_bytes.len() + 1024);
    out.extend_from_slice(stub_bytes);
    let payload_start = out.len() as u64;
    out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (rel_path, data) in entries {
        let path_bytes = rel_path.as_bytes();
        out.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(path_bytes);
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&payload_start.to_le_bytes());
    out.extend_from_slice(SFX_MAGIC);
    out
}

pub fn unpack_sfx_entries(image: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    if image.len() < 20 {
        return None;
    }
    let search_floor = image.len().saturating_sub(SFX_SEARCH_WINDOW_BYTES);
    let magic_pos = image[search_floor..]
        .windows(SFX_MAGIC.len())
        .rposition(|w| w == SFX_MAGIC)
        .map(|rel| search_floor + rel)?;
    if magic_pos < 8 {
        return None;
    }
    let trailer_start = magic_pos - 8;
    let mut off_bytes = [0u8; 8];
    off_bytes.copy_from_slice(&image[trailer_start..trailer_start + 8]);
    let payload_start = u64::from_le_bytes(off_bytes) as usize;
    if payload_start + 4 > trailer_start {
        return None;
    }
    let mut cursor = payload_start;
    let mut count_bytes = [0u8; 4];
    count_bytes.copy_from_slice(&image[cursor..cursor + 4]);
    cursor += 4;
    let count = u32::from_le_bytes(count_bytes) as usize;
    let mut entries = Vec::with_capacity(count.min(64));
    for _ in 0..count {
        if cursor + 4 > trailer_start {
            return None;
        }
        let mut plen_bytes = [0u8; 4];
        plen_bytes.copy_from_slice(&image[cursor..cursor + 4]);
        cursor += 4;
        let plen = u32::from_le_bytes(plen_bytes) as usize;
        if cursor + plen + 8 > trailer_start {
            return None;
        }
        let rel_path = String::from_utf8(image[cursor..cursor + plen].to_vec()).ok()?;
        cursor += plen;
        let mut dlen_bytes = [0u8; 8];
        dlen_bytes.copy_from_slice(&image[cursor..cursor + 8]);
        cursor += 8;
        let dlen = u64::from_le_bytes(dlen_bytes) as usize;
        if cursor + dlen > trailer_start {
            return None;
        }
        let data = image[cursor..cursor + dlen].to_vec();
        cursor += dlen;
        entries.push((rel_path, data));
    }
    Some(entries)
}

fn collect_dir_files_relative(
    current_dir: &Path,
    rel_prefix: &str,
    out: &mut Vec<(String, Vec<u8>)>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(current_dir)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = format!("{}/{}", rel_prefix, name);
        let path = entry.path();
        if ty.is_dir() {
            collect_dir_files_relative(&path, &rel, out)?;
        } else {
            out.push((rel, fs::read(&path)?));
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

fn resolve_package_bundle_and_bin(package: &str) -> (String, String) {
    let bundler_raw = fs::read_to_string("bundler.toml").unwrap_or_default();
    let bundle_name = resolve_bundle_name_from_str(&bundler_raw, package);

    let crate_toml_path = Path::new("crates").join(package).join("Cargo.toml");
    let crate_toml = fs::read_to_string(crate_toml_path).unwrap_or_default();
    let default_bin = resolve_default_bin_from_cargo_toml(&crate_toml, package);

    (bundle_name, default_bin)
}

fn build_sfx_installer_for_package(
    bundled_dir: &Path,
    package: &str,
) -> std::io::Result<Option<PathBuf>> {
    let (bundle_name, default_bin) = resolve_package_bundle_and_bin(package);
    let vst3_dir_name = format!("{}.vst3", bundle_name);
    let clap_file_name = format!("{}.clap", bundle_name);
    let standalone_exe_name = format!("{}.exe", default_bin);

    let vst3_dir = bundled_dir.join(&vst3_dir_name);
    let clap_path = bundled_dir.join(&clap_file_name);

    let mut owned_entries: Vec<(String, Vec<u8>)> = Vec::new();
    if vst3_dir.is_dir() {
        collect_dir_files_relative(&vst3_dir, &vst3_dir_name, &mut owned_entries)?;
    }
    if clap_path.is_file() {
        owned_entries.push((clap_file_name, fs::read(&clap_path)?));
    }
    if owned_entries.is_empty() {
        return Ok(None);
    }

    let standalone_release = Path::new("target/release").join(&standalone_exe_name);
    let standalone_debug = Path::new("target/debug").join(&standalone_exe_name);
    if standalone_release.is_file() {
        owned_entries.push((standalone_exe_name, fs::read(standalone_release)?));
    } else if standalone_debug.is_file() {
        owned_entries.push((standalone_exe_name, fs::read(standalone_debug)?));
    }

    let stub_exe = env::current_exe()?;
    let stub_bytes = fs::read(stub_exe)?;
    let ref_entries: Vec<(&str, &[u8])> = owned_entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();

    let sfx_bytes = pack_sfx_entries(&ref_entries, &stub_bytes);
    fs::create_dir_all(bundled_dir)?;
    let out_installer = bundled_dir.join(format!("{}-Setup-Windows-x64.exe", bundle_name));
    fs::write(&out_installer, sfx_bytes)?;
    println!(
        "Created self-extracting Windows Installer at '{}'",
        out_installer.display()
    );
    Ok(Some(out_installer))
}

fn install_bundle_entry(
    src: &Path,
    preferred_dir: &Path,
    fallback_dir: &Path,
) -> std::io::Result<PathBuf> {
    let file_name = src.file_name().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing file name")
    })?;
    let try_install = |base_dir: &Path| -> std::io::Result<PathBuf> {
        fs::create_dir_all(base_dir)?;
        let dst = base_dir.join(file_name);
        if src.is_dir() {
            if dst.exists() {
                let _ = fs::remove_dir_all(&dst);
            }
            copy_dir_recursive(src, &dst)?;
        } else {
            fs::copy(src, &dst)?;
        }
        Ok(dst)
    };

    try_install(preferred_dir).or_else(|_| try_install(fallback_dir))
}

fn default_plugin_install_dirs(bundle_name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf, PathBuf) {
    if cfg!(target_os = "macos") {
        let home = env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("target/user_plugins"));
        (
            PathBuf::from("/Library/Audio/Plug-Ins/VST3"),
            home.join("Library/Audio/Plug-Ins/VST3"),
            PathBuf::from("/Library/Audio/Plug-Ins/CLAP"),
            home.join("Library/Audio/Plug-Ins/CLAP"),
            home.join("Applications").join(bundle_name),
        )
    } else if cfg!(target_os = "linux") {
        let home = env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("target/user_plugins"));
        (
            PathBuf::from("/usr/lib/vst3"),
            home.join(".vst3"),
            PathBuf::from("/usr/lib/clap"),
            home.join(".clap"),
            home.join(".local/bin"),
        )
    } else {
        let local_app_data = env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("target/user_plugins"));
        (
            PathBuf::from(r"C:\Program Files\Common Files\VST3"),
            local_app_data.join(r"Programs\Common\VST3"),
            PathBuf::from(r"C:\Program Files\Common Files\CLAP"),
            local_app_data.join(r"Programs\Common\CLAP"),
            local_app_data.join("Programs").join(bundle_name),
        )
    }
}

fn try_run_self_extracting_mode() -> Option<nih_plug_xtask::Result<()>> {
    let exe_path = env::current_exe().ok()?;
    let bytes = fs::read(&exe_path).ok()?;
    let entries = unpack_sfx_entries(&bytes)?;

    let temp_extract = env::temp_dir().join("oaps_sfx_extract");
    let _ = fs::remove_dir_all(&temp_extract);
    for (rel_path, data) in &entries {
        let target = temp_extract.join(rel_path);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&target, data) {
            return Some(Err(e.into()));
        }
    }

    let mut inferred_bundle_name = String::from("AudioPlugin");
    if let Ok(read_dir) = fs::read_dir(&temp_extract) {
        for item in read_dir.flatten() {
            let name = item.file_name().to_string_lossy().to_string();
            if let Some(stem) = name
                .strip_suffix(".vst3")
                .or_else(|| name.strip_suffix(".clap"))
            {
                inferred_bundle_name = stem.to_string();
                break;
            }
        }
    }

    let (sys_vst3, usr_vst3, sys_clap, usr_clap, standalone_dir) =
        default_plugin_install_dirs(&inferred_bundle_name);

    if let Ok(read_dir) = fs::read_dir(&temp_extract) {
        for item in read_dir.flatten() {
            let path = item.path();
            let name = item.file_name().to_string_lossy().to_string();
            if name.ends_with(".vst3") {
                if let Ok(dst) = install_bundle_entry(&path, &sys_vst3, &usr_vst3) {
                    println!("[PLUGIN SETUP] VST3 installed at: '{}'", dst.display());
                }
            } else if name.ends_with(".clap") {
                if let Ok(dst) = install_bundle_entry(&path, &sys_clap, &usr_clap) {
                    println!("[PLUGIN SETUP] CLAP installed at: '{}'", dst.display());
                }
            } else if name.ends_with(".exe") && fs::create_dir_all(&standalone_dir).is_ok() {
                let dst = standalone_dir.join(&name);
                if fs::copy(&path, &dst).is_ok() {
                    println!(
                        "[PLUGIN SETUP] Standalone installed at: '{}'",
                        dst.display()
                    );
                }
            }
        }
    }

    let _ = fs::remove_dir_all(&temp_extract);
    Some(Ok(()))
}

fn default_target_package() -> String {
    if let Ok(bundler_raw) = fs::read_to_string("bundler.toml") {
        if let Some(pkg) = first_configured_package_from_bundler_str(&bundler_raw) {
            return pkg;
        }
    }
    String::from("studio_bus_comp")
}

fn run_install(args: &[String]) -> nih_plug_xtask::Result<()> {
    let package = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_else(default_target_package);
    let (bundle_name, default_bin) = resolve_package_bundle_and_bin(&package);
    let bundled_dir = Path::new("target/bundled");
    let dst_vst3 = bundled_dir.join(format!("{}.vst3", bundle_name));
    let dst_clap = bundled_dir.join(format!("{}.clap", bundle_name));

    if !dst_vst3.exists() || !dst_clap.exists() {
        println!(
            "[XTASK INSTALL] Bundles for '{}' not found in target/bundled; building release...",
            package
        );
        nih_plug_xtask::main_with_args(
            "cargo xtask",
            vec![
                "bundle".to_string(),
                package.clone(),
                "--release".to_string(),
            ],
        )?;
    }

    let (sys_vst3, usr_vst3, sys_clap, usr_clap, standalone_dir) =
        default_plugin_install_dirs(&bundle_name);

    if dst_vst3.exists() {
        let installed = install_bundle_entry(&dst_vst3, &sys_vst3, &usr_vst3)?;
        println!(
            "[XTASK INSTALL] VST3 installed at: '{}'",
            installed.display()
        );
    }
    if dst_clap.exists() {
        let installed = install_bundle_entry(&dst_clap, &sys_clap, &usr_clap)?;
        println!(
            "[XTASK INSTALL] CLAP installed at: '{}'",
            installed.display()
        );
    }

    let standalone_bin_name = if cfg!(windows) {
        format!("{}.exe", default_bin)
    } else {
        default_bin
    };
    let standalone_src = Path::new("target/release").join(&standalone_bin_name);
    if standalone_src.exists() && fs::create_dir_all(&standalone_dir).is_ok() {
        let standalone_dst = standalone_dir.join(&standalone_bin_name);
        if fs::copy(&standalone_src, &standalone_dst).is_ok() {
            println!(
                "[XTASK INSTALL] Standalone installed at: '{}'",
                standalone_dst.display()
            );
        }
    }

    if cfg!(windows) {
        let _ = build_sfx_installer_for_package(bundled_dir, &package);
    }
    Ok(())
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

fn sanitize_bundle_filename(input: &str) -> String {
    let filtered: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == ' ')
        .collect();
    let trimmed = filtered.trim();
    if trimmed.is_empty() {
        to_snake_slug(input)
    } else {
        trimmed.to_string()
    }
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
        .replace("*b\"StBsCmpOpenAudv1\"", &format!("[{}]", bytes_fmt));

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

    // Register display bundle name in bundler.toml
    let bundler_path = Path::new("bundler.toml");
    let bundle_display = sanitize_bundle_filename(&plugin_name);
    let section_header = format!("[{}]", slug);
    let existing_bundler = fs::read_to_string(bundler_path).unwrap_or_default();
    if !existing_bundler.contains(&section_header) {
        let mut updated_bundler = existing_bundler;
        if !updated_bundler.is_empty() && !updated_bundler.ends_with('\n') {
            updated_bundler.push('\n');
        }
        if !updated_bundler.is_empty() {
            updated_bundler.push('\n');
        }
        updated_bundler.push_str(&format!("[{}]\nname = \"{}\"\n", slug, bundle_display));
        fs::write(bundler_path, updated_bundler)?;
    }

    println!("==================================================================");
    println!(" NUEVO PLUGIN CREADO EXITOSAMENTE: crates/{}", slug);
    println!("==================================================================");
    println!(
        "  Compilar Standalone : cargo build --release -p {} --bin {}_app",
        slug, slug
    );
    println!(
        "  Empaquetar VST3/CLAP: cargo run --package xtask -- bundle {} --release",
        slug
    );
    println!("  Ejecutar Tests      : cargo test -p {}", slug);
    println!("==================================================================");
    Ok(())
}

fn update_cargo_toml_version_str(content: &str, new_version: &str) -> String {
    let mut in_pkg = false;
    let mut updated = false;
    let mut out = String::with_capacity(content.len() + 16);
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_pkg = trimmed == "[package]" || trimmed == "[workspace.package]";
            updated = false;
        }
        if in_pkg && !updated && trimmed.starts_with("version") && trimmed.contains('=') {
            out.push_str(&format!("version = \"{}\"\n", new_version));
            updated = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn run_set_version(args: &[String]) -> nih_plug_xtask::Result<()> {
    let Some(raw_ver) = args.iter().find(|a| !a.starts_with('-')) else {
        return Ok(());
    };
    let clean_ver = raw_ver.trim().trim_start_matches('v');
    let is_downstream_plugin = Path::new(".github/workflows/sync-upstream-sdk.yml").exists();
    let include_sdk = args.iter().any(|a| a == "--include-sdk") || !is_downstream_plugin;

    let manifests = collect_version_target_manifests(Path::new("."), include_sdk)?;
    for manifest in manifests {
        let path = Path::new(&manifest);
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let updated = update_cargo_toml_version_str(&content, clean_ver);
            fs::write(path, updated)?;
            println!("[XTASK VERSION] Updated {} -> {}", manifest, clean_ver);
        }
    }

    if include_sdk {
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
            println!(
                "[XTASK VERSION] Updated packages/base-plugin-ui/package.json -> {}",
                clean_ver
            );
        }
    }
    Ok(())
}

fn main() -> nih_plug_xtask::Result<()> {
    if let Some(res) = try_run_self_extracting_mode() {
        return res;
    }

    let args: Vec<String> = env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("install") => return run_install(&args[1..]),
        Some("installer") => {
            let pkg = args
                .get(1)
                .filter(|a| !a.starts_with('-'))
                .cloned()
                .unwrap_or_else(default_target_package);
            build_sfx_installer_for_package(Path::new("target/bundled"), &pkg)?;
            return Ok(());
        }
        Some("set-version") => return run_set_version(&args[1..]),
        Some("init-template") => return run_init_template(&args[1..]),
        Some("new-plugin") => return run_new_plugin(&args[1..]),
        _ => {}
    }

    let bundled_pkgs = extract_bundled_packages(&args);
    nih_plug_xtask::main()?;

    if cfg!(windows) {
        let bundled_dir = Path::new("target/bundled");
        for pkg in &bundled_pkgs {
            if let Err(e) = build_sfx_installer_for_package(bundled_dir, pkg) {
                eprintln!(
                    "[XTASK] Warning building Windows SFX installer for '{}': {}",
                    pkg, e
                );
            }
        }
    }
    Ok(())
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

    #[test]
    fn test_workspace_bundler_toml_maps_primary_plugin_bundle_and_default_bin() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        let bundler_path = workspace_root.join("bundler.toml");
        assert!(
            bundler_path.exists(),
            "bundler.toml must exist at workspace root for nih_plug_xtask bundle naming"
        );
        let bundler_raw = fs::read_to_string(&bundler_path).expect("read bundler.toml");
        assert_eq!(
            resolve_bundle_name_from_str(&bundler_raw, "plugin_vst"),
            "TEPUI"
        );
        assert_eq!(
            resolve_bundle_name_from_str(&bundler_raw, "unlisted_plugin"),
            "unlisted_plugin"
        );

        let plugin_toml = fs::read_to_string(
            workspace_root
                .join("crates")
                .join("plugin_vst")
                .join("Cargo.toml"),
        )
        .expect("read plugin_vst Cargo.toml");
        assert_eq!(
            resolve_default_bin_from_cargo_toml(&plugin_toml, "plugin_vst"),
            "tepui_studio"
        );
    }

    #[test]
    fn test_downstream_set_version_targets_product_crates_and_preserves_sdk() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|p| p.parent())
            .expect("workspace root");
        let downstream_manifests = collect_version_target_manifests(workspace_root, false)
            .expect("collect downstream manifests");

        assert!(downstream_manifests.contains(&"Cargo.toml".to_string()));
        assert!(downstream_manifests.contains(&"crates/plugin_vst/Cargo.toml".to_string()));
        assert!(downstream_manifests.contains(&"crates/synth_core/Cargo.toml".to_string()));
        assert!(downstream_manifests.contains(&"crates/ai_bridge/Cargo.toml".to_string()));
        assert!(downstream_manifests.contains(&"crates/xtask/Cargo.toml".to_string()));

        assert!(!downstream_manifests.contains(&"crates/dsp_core/Cargo.toml".to_string()));
        assert!(!downstream_manifests.contains(&"crates/base_plugin_core/Cargo.toml".to_string()));
        assert!(
            !downstream_manifests.contains(&"crates/base_plugin_webview/Cargo.toml".to_string())
        );
        assert!(
            !downstream_manifests.contains(&"crates/base_plugin_standalone/Cargo.toml".to_string())
        );

        let sdk_manifests = collect_version_target_manifests(workspace_root, true)
            .expect("collect upstream sdk manifests");
        assert!(sdk_manifests.contains(&"crates/dsp_core/Cargo.toml".to_string()));
        assert!(sdk_manifests.contains(&"crates/base_plugin_core/Cargo.toml".to_string()));
    }

    #[test]
    fn test_generic_sfx_installer_roundtrip_with_authenticode_padding() {
        let stub = b"MZ_FAKE_PE_HEADER_STUB";
        let entries: [(&str, &[u8]); 3] = [
            (
                "MySynth.vst3/Contents/x86_64-win/MySynth.vst3",
                b"VST3_PAYLOAD_DATA",
            ),
            ("MySynth.clap", b"CLAP_PAYLOAD_DATA"),
            ("my_synth_app.exe", b"STANDALONE_PAYLOAD_DATA"),
        ];
        let image = pack_sfx_entries(&entries, stub);
        let mut signed_image = image;
        signed_image.extend_from_slice(&[0x30u8; 8192]);

        let unpacked = unpack_sfx_entries(&signed_image)
            .expect("SFX unpack should succeed even with appended Authenticode PKCS#7 signature");
        assert_eq!(unpacked.len(), 3);
        assert_eq!(
            unpacked[0].0,
            "MySynth.vst3/Contents/x86_64-win/MySynth.vst3"
        );
        assert_eq!(unpacked[0].1, b"VST3_PAYLOAD_DATA");
        assert_eq!(unpacked[1].0, "MySynth.clap");
        assert_eq!(unpacked[2].0, "my_synth_app.exe");
    }
}
