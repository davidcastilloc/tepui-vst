use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_rc_exe() -> Option<PathBuf> {
    if let Ok(output) = Command::new("where.exe").arg("rc.exe").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(first_line) = stdout.lines().next() {
                let candidate = PathBuf::from(first_line.trim());
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    let kits_bin = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    if let Ok(entries) = fs::read_dir(kits_bin) {
        let mut candidates: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("x64").join("rc.exe"))
            .filter(|p| p.exists())
            .collect();
        candidates.sort();
        if let Some(best) = candidates.pop() {
            return Some(best);
        }
    }
    None
}

fn main() {
    println!("cargo:rerun-if-changed=../../assets/tepui-icon.ico");
    println!("cargo:rerun-if-changed=ui/tepui-icon.png");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = match env::var("CARGO_MANIFEST_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return,
    };
    let ico_path = manifest_dir
        .join("..")
        .join("..")
        .join("assets")
        .join("tepui-icon.ico");
    if !ico_path.exists() {
        return;
    }

    let out_dir = match env::var("OUT_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => return,
    };

    let rc_file = out_dir.join("tepui_icon.rc");
    let res_file = out_dir.join("tepui_icon.res");
    let escaped_ico = ico_path.to_string_lossy().replace('\\', "\\\\");
    let rc_content = format!("1 ICON \"{}\"\r\n", escaped_ico);

    if fs::write(&rc_file, rc_content).is_err() {
        return;
    }

    if let Some(rc_exe) = find_rc_exe() {
        if let Ok(status) = Command::new(rc_exe)
            .arg("/nologo")
            .arg("/fo")
            .arg(&res_file)
            .arg(&rc_file)
            .status()
        {
            if status.success() && res_file.exists() {
                println!("cargo:rustc-link-arg-bins={}", res_file.display());
            }
        }
    }
}
