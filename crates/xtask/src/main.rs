use std::env;
use std::fs;
use std::path::{Path, PathBuf};

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
}

fn install_vst3_bundle(src_vst3: &Path, preferred_dir: &Path, fallback_dir: &Path) -> std::io::Result<PathBuf> {
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

fn install_clap_bundle(src_clap: &Path, preferred_dir: &Path, fallback_dir: &Path) -> std::io::Result<PathBuf> {
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
        println!("[TEPUI INSTALLER] Bundles no encontrados en target/bundled; compilando release...");
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

    Ok(())
}

fn main() -> nih_plug_xtask::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) == Some("install") {
        return run_install();
    }

    nih_plug_xtask::main()?;
    ensure_tepui_bundles(Path::new("target/bundled"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_installer_vst3_and_clap_copy_with_fallback() {
        let temp_root = std::env::temp_dir().join("tepui_xtask_installer_test");
        let _ = fs::remove_dir_all(&temp_root);

        let src_root = temp_root.join("src");
        let src_vst3 = src_root.join("plugin_vst.vst3").join("Contents").join("x86_64-win");
        fs::create_dir_all(&src_vst3).expect("create src_vst3 failed");
        fs::write(src_vst3.join("plugin_vst.vst3"), b"MOCK_VST3_BIN").expect("write vst3 failed");

        let src_clap = src_root.join("TEPUI.clap");
        fs::write(&src_clap, b"MOCK_CLAP_BIN").expect("write clap failed");

        let dst_root = temp_root.join("dst");
        let vst3_out = install_vst3_bundle(&src_root.join("plugin_vst.vst3"), &dst_root.join("VST3"), &dst_root.join("FallbackVST3"))
            .expect("install_vst3_bundle failed");
        assert!(vst3_out.join("Contents").join("x86_64-win").join("TEPUI.vst3").exists());

        let clap_out = install_clap_bundle(&src_clap, &dst_root.join("CLAP"), &dst_root.join("FallbackCLAP"))
            .expect("install_clap_bundle failed");
        assert!(clap_out.exists());

        let _ = fs::remove_dir_all(&temp_root);
    }
}
