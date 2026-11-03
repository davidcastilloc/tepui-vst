use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const SFX_MAGIC: &[u8; 12] = b"TEPUI_SFX_V1";

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !dst.exists() {
        fs::create_dir_all(dst)?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let file_name = entry.file_name();
        let dst_name = if file_name == "plugin_vst.vst3" {
            std::ffi::OsString::from("TEPUI.vst3")
        } else {
            file_name
        };
        let dst_path = dst.join(dst_name);
        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn pack_sfx_entries(entries: &[(&str, &[u8])], stub_bytes: &[u8]) -> Vec<u8> {
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

fn unpack_sfx_entries(image: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    if image.len() < 20 {
        return None;
    }
    let trailer_start = image.len() - 20;
    if &image[trailer_start + 8..] != SFX_MAGIC {
        return None;
    }
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
    let mut entries = Vec::with_capacity(count.min(32));
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

fn build_sfx_installer(bundled_dir: &Path) -> std::io::Result<PathBuf> {
    let vst3_bin_path = bundled_dir
        .join("TEPUI.vst3")
        .join("Contents")
        .join("x86_64-win")
        .join("TEPUI.vst3");
    let clap_bin_path = bundled_dir.join("TEPUI.clap");
    let standalone_release = Path::new("target/release/tepui_studio.exe");
    let standalone_debug = Path::new("target/debug/tepui_studio.exe");

    let stub_exe = env::current_exe()?;
    let stub_bytes = fs::read(&stub_exe)?;

    let mut owned_entries: Vec<(String, Vec<u8>)> = Vec::new();
    if vst3_bin_path.exists() {
        owned_entries.push((
            "TEPUI.vst3/Contents/x86_64-win/TEPUI.vst3".to_string(),
            fs::read(&vst3_bin_path)?,
        ));
    }
    if clap_bin_path.exists() {
        owned_entries.push(("TEPUI.clap".to_string(), fs::read(&clap_bin_path)?));
    }
    if standalone_release.exists() {
        owned_entries.push((
            "tepui_studio.exe".to_string(),
            fs::read(standalone_release)?,
        ));
    } else if standalone_debug.exists() {
        owned_entries.push(("tepui_studio.exe".to_string(), fs::read(standalone_debug)?));
    }

    let ref_entries: Vec<(&str, &[u8])> = owned_entries
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    let sfx_bytes = pack_sfx_entries(&ref_entries, &stub_bytes);
    fs::create_dir_all(bundled_dir)?;
    let out_installer = bundled_dir.join("TEPUI-Setup-Windows-x64.exe");
    fs::write(&out_installer, sfx_bytes)?;
    println!(
        "Created self-extracting Windows Installer at '{}'",
        out_installer.display()
    );
    Ok(out_installer)
}

fn try_run_self_extracting_mode() -> Option<nih_plug_xtask::Result<()>> {
    let exe_path = env::current_exe().ok()?;
    let bytes = fs::read(&exe_path).ok()?;
    let entries = unpack_sfx_entries(&bytes)?;

    let temp_extract = env::temp_dir().join("tepui_sfx_extract");
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

    let local_app_data = env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target/user_plugins"));

    let extracted_vst3 = temp_extract.join("TEPUI.vst3");
    if extracted_vst3.exists() {
        let sys_vst3 = Path::new(r"C:\Program Files\Common Files\VST3");
        let usr_vst3 = local_app_data.join(r"Programs\Common\VST3");
        if let Ok(dst) = install_vst3_bundle(&extracted_vst3, sys_vst3, &usr_vst3) {
            println!("[TEPUI SETUP] VST3 instalado en: '{}'", dst.display());
        }
    }

    let extracted_clap = temp_extract.join("TEPUI.clap");
    if extracted_clap.exists() {
        let sys_clap = Path::new(r"C:\Program Files\Common Files\CLAP");
        let usr_clap = local_app_data.join(r"Programs\Common\CLAP");
        if let Ok(dst) = install_clap_bundle(&extracted_clap, sys_clap, &usr_clap) {
            println!("[TEPUI SETUP] CLAP instalado en: '{}'", dst.display());
        }
    }

    let extracted_studio = temp_extract.join("tepui_studio.exe");
    if extracted_studio.exists() {
        let standalone_dir = local_app_data.join(r"Programs\Tepui");
        if fs::create_dir_all(&standalone_dir).is_ok() {
            let standalone_dst = standalone_dir.join("tepui_studio.exe");
            if fs::copy(&extracted_studio, &standalone_dst).is_ok() {
                println!(
                    "[TEPUI SETUP] Standalone Studio instalado en: '{}'",
                    standalone_dst.display()
                );
            }
        }
    }

    let _ = fs::remove_dir_all(&temp_extract);
    Some(Ok(()))
}

fn ensure_tepui_bundles(bundled_dir: &Path) {
    let src_clap = bundled_dir.join("plugin_vst.clap");
    let dst_clap = bundled_dir.join("TEPUI.clap");
    if src_clap.exists() {
        if let Err(e) = fs::copy(&src_clap, &dst_clap) {
            eprintln!("[TEPUI XTASK] Warning copying TEPUI.clap: {}", e);
        } else {
            println!("Created a CLAP bundle at '{}'", dst_clap.display());
        }
    }

    let src_vst3 = bundled_dir.join("plugin_vst.vst3");
    let dst_vst3 = bundled_dir.join("TEPUI.vst3");
    if src_vst3.exists() {
        let _ = fs::remove_dir_all(&dst_vst3);
        if let Err(e) = copy_dir_recursive(&src_vst3, &dst_vst3) {
            eprintln!("[TEPUI XTASK] Warning creating TEPUI.vst3: {}", e);
        } else {
            println!("Created a VST3 bundle at '{}'", dst_vst3.display());
        }
    }

    if dst_clap.exists() || dst_vst3.exists() {
        if let Err(e) = build_sfx_installer(bundled_dir) {
            eprintln!(
                "[TEPUI XTASK] Warning building TEPUI-Setup-Windows-x64.exe: {}",
                e
            );
        }
    }
}

fn install_vst3_bundle(
    src_vst3: &Path,
    preferred_dir: &Path,
    fallback_dir: &Path,
) -> std::io::Result<PathBuf> {
    let try_install = |base_dir: &Path| -> std::io::Result<PathBuf> {
        fs::create_dir_all(base_dir)?;
        let dst = base_dir.join("TEPUI.vst3");
        if dst.exists() {
            let _ = fs::remove_dir_all(&dst);
        }
        copy_dir_recursive(src_vst3, &dst)?;
        Ok(dst)
    };

    try_install(preferred_dir).or_else(|_| try_install(fallback_dir))
}

fn install_clap_bundle(
    src_clap: &Path,
    preferred_dir: &Path,
    fallback_dir: &Path,
) -> std::io::Result<PathBuf> {
    let try_install = |base_dir: &Path| -> std::io::Result<PathBuf> {
        fs::create_dir_all(base_dir)?;
        let dst = base_dir.join("TEPUI.clap");
        fs::copy(src_clap, &dst)?;
        Ok(dst)
    };

    try_install(preferred_dir).or_else(|_| try_install(fallback_dir))
}

fn run_install() -> nih_plug_xtask::Result<()> {
    let bundled_dir = Path::new("target/bundled");
    let dst_vst3 = bundled_dir.join("TEPUI.vst3");
    let dst_clap = bundled_dir.join("TEPUI.clap");

    if !dst_vst3.exists() || !dst_clap.exists() {
        println!(
            "[TEPUI INSTALLER] Bundles no encontrados en target/bundled; compilando release..."
        );
        nih_plug_xtask::main_with_args(
            "cargo xtask",
            vec![
                "bundle".to_string(),
                "plugin_vst".to_string(),
                "--release".to_string(),
            ],
        )?;
        ensure_tepui_bundles(bundled_dir);
    }

    let local_app_data = env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("target/user_plugins"));

    let sys_vst3 = Path::new(r"C:\Program Files\Common Files\VST3");
    let usr_vst3 = local_app_data.join(r"Programs\Common\VST3");
    let installed_vst3 = install_vst3_bundle(&dst_vst3, sys_vst3, &usr_vst3)?;
    println!(
        "[TEPUI INSTALLER] VST3 instalado exitosamente en: '{}'",
        installed_vst3.display()
    );

    let sys_clap = Path::new(r"C:\Program Files\Common Files\CLAP");
    let usr_clap = local_app_data.join(r"Programs\Common\CLAP");
    let installed_clap = install_clap_bundle(&dst_clap, sys_clap, &usr_clap)?;
    println!(
        "[TEPUI INSTALLER] CLAP instalado exitosamente en: '{}'",
        installed_clap.display()
    );

    let standalone_src = Path::new("target/release/tepui_studio.exe");
    if standalone_src.exists() {
        let standalone_dir = local_app_data.join(r"Programs\Tepui");
        if fs::create_dir_all(&standalone_dir).is_ok() {
            let standalone_dst = standalone_dir.join("tepui_studio.exe");
            if fs::copy(standalone_src, &standalone_dst).is_ok() {
                println!(
                    "[TEPUI INSTALLER] Standalone Studio instalado exitosamente en: '{}'",
                    standalone_dst.display()
                );
            }
        }
    }

    let _ = build_sfx_installer(bundled_dir);
    Ok(())
}

fn update_cargo_toml_version_str(content: &str, new_version: &str) -> String {
    let mut out = String::with_capacity(content.len() + 16);
    let mut in_pkg_section = false;
    let mut updated = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_pkg_section = trimmed == "[package]" || trimmed == "[workspace.package]";
            updated = false;
        }
        if in_pkg_section && !updated && trimmed.starts_with("version") && trimmed.contains('=') {
            out.push_str(&format!("version = \"{}\"\n", new_version));
            updated = true;
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
        "crates/synth_core/Cargo.toml",
        "crates/ai_bridge/Cargo.toml",
        "crates/plugin_vst/Cargo.toml",
        "crates/xtask/Cargo.toml",
    ];

    for manifest in manifests {
        let path = Path::new(manifest);
        if path.exists() {
            let content = fs::read_to_string(path)?;
            let updated = update_cargo_toml_version_str(&content, clean_ver);
            fs::write(path, updated)?;
            println!("[TEPUI VERSION] Actualizado {} -> {}", manifest, clean_ver);
        }
    }
    Ok(())
}

fn main() -> nih_plug_xtask::Result<()> {
    if let Some(res) = try_run_self_extracting_mode() {
        return res;
    }

    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("install") {
        return run_install();
    }
    if args.first().map(|s| s.as_str()) == Some("installer") {
        build_sfx_installer(Path::new("target/bundled"))?;
        return Ok(());
    }
    if args.first().map(|s| s.as_str()) == Some("set-version") {
        if let Some(ver) = args.get(1) {
            return run_set_version(ver);
        }
    }

    nih_plug_xtask::main()?;
    ensure_tepui_bundles(Path::new("target/bundled"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_cargo_toml_version() {
        let sample = "[package]\nname = \"plugin_vst\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nsynth_core = { version = \"0.1.0\" }\n";
        let updated = update_cargo_toml_version_str(sample, "1.2.3");
        assert!(updated.contains("version = \"1.2.3\""));
        assert!(updated.contains("synth_core = { version = \"0.1.0\" }"));
    }

    #[test]
    fn test_installer_vst3_and_clap_copy_with_fallback() {
        let temp_root = std::env::temp_dir().join("tepui_xtask_installer_test");
        let _ = fs::remove_dir_all(&temp_root);

        let src_root = temp_root.join("src");
        let src_vst3 = src_root
            .join("plugin_vst.vst3")
            .join("Contents")
            .join("x86_64-win");
        fs::create_dir_all(&src_vst3).expect("create src_vst3 failed");
        fs::write(src_vst3.join("plugin_vst.vst3"), b"MOCK_VST3_BIN").expect("write vst3 failed");

        let src_clap = src_root.join("TEPUI.clap");
        fs::write(&src_clap, b"MOCK_CLAP_BIN").expect("write clap failed");

        let dst_root = temp_root.join("dst");
        let vst3_out = install_vst3_bundle(
            &src_root.join("plugin_vst.vst3"),
            &dst_root.join("VST3"),
            &dst_root.join("FallbackVST3"),
        )
        .expect("install_vst3_bundle failed");
        assert!(vst3_out
            .join("Contents")
            .join("x86_64-win")
            .join("TEPUI.vst3")
            .exists());

        let clap_out = install_clap_bundle(
            &src_clap,
            &dst_root.join("CLAP"),
            &dst_root.join("FallbackCLAP"),
        )
        .expect("install_clap_bundle failed");
        assert!(clap_out.exists());

        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn test_sfx_installer_pack_and_unpack_roundtrip() {
        let stub = b"MZ_FAKE_PE_HEADER_STUB";
        let entries: [(&str, &[u8]); 3] = [
            (
                "TEPUI.vst3/Contents/x86_64-win/TEPUI.vst3",
                b"VST3_PAYLOAD_DATA",
            ),
            ("TEPUI.clap", b"CLAP_PAYLOAD_DATA"),
            ("tepui_studio.exe", b"STUDIO_PAYLOAD_DATA"),
        ];
        let image = pack_sfx_entries(&entries, stub);
        let unpacked = unpack_sfx_entries(&image).expect("unpack_sfx_entries should succeed");
        assert_eq!(unpacked.len(), 3);
        assert_eq!(unpacked[0].0, "TEPUI.vst3/Contents/x86_64-win/TEPUI.vst3");
        assert_eq!(unpacked[0].1, b"VST3_PAYLOAD_DATA");
        assert_eq!(unpacked[1].0, "TEPUI.clap");
        assert_eq!(unpacked[1].1, b"CLAP_PAYLOAD_DATA");
        assert_eq!(unpacked[2].0, "tepui_studio.exe");
        assert_eq!(unpacked[2].1, b"STUDIO_PAYLOAD_DATA");
    }
}
