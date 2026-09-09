use super::config::BookScanConfig;
use super::manifest::{BookScanStage, PageRecord};
use super::tools::find_scantailor;
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug)]
struct ImageInfo {
    file_id: String,
    width: u32,
    height: u32,
}

#[derive(Debug)]
struct OutputInfo {
    page_id: String,
    width: u32,
    height: u32,
    restore_x: f32,
    restore_y: f32,
    crop_point_seen: bool,
}

pub fn process(
    config: &BookScanConfig,
    work_dir: &Path,
    pages: &mut [PageRecord],
) -> Result<PathBuf, String> {
    if !config.scantailor_enabled {
        for page in pages {
            set_passthrough(page);
        }
        return Ok(work_dir.join("scantailor-disabled.ScanTailor"));
    }

    for page in pages
        .iter_mut()
        .filter(|page| page.is_blank || config.crop_excluded(page.page_number))
    {
        set_passthrough(page);
    }
    if pages
        .iter()
        .all(|page| page.is_blank || config.crop_excluded(page.page_number))
    {
        return Ok(work_dir.join("all-pages-blank.ScanTailor"));
    }

    let executable = find_scantailor(config.scantailor_path.as_deref()).ok_or_else(|| {
        "scantailor-cliが見つかりません。--scantailor-pathまたはIMG2PDF_SCANTAILORを指定してください"
            .to_string()
    })?;
    let source_dir = work_dir.join("scantailor-input");
    let output_dir = work_dir.join("scantailor");
    let project_path = work_dir.join("book.ScanTailor");
    if source_dir.exists() {
        fs::remove_dir_all(&source_dir)
            .map_err(|e| format!("旧ScanTailor入力フォルダを除去できません: {e}"))?;
    }
    fs::create_dir_all(&source_dir)
        .map_err(|e| format!("ScanTailor入力フォルダを作成できません: {e}"))?;
    for page in pages
        .iter()
        .filter(|page| !page.is_blank && !config.crop_excluded(page.page_number))
    {
        let filename = page
            .source_path
            .file_name()
            .ok_or_else(|| format!("入力ファイル名が不正です: {}", page.source_path.display()))?;
        let staged = source_dir.join(filename);
        if fs::hard_link(&page.source_path, &staged).is_err() {
            fs::copy(&page.source_path, &staged)
                .map_err(|e| format!("ScanTailor入力を準備できません: {e}"))?;
        }
    }
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("ScanTailor出力フォルダを作成できません: {e}"))?;

    let mut command = Command::new(&executable);
    command.arg("--layout=1");
    configure_deskew(&mut command, config.deskew_enabled);
    command
        .arg(format!("--dpi={}", config.dpi))
        .arg(format!("--output-dpi={}", config.output_dpi))
        .arg(format!("--margins={}", config.margins))
        .arg("--alignment=original")
        .arg("--match-layout=false")
        .arg("--color-mode=color_grayscale")
        .arg(format!("--despeckle={}", config.despeckle))
        .arg(if config.dewarp_enabled {
            "--dewarping=auto"
        } else {
            "--dewarping=off"
        })
        .arg("--tiff-force-rgb")
        .arg(format!("--output-project={}", project_path.display()));

    if config.crop_enabled {
        command
            .arg("--enable-page-detection")
            .arg("--enable-fine-tuning")
            .arg(format!(
                "--page-detection-tolerance={}",
                config.page_detection_tolerance
            ));
    } else {
        command
            .arg("--force-disable-page-detection")
            .arg("--disable-content-detection");
    }
    if config.normalize_illumination {
        command.arg("--normalize-illumination");
    }
    command.arg(&source_dir).arg(&output_dir);

    let output = command
        .output()
        .map_err(|e| format!("ScanTailorを実行できません: {e}"))?;
    fs::write(
        work_dir.join("scantailor.log"),
        [&output.stdout[..], &output.stderr[..]].concat(),
    )
    .map_err(|e| format!("ScanTailor logを保存できません: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "ScanTailorが失敗しました: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if !project_path.is_file() {
        return Err("ScanTailor projectが生成されませんでした".to_string());
    }

    let parsed = parse_project(&project_path)?;
    for page in pages
        .iter_mut()
        .filter(|page| !page.is_blank && !config.crop_excluded(page.page_number))
    {
        let info = parsed
            .get(&page.stem)
            .ok_or_else(|| format!("ScanTailor projectに{}の出力情報がありません", page.stem))?;
        let crop_path = find_output_image(&output_dir, &page.stem)
            .ok_or_else(|| format!("ScanTailor出力がありません: {}", page.stem))?;
        let actual = image::image_dimensions(&crop_path)
            .map_err(|e| format!("ScanTailor出力を読めません: {e}"))?;
        if actual != (info.width, info.height) {
            return Err(format!(
                "ScanTailor出力寸法が不一致です: {} expected={}x{} actual={}x{}",
                page.stem, info.width, info.height, actual.0, actual.1
            ));
        }
        page.source_width = info.source_width;
        page.source_height = info.source_height;
        page.crop_path = Some(crop_path);
        page.crop_width = info.width;
        page.crop_height = info.height;
        page.restore_x = info.restore_x;
        page.restore_y = info.restore_y;
        page.stage = BookScanStage::ScanTailored;
        page.error = None;
    }
    Ok(project_path)
}

fn configure_deskew(command: &mut Command, enabled: bool) {
    // ScanTailor Advanced 1.0.16のConsoleBatch::setupDeskew()は
    // --deskewが存在すると値がautoでもMODE_MANUAL angle=0を設定する。
    // autoはCLI既定値なので、deskew/rotateをどちらも渡さない。
    if !enabled {
        command.arg("--rotate=0").arg("--deskew=manual");
    }
}

fn set_passthrough(page: &mut PageRecord) {
    page.crop_path = Some(page.source_path.clone());
    page.crop_width = page.source_width;
    page.crop_height = page.source_height;
    page.restore_x = 0.0;
    page.restore_y = 0.0;
    page.stage = BookScanStage::ScanTailored;
    page.error = None;
}

#[derive(Debug, Clone)]
struct ParsedPage {
    source_width: u32,
    source_height: u32,
    width: u32,
    height: u32,
    restore_x: f32,
    restore_y: f32,
}

fn parse_project(path: &Path) -> Result<HashMap<String, ParsedPage>, String> {
    let mut reader = Reader::from_file(path).map_err(|e| format!("XMLを開けません: {e}"))?;
    reader.config_mut().trim_text(true);
    let mut buffer = Vec::new();
    let mut stack = Vec::<String>::new();
    let mut files = HashMap::<String, String>::new();
    let mut images = HashMap::<String, ImageInfo>::new();
    let mut page_to_image = HashMap::<String, String>::new();
    let mut current_image: Option<String> = None;
    let mut current_output_page: Option<String> = None;
    let mut current_output: Option<OutputInfo> = None;
    let mut outputs = Vec::<OutputInfo>::new();
    let mut in_output_image = false;
    let mut in_crop_area = false;

    loop {
        match reader
            .read_event_into(&mut buffer)
            .map_err(|e| format!("ScanTailor XML解析エラー: {e}"))?
        {
            Event::Start(event) => {
                let name = local_name(&event);
                let parent = stack.last().map(String::as_str);
                handle_element(
                    &event,
                    &name,
                    parent,
                    &mut files,
                    &mut images,
                    &mut page_to_image,
                    &mut current_image,
                    &mut current_output_page,
                    &mut current_output,
                    &mut in_output_image,
                    &mut in_crop_area,
                )?;
                stack.push(name);
            }
            Event::Empty(event) => {
                let name = local_name(&event);
                let parent = stack.last().map(String::as_str);
                handle_element(
                    &event,
                    &name,
                    parent,
                    &mut files,
                    &mut images,
                    &mut page_to_image,
                    &mut current_image,
                    &mut current_output_page,
                    &mut current_output,
                    &mut in_output_image,
                    &mut in_crop_area,
                )?;
            }
            Event::End(event) => {
                let name = event.local_name().as_ref().to_string();
                if name == "image" && in_output_image {
                    in_output_image = false;
                } else if name == "image" {
                    current_image = None;
                } else if name == "crop-area" {
                    in_crop_area = false;
                } else if name == "output-params"
                    && let Some(output) = current_output.take()
                {
                    outputs.push(output);
                } else if name == "page" && stack.iter().any(|item| item == "output") {
                    current_output_page = None;
                }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    let mut parsed = HashMap::new();
    for output in outputs {
        let image_id = page_to_image
            .get(&output.page_id)
            .ok_or_else(|| format!("page {}のimageIdがありません", output.page_id))?;
        let image = images
            .get(image_id)
            .ok_or_else(|| format!("image {image_id}がありません"))?;
        let filename = files
            .get(&image.file_id)
            .ok_or_else(|| format!("file {}がありません", image.file_id))?;
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("不正なファイル名です: {filename}"))?;
        parsed.insert(
            stem.to_string(),
            ParsedPage {
                source_width: image.width,
                source_height: image.height,
                width: output.width,
                height: output.height,
                restore_x: output.restore_x,
                restore_y: output.restore_y,
            },
        );
    }
    Ok(parsed)
}

#[allow(clippy::too_many_arguments)]
fn handle_element(
    event: &BytesStart<'_>,
    name: &str,
    parent: Option<&str>,
    files: &mut HashMap<String, String>,
    images: &mut HashMap<String, ImageInfo>,
    page_to_image: &mut HashMap<String, String>,
    current_image: &mut Option<String>,
    current_output_page: &mut Option<String>,
    current_output: &mut Option<OutputInfo>,
    in_output_image: &mut bool,
    in_crop_area: &mut bool,
) -> Result<(), String> {
    match (parent, name) {
        (Some("files"), "file") => {
            files.insert(attribute(event, "id")?, attribute(event, "name")?);
        }
        (Some("images"), "image") => {
            let id = attribute(event, "id")?;
            images.insert(
                id.clone(),
                ImageInfo {
                    file_id: attribute(event, "fileId")?,
                    width: 0,
                    height: 0,
                },
            );
            *current_image = Some(id);
        }
        (Some("image"), "size") if current_image.is_some() && !*in_output_image => {
            let id = current_image.as_ref().unwrap();
            let info = images.get_mut(id).unwrap();
            info.width = attribute(event, "width")?
                .parse()
                .map_err(|_| "widthが不正です")?;
            info.height = attribute(event, "height")?
                .parse()
                .map_err(|_| "heightが不正です")?;
        }
        (Some("pages"), "page") => {
            page_to_image.insert(attribute(event, "id")?, attribute(event, "imageId")?);
        }
        (Some("output"), "page") => {
            *current_output_page = Some(attribute(event, "id")?);
        }
        (Some("output-params"), "image") if current_output_page.is_some() => {
            *in_output_image = true;
            *current_output = Some(OutputInfo {
                page_id: current_output_page.as_ref().unwrap().clone(),
                width: 0,
                height: 0,
                restore_x: 0.0,
                restore_y: 0.0,
                crop_point_seen: false,
            });
        }
        (Some("image"), "size") if *in_output_image => {
            let output = current_output.as_mut().ok_or("output情報がありません")?;
            output.width = attribute(event, "width")?
                .parse()
                .map_err(|_| "widthが不正です")?;
            output.height = attribute(event, "height")?
                .parse()
                .map_err(|_| "heightが不正です")?;
        }
        (Some("image"), "crop-area") if *in_output_image => *in_crop_area = true,
        (Some("crop-area"), "point") if *in_crop_area => {
            let output = current_output.as_mut().ok_or("output情報がありません")?;
            if !output.crop_point_seen {
                let x: f32 = attribute(event, "x")?.parse().map_err(|_| "xが不正です")?;
                let y: f32 = attribute(event, "y")?.parse().map_err(|_| "yが不正です")?;
                output.restore_x = -x;
                output.restore_y = -y;
                output.crop_point_seen = true;
            }
        }
        _ => {}
    }
    Ok(())
}

fn attribute(event: &BytesStart<'_>, name: &str) -> Result<String, String> {
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute.map_err(|e| format!("XML属性エラー: {e}"))?;
        if attribute.key.local_name().as_ref() == name {
            return Ok(attribute.value.into_owned());
        }
    }
    Err(format!("XML属性{name}がありません"))
}

fn local_name(event: &BytesStart<'_>) -> String {
    event.local_name().as_ref().to_string()
}

fn find_output_image(directory: &Path, stem: &str) -> Option<PathBuf> {
    ["tif", "tiff", "png", "jpg", "jpeg"]
        .iter()
        .map(|extension| directory.join(format!("{stem}.{extension}")))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_deskew_omits_broken_cli_switch() {
        let mut auto = Command::new("scantailor-cli");
        configure_deskew(&mut auto, true);
        assert_eq!(auto.get_args().count(), 0);

        let mut disabled = Command::new("scantailor-cli");
        configure_deskew(&mut disabled, false);
        let args = disabled
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, ["--rotate=0", "--deskew=manual"]);
    }

    #[test]
    fn parses_experiment_project_when_available() {
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).join(
            ".work/book-scan/experiment/medium-16-pages-2026-08-28/projects/medium-16.ScanTailor",
        );
        if !project.is_file() {
            return;
        }
        let pages = parse_project(&project).unwrap();
        let page = pages.get("p0077").unwrap();
        assert_eq!((page.width, page.height), (1232, 1022));
        assert_eq!((page.source_width, page.source_height), (1654, 2339));
        assert_eq!((page.restore_x, page.restore_y), (194.0, 168.0));
    }
}
