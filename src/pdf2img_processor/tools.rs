use super::Pdf2ImgProcessor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PROJECT_TOOLS_DIR: &str = "tools";
const PDFTOPPM_TOOL_NAMES: &[&str] = if cfg!(windows) {
    &["pdftoppm.exe", "pdftoppm"]
} else {
    &["pdftoppm", "pdftoppm.exe"]
};
const PDFINFO_TOOL_NAMES: &[&str] = if cfg!(windows) {
    &["pdfinfo.exe", "pdfinfo"]
} else {
    &["pdfinfo", "pdfinfo.exe"]
};
const PDFIMAGES_TOOL_NAMES: &[&str] = if cfg!(windows) {
    &["pdfimages.exe", "pdfimages"]
} else {
    &["pdfimages", "pdfimages.exe"]
};

impl Pdf2ImgProcessor {
    /// 利用可能な pdftoppm を探す。
    pub fn find_pdftoppm() -> Option<PathBuf> {
        static PDFTOPPM_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PDFTOPPM_PATH
            .get_or_init(|| find_tool(PDFTOPPM_TOOL_NAMES))
            .clone()
    }

    /// 利用可能な pdfinfo を探す。
    pub fn find_pdfinfo() -> Option<PathBuf> {
        static PDFINFO_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PDFINFO_PATH
            .get_or_init(|| find_tool(PDFINFO_TOOL_NAMES))
            .clone()
    }

    /// 利用可能な pdfimages を探す。
    pub fn find_pdfimages() -> Option<PathBuf> {
        static PDFIMAGES_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
        PDFIMAGES_PATH
            .get_or_init(|| find_tool(PDFIMAGES_TOOL_NAMES))
            .clone()
    }
}

fn find_tool(tool_names: &[&str]) -> Option<PathBuf> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for tool_name in tool_names {
        for dir in [
            manifest_dir.join(PROJECT_TOOLS_DIR),
            manifest_dir.to_path_buf(),
        ] {
            let candidate = dir.join(tool_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            tool_names.iter().find_map(|tool_name| {
                let candidate = dir.join(tool_name);
                if candidate.is_file() {
                    Some(candidate)
                } else {
                    None
                }
            })
        })
    })
}
