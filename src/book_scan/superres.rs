use super::config::{BookScanConfig, SuperResolutionMode};
use super::image_ops::{convert_to_png, prepare_for_superres, save_lanczos};
use super::manifest::{BookScanStage, PageRecord};
use super::scheduler::assign_lpt;
use super::tools::{find_realesrgan, is_windows_executable, to_windows_path, windows_temp_bridge};
use rayon::prelude::*;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

pub fn process(
    config: &BookScanConfig,
    work_dir: &Path,
    pages: &mut [PageRecord],
) -> Result<(), String> {
    for page in pages.iter_mut().filter(|page| page.is_blank) {
        page.processed_path = None;
        page.stage = BookScanStage::Upscaled;
        page.error = None;
    }
    let inputs = prepare_inputs(config, work_dir, pages)?;
    match config.super_resolution {
        SuperResolutionMode::Off => {
            for (index, page) in pages
                .iter_mut()
                .enumerate()
                .filter(|(_, page)| !page.is_blank)
            {
                page.processed_path = inputs[index].clone();
                page.stage = BookScanStage::Upscaled;
            }
            Ok(())
        }
        SuperResolutionMode::Lanczos => process_lanczos(config, work_dir, pages, &inputs),
        SuperResolutionMode::Anime => process_anime(config, work_dir, pages, &inputs),
    }
}

fn prepare_inputs(
    config: &BookScanConfig,
    work_dir: &Path,
    pages: &[PageRecord],
) -> Result<Vec<Option<std::path::PathBuf>>, String> {
    let needs_preprocessing = config.color_normalization_enabled
        || config.ink_neutralization_enabled
        || config.pre_stroke_enabled;
    if !needs_preprocessing {
        return pages
            .iter()
            .map(|page| {
                if page.is_blank {
                    Ok(None)
                } else {
                    page.crop_path
                        .clone()
                        .map(Some)
                        .ok_or_else(|| format!("{}のcrop画像がありません", page.stem))
                }
            })
            .collect();
    }

    let output_dir = work_dir.join("preprocessed");
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("超解像前処理フォルダを作成できません: {e}"))?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.cpu_workers)
        .build()
        .map_err(|e| format!("前処理worker poolを作成できません: {e}"))?;
    let results = pool.install(|| {
        pages
            .par_iter()
            .map(|page| {
                if page.is_blank {
                    return Ok(None);
                }
                let input = page
                    .crop_path
                    .as_deref()
                    .ok_or_else(|| format!("{}のcrop画像がありません", page.stem))?;
                let output = output_dir.join(format!("{}.png", page.stem));
                let neutralize_ink = config.ink_neutralization_enabled
                    && !config.ink_neutralization_excluded(page.page_number);
                prepare_for_superres(input, &output, config, neutralize_ink).map(Some)
            })
            .collect::<Vec<_>>()
    });
    results.into_iter().collect()
}

fn process_lanczos(
    config: &BookScanConfig,
    work_dir: &Path,
    pages: &mut [PageRecord],
    prepared: &[Option<std::path::PathBuf>],
) -> Result<(), String> {
    let output_dir = work_dir.join("ai-output");
    fs::create_dir_all(&output_dir)
        .map_err(|e| format!("Lanczos出力フォルダを作成できません: {e}"))?;
    let inputs = pages
        .iter()
        .enumerate()
        .filter(|(_, page)| !page.is_blank)
        .map(|(index, page)| {
            prepared[index]
                .clone()
                .ok_or_else(|| format!("{}の前処理画像がありません", page.stem))
                .map(|path| (index, path))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.cpu_workers)
        .build()
        .map_err(|e| format!("CPU worker poolを作成できません: {e}"))?;
    let results = pool.install(|| {
        inputs
            .par_iter()
            .map(|(page_index, input)| {
                let output = output_dir.join(format!("{}.png", pages[*page_index].stem));
                save_lanczos(input, &output, config.superres_output_scale)?;
                Ok::<_, String>((*page_index, output))
            })
            .collect::<Vec<_>>()
    });
    for result in results {
        let (index, output) = result?;
        validate_dimensions(
            &output,
            pages[index].crop_width * config.superres_output_scale,
            pages[index].crop_height * config.superres_output_scale,
        )?;
        pages[index].processed_path = Some(output);
        pages[index].stage = BookScanStage::Upscaled;
    }
    Ok(())
}

fn process_anime(
    config: &BookScanConfig,
    work_dir: &Path,
    pages: &mut [PageRecord],
    prepared: &[Option<std::path::PathBuf>],
) -> Result<(), String> {
    let executable = find_realesrgan(config.realesrgan_path.as_deref()).ok_or_else(|| {
        "realesrgan-ncnn-vulkanが見つかりません。--realesrgan-pathまたはIMG2PDF_REALESRGANを指定してください"
            .to_string()
    })?;
    let ai_input = work_dir.join("ai-input");
    let ai_output = work_dir.join("ai-output");
    let logs = work_dir.join("logs");
    fs::create_dir_all(&ai_input).map_err(|e| format!("AI入力フォルダを作れません: {e}"))?;
    fs::create_dir_all(&ai_output).map_err(|e| format!("AI出力フォルダを作れません: {e}"))?;
    fs::create_dir_all(&logs).map_err(|e| format!("logフォルダを作れません: {e}"))?;

    let active_indices = pages
        .iter()
        .enumerate()
        .filter_map(|(index, page)| (!page.is_blank).then_some(index))
        .collect::<Vec<_>>();
    if active_indices.is_empty() {
        return Ok(());
    }

    let crop_paths = active_indices
        .iter()
        .map(|&index| {
            let page = &pages[index];
            prepared[index]
                .clone()
                .ok_or_else(|| format!("{}の前処理画像がありません", page.stem))
                .map(|path| (index, path))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.cpu_workers)
        .build()
        .map_err(|e| format!("CPU worker poolを作成できません: {e}"))?;
    let conversions = pool.install(|| {
        crop_paths
            .par_iter()
            .map(|(page_index, input)| {
                let output = ai_input.join(format!("{}.png", pages[*page_index].stem));
                convert_to_png(input, &output).map(|_| (*page_index, output))
            })
            .collect::<Vec<_>>()
    });
    let mut ai_inputs = vec![None; pages.len()];
    for conversion in conversions {
        let (page_index, path) = conversion?;
        ai_inputs[page_index] = Some(path);
    }

    let windows_bridge = is_windows_executable(&executable);
    let staging_root = if windows_bridge {
        windows_temp_bridge()?
    } else {
        work_dir.join("realesrgan-batches")
    };
    fs::create_dir_all(&staging_root)
        .map_err(|e| format!("Real-ESRGAN作業フォルダを作成できません: {e}"))?;
    let staging_guard = StagingGuard {
        path: staging_root.clone(),
        cleanup: windows_bridge || !config.keep_work,
    };

    let active_pages = active_indices
        .iter()
        .map(|&index| pages[index].clone())
        .collect::<Vec<_>>();
    let assignments = assign_lpt(&active_pages, config.gpu_workers)
        .into_iter()
        .map(|assignment| {
            assignment
                .into_iter()
                .map(|local_index| active_indices[local_index])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut worker_directories = Vec::new();
    for (worker, assignment) in assignments.iter().enumerate() {
        let input_dir = staging_root.join(format!("worker-{worker}/input"));
        let output_dir = staging_root.join(format!("worker-{worker}/output"));
        fs::create_dir_all(&input_dir)
            .and_then(|_| fs::create_dir_all(&output_dir))
            .map_err(|e| format!("Real-ESRGAN workerフォルダを作成できません: {e}"))?;
        for &page_index in assignment {
            fs::copy(
                ai_inputs[page_index]
                    .as_deref()
                    .ok_or_else(|| format!("{}のAI入力がありません", pages[page_index].stem))?,
                input_dir.join(format!("{}.png", pages[page_index].stem)),
            )
            .map_err(|e| format!("AI worker入力をコピーできません: {e}"))?;
        }
        worker_directories.push((input_dir, output_dir));
    }

    let model_dir = config
        .realesrgan_model_dir
        .clone()
        .or_else(|| executable.parent().map(|parent| parent.join("models")))
        .ok_or_else(|| "Real-ESRGAN model folderを特定できません".to_string())?;
    if !model_dir.is_dir() {
        return Err(format!(
            "Real-ESRGAN model folderがありません: {}",
            model_dir.display()
        ));
    }

    let results = std::thread::scope(|scope| {
        let handles = worker_directories
            .iter()
            .enumerate()
            .map(|(worker, (input, output))| {
                let executable = executable.clone();
                let model_dir = model_dir.clone();
                scope.spawn(move || {
                    run_batch(
                        &executable,
                        &model_dir,
                        input,
                        output,
                        config,
                        windows_bridge,
                    )
                    .map(|result| (worker, result))
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .map_err(|_| "Real-ESRGAN workerがpanicしました".to_string())?
            })
            .collect::<Result<Vec<_>, String>>()
    })?;
    for (worker, output) in results {
        let mut bytes = output.stdout;
        bytes.extend_from_slice(&output.stderr);
        fs::write(logs.join(format!("realesrgan-worker-{worker}.log")), bytes)
            .map_err(|e| format!("Real-ESRGAN logを保存できません: {e}"))?;
    }

    let mut invalid = Vec::new();
    let inference_scale = config.resolved_ai_scale();
    for (worker, assignment) in assignments.iter().enumerate() {
        for &page_index in assignment {
            let staged = worker_directories[worker]
                .1
                .join(format!("{}.png", pages[page_index].stem));
            let expected = (
                pages[page_index].crop_width * inference_scale,
                pages[page_index].crop_height * inference_scale,
            );
            if validate_dimensions(&staged, expected.0, expected.1).is_err() {
                invalid.push((worker, page_index));
            }
        }
    }

    // ncnn-vulkanは終了コード0でも欠落する場合があるため、失敗ページだけ1回再試行する。
    for (worker, page_index) in invalid {
        let input = worker_directories[worker]
            .0
            .join(format!("{}.png", pages[page_index].stem));
        let output = worker_directories[worker]
            .1
            .join(format!("{}.png", pages[page_index].stem));
        let result = run_single(
            &executable,
            &model_dir,
            &input,
            &output,
            config,
            windows_bridge,
        )?;
        fs::write(
            logs.join(format!("realesrgan-retry-{}.log", pages[page_index].stem)),
            [&result.stdout[..], &result.stderr[..]].concat(),
        )
        .map_err(|e| format!("Real-ESRGAN retry logを保存できません: {e}"))?;
        pages[page_index].attempts = pages[page_index].attempts.saturating_add(1);
    }

    for (worker, assignment) in assignments.iter().enumerate() {
        for &page_index in assignment {
            let staged = worker_directories[worker]
                .1
                .join(format!("{}.png", pages[page_index].stem));
            validate_dimensions(
                &staged,
                pages[page_index].crop_width * inference_scale,
                pages[page_index].crop_height * inference_scale,
            )?;
            let output = ai_output.join(format!("{}.png", pages[page_index].stem));
            if inference_scale == config.superres_output_scale {
                fs::copy(&staged, &output).map_err(|e| format!("AI出力を確定できません: {e}"))?;
            } else {
                let image = image::open(&staged)
                    .map_err(|e| format!("x{inference_scale} AI出力を開けません: {e}"))?;
                image
                    .resize_exact(
                        pages[page_index].crop_width * config.superres_output_scale,
                        pages[page_index].crop_height * config.superres_output_scale,
                        image::imageops::FilterType::Lanczos3,
                    )
                    .save(&output)
                    .map_err(|e| {
                        format!(
                            "AI出力を最終画像x{}へ縮小できません: {e}",
                            config.superres_output_scale
                        )
                    })?;
            }
            validate_dimensions(
                &output,
                pages[page_index].crop_width * config.superres_output_scale,
                pages[page_index].crop_height * config.superres_output_scale,
            )?;
            pages[page_index].processed_path = Some(output);
            pages[page_index].stage = BookScanStage::Upscaled;
            pages[page_index].error = None;
        }
    }

    drop(staging_guard);
    Ok(())
}

struct StagingGuard {
    path: std::path::PathBuf,
    cleanup: bool,
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.cleanup && self.path.is_dir() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn run_batch(
    executable: &Path,
    model_dir: &Path,
    input: &Path,
    output: &Path,
    config: &BookScanConfig,
    windows_bridge: bool,
) -> Result<Output, String> {
    run_command(executable, model_dir, input, output, config, windows_bridge)
}

fn run_single(
    executable: &Path,
    model_dir: &Path,
    input: &Path,
    output: &Path,
    config: &BookScanConfig,
    windows_bridge: bool,
) -> Result<Output, String> {
    if output.exists() {
        fs::remove_file(output).map_err(|e| format!("不正AI出力を除去できません: {e}"))?;
    }
    run_command(executable, model_dir, input, output, config, windows_bridge)
}

fn run_command(
    executable: &Path,
    model_dir: &Path,
    input: &Path,
    output: &Path,
    config: &BookScanConfig,
    windows_bridge: bool,
) -> Result<Output, String> {
    let path = |value: &Path| {
        if windows_bridge {
            to_windows_path(value)
        } else {
            Ok(value.to_string_lossy().to_string())
        }
    };
    let mut command = Command::new(executable);
    command
        .arg("-i")
        .arg(path(input)?)
        .arg("-o")
        .arg(path(output)?)
        .arg("-s")
        .arg(config.resolved_ai_scale().to_string())
        .arg("-n")
        .arg(&config.superres_model)
        .arg("-m")
        .arg(path(model_dir)?)
        .arg("-j")
        .arg("1:1:1")
        .arg("-t")
        .arg(config.tile_size.to_string())
        .arg("-f")
        .arg("png");
    if config.gpu_id >= 0 {
        command.arg("-g").arg(config.gpu_id.to_string());
    }
    if config.tta_enabled {
        command.arg("-x");
    }
    command
        .output()
        .map_err(|e| format!("Real-ESRGANを実行できません: {e}"))
}

fn validate_dimensions(path: &Path, width: u32, height: u32) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("出力がありません: {}", path.display()));
    }
    let actual = image::image_dimensions(path)
        .map_err(|e| format!("出力画像をデコードできません: {}: {e}", path.display()))?;
    if actual != (width, height) {
        return Err(format!(
            "出力寸法が不一致です: {} expected={}x{} actual={}x{}",
            path.display(),
            width,
            height,
            actual.0,
            actual.1
        ));
    }
    Ok(())
}
