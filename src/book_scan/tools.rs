use std::path::{Path, PathBuf};
use std::process::Command;

pub fn find_scantailor(explicit: Option<&Path>) -> Option<PathBuf> {
    find_tool(
        explicit,
        "IMG2PDF_SCANTAILOR",
        if cfg!(windows) {
            &["scantailor-cli.exe", "scantailor-cli"]
        } else {
            &["scantailor-cli", "scantailor-cli.exe"]
        },
    )
}

pub fn find_realesrgan(explicit: Option<&Path>) -> Option<PathBuf> {
    find_tool(
        explicit,
        "IMG2PDF_REALESRGAN",
        if cfg!(windows) {
            &["realesrgan-ncnn-vulkan.exe", "realesrgan-ncnn-vulkan"]
        } else {
            &["realesrgan-ncnn-vulkan", "realesrgan-ncnn-vulkan.exe"]
        },
    )
}

fn find_tool(explicit: Option<&Path>, env_name: &str, names: &[&str]) -> Option<PathBuf> {
    if let Some(path) = explicit.filter(|path| path.is_file()) {
        return Some(path.to_path_buf());
    }
    if let Some(path) = std::env::var_os(env_name).map(PathBuf::from)
        && path.is_file()
    {
        return Some(path);
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for directory in [root.join("tools"), root.to_path_buf()] {
        for name in names {
            let candidate = directory.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|directory| {
            names.iter().find_map(|name| {
                let candidate = directory.join(name);
                candidate.is_file().then_some(candidate)
            })
        })
    })
}

pub fn is_windows_executable(path: &Path) -> bool {
    !cfg!(windows)
        && path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
}

pub fn to_windows_path(path: &Path) -> Result<String, String> {
    if cfg!(windows) {
        return Ok(path.to_string_lossy().to_string());
    }
    let output = Command::new("wslpath")
        .arg("-w")
        .arg(path)
        .output()
        .map_err(|e| format!("wslpathを実行できません: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Windowsパスへ変換できません: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn windows_temp_bridge() -> Result<PathBuf, String> {
    let output = Command::new("cmd.exe")
        .args(["/C", "echo", "%TEMP%"])
        .output()
        .map_err(|e| format!("Windows TEMPを取得できません: {e}"))?;
    if !output.status.success() {
        return Err("Windows TEMPを取得できません".to_string());
    }
    let windows_temp = String::from_utf8_lossy(&output.stdout)
        .trim()
        .trim_end_matches('\r')
        .to_string();
    let output = Command::new("wslpath")
        .arg("-u")
        .arg(&windows_temp)
        .output()
        .map_err(|e| format!("Windows TEMPをWSLパスへ変換できません: {e}"))?;
    if !output.status.success() {
        return Err("Windows TEMPをWSLパスへ変換できません".to_string());
    }
    let unix_temp = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    Ok(PathBuf::from(unix_temp).join(format!("img2pdf-book-{}-{nonce}", std::process::id())))
}
