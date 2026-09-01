use crate::app_config::AppConfig;
use crate::book_scan::{
    BookScanConfig, BookScanProcessor, BookScanProgress, BookScanReport, JpegSampling, PageRange,
    PartialGrayscaleMode, SuperResolutionMode,
};
use fltk::{
    app,
    button::{Button, CheckButton},
    dialog,
    enums::{Align, Color, FrameType},
    frame::Frame,
    group::Scroll,
    input::{FloatInput, Input, IntInput},
    menu::Choice,
    prelude::*,
    window::Window,
};
use std::cell::RefCell;
use std::path::PathBuf;

thread_local! {
    static WINDOWS: RefCell<Vec<Window>> = const { RefCell::new(Vec::new()) };
}

pub fn show() {
    let saved = AppConfig::new()
        .ok()
        .and_then(|config| config.get_book_scan_config().cloned())
        .unwrap_or_default();

    let mut window = Window::new(120, 60, 860, 760, "本の自炊モード設定");
    window.set_color(Color::from_rgb(245, 245, 245));
    let mut scroll = Scroll::new(0, 0, 860, 760, None);
    scroll.set_scrollbar_size(18);

    section(10, 10, 820, 125, "入力・出力");
    label(25, 35, 125, "入力PDF/画像:");
    let mut input_path = Input::new(150, 35, 545, 26, None);
    input_path.set_value(&saved.input_path.to_string_lossy());
    let mut browse_pdf = Button::new(705, 35, 110, 26, "PDF選択");
    label(25, 68, 125, "出力PDF:");
    let mut output_path = Input::new(150, 68, 545, 26, None);
    output_path.set_value(&saved.output_pdf.to_string_lossy());
    let mut browse_output = Button::new(705, 68, 110, 26, "保存先");
    label(25, 101, 125, "ページ範囲:");
    let mut pages = Input::new(150, 101, 150, 26, None);
    if let Some(range) = saved.pages {
        pages.set_value(&format!("{}-{}", range.start, range.end));
    }
    let mut browse_folder = Button::new(315, 101, 150, 26, "画像フォルダ選択");

    section(10, 145, 820, 195, "ScanTailor 前処理");
    let scantailor_enabled = check(25, 170, "ScanTailor", saved.scantailor_enabled);
    let crop_enabled = check(180, 170, "自動crop", saved.crop_enabled);
    let normalize = check(330, 170, "背景正規化", saved.normalize_illumination);
    let deskew = check(500, 170, "傾き補正", saved.deskew_enabled);
    let dewarp = check(650, 170, "湾曲補正", saved.dewarp_enabled);

    label(25, 207, 80, "余白:");
    let mut margins = FloatInput::new(105, 207, 90, 26, None);
    margins.set_value(&saved.margins.to_string());
    label(215, 207, 80, "入力DPI:");
    let mut dpi = IntInput::new(295, 207, 90, 26, None);
    dpi.set_value(&saved.dpi.to_string());
    label(405, 207, 90, "出力DPI:");
    let mut output_dpi = IntInput::new(495, 207, 90, 26, None);
    output_dpi.set_value(&saved.output_dpi.to_string());
    label(605, 207, 90, "despeckle:");
    let mut despeckle = FloatInput::new(695, 207, 90, 26, None);
    despeckle.set_value(&saved.despeckle.to_string());

    label(25, 244, 165, "ページ検出 tolerance:");
    let mut tolerance = FloatInput::new(190, 244, 90, 26, None);
    tolerance.set_value(&saved.page_detection_tolerance.to_string());
    label(305, 244, 155, "scantailor-cli:");
    let mut scantailor_path = Input::new(460, 244, 325, 26, None);
    if let Some(path) = &saved.scantailor_path {
        scantailor_path.set_value(&path.to_string_lossy());
    }
    let color_normalize = check(25, 281, "カラー紙面補正", saved.color_normalization_enabled);
    label(180, 281, 65, "強さ:");
    let mut color_normalize_strength = IntInput::new(245, 281, 65, 26, None);
    color_normalize_strength.set_value(&saved.color_normalization_strength.to_string());
    label(330, 281, 65, "半径:");
    let mut color_normalize_radius = IntInput::new(395, 281, 65, 26, None);
    color_normalize_radius.set_value(&saved.color_normalization_radius.to_string());
    label(480, 281, 105, "crop除外:");
    let mut crop_exclude_pages = Input::new(585, 281, 200, 26, None);
    crop_exclude_pages.set_value(&PageRange::format_list(&saved.crop_exclude_pages));
    let ink_neutralize = check(
        25,
        318,
        "黒インク色差補正",
        saved.ink_neutralization_enabled,
    );
    label(180, 318, 65, "強さ:");
    let mut ink_neutralize_strength = IntInput::new(245, 318, 65, 26, None);
    ink_neutralize_strength.set_value(&saved.ink_neutralization_strength.to_string());
    label(330, 318, 180, "補正除外ページ:");
    let mut ink_neutralize_exclude_pages = Input::new(510, 318, 275, 26, None);
    ink_neutralize_exclude_pages.set_value(&PageRange::format_list(
        &saved.ink_neutralization_exclude_pages,
    ));

    section(10, 350, 820, 235, "超解像");
    label(25, 375, 80, "方式:");
    let mut superres = Choice::new(105, 375, 145, 26, None);
    superres.add_choice("off|anime|lanczos");
    superres.set_value(match saved.super_resolution {
        SuperResolutionMode::Off => 0,
        SuperResolutionMode::Anime => 1,
        SuperResolutionMode::Lanczos => 2,
    });
    label(265, 375, 100, "最終倍率:");
    let mut output_scale = IntInput::new(365, 375, 55, 26, None);
    output_scale.set_value(&saved.superres_output_scale.to_string());
    label(430, 375, 115, "GPU workers:");
    let mut gpu_workers = IntInput::new(545, 375, 70, 26, None);
    gpu_workers.set_value(&saved.gpu_workers.to_string());
    label(640, 375, 105, "CPU workers:");
    let mut cpu_workers = IntInput::new(745, 375, 55, 26, None);
    cpu_workers.set_value(&saved.cpu_workers.to_string());

    label(25, 412, 80, "model:");
    let mut model = Input::new(105, 412, 300, 26, None);
    model.set_value(&saved.superres_model);
    label(430, 412, 60, "tile:");
    let mut tile = IntInput::new(490, 412, 70, 26, None);
    tile.set_value(&saved.tile_size.to_string());
    label(580, 412, 70, "GPU ID:");
    let mut gpu_id = IntInput::new(650, 412, 70, 26, None);
    gpu_id.set_value(&saved.gpu_id.to_string());
    let tta = check(735, 412, "TTA", saved.tta_enabled);

    label(25, 449, 155, "Real-ESRGAN:");
    let mut realesrgan_path = Input::new(180, 449, 605, 26, None);
    if let Some(path) = &saved.realesrgan_path {
        realesrgan_path.set_value(&path.to_string_lossy());
    }
    label(25, 486, 155, "model directory:");
    let mut model_dir = Input::new(180, 486, 605, 26, None);
    if let Some(path) = &saved.realesrgan_model_dir {
        model_dir.set_value(&path.to_string_lossy());
    }
    let pre_stroke = check(25, 523, "超解像前の線補強", saved.pre_stroke_enabled);
    label(205, 523, 65, "強さ:");
    let mut pre_stroke_strength = IntInput::new(270, 523, 70, 26, None);
    pre_stroke_strength.set_value(&saved.pre_stroke_strength.to_string());
    label(380, 523, 105, "AI内部倍率:");
    let mut ai_scale = IntInput::new(485, 523, 70, 26, None);
    ai_scale.set_value(&saved.superres_ai_scale.to_string());

    section(10, 595, 820, 255, "文字・階調・JPEG・再開");
    let stroke_enabled = check(25, 620, "文字太さ調整", saved.stroke_enabled);
    label(195, 620, 80, "強さ:");
    let mut stroke_strength = IntInput::new(275, 620, 75, 26, None);
    stroke_strength.set_value(&saved.stroke_strength.to_string());
    label(380, 620, 100, "JPEG品質:");
    let mut jpeg_quality = IntInput::new(480, 620, 75, 26, None);
    jpeg_quality.set_value(&saved.jpeg_quality.to_string());
    label(580, 620, 90, "sampling:");
    let mut sampling = Choice::new(670, 620, 115, 26, None);
    sampling.add_choice("4:4:4|4:2:2|4:2:0");
    sampling.set_value(match saved.jpeg_sampling {
        JpegSampling::S444 => 0,
        JpegSampling::S422 => 1,
        JpegSampling::S420 => 2,
    });
    let tone_boost = check(25, 657, "tone boost", saved.tone_boost_enabled);
    label(195, 657, 80, "強さ:");
    let mut tone_boost_strength = IntInput::new(275, 657, 75, 26, None);
    tone_boost_strength.set_value(&saved.tone_boost_strength.to_string());
    label(
        380,
        657,
        405,
        "暗部の自動gray: 無効（全体grayはページ指定）",
    );

    label(25, 694, 145, "全体カラー閾値:");
    let mut tone_global_threshold = FloatInput::new(170, 694, 80, 26, None);
    tone_global_threshold.set_value(&saved.tone_color_global_threshold.to_string());
    label(275, 694, 145, "局所カラー閾値:");
    let mut tone_tile_threshold = FloatInput::new(420, 694, 80, 26, None);
    tone_tile_threshold.set_value(&saved.tone_color_tile_threshold.to_string());
    label(525, 694, 110, "補正除外:");
    let mut tone_exclude_pages = Input::new(635, 694, 150, 26, None);
    tone_exclude_pages.set_value(&PageRange::format_list(&saved.tone_exclude_pages));

    label(25, 735, 185, "全体gray指定ページ:");
    let mut grayscale_pages = Input::new(210, 735, 250, 26, None);
    grayscale_pages.set_value(&PageRange::format_list(&saved.grayscale_pages));
    label(480, 735, 305, "空欄なら無効（AUTOなし）");

    let resume = check(25, 772, "中断再開", saved.resume);
    let keep_work = check(180, 772, "中間画像を保持", saved.keep_work);
    let preserve_position = check(335, 772, "元位置・サイズを保持", saved.preserve_position);
    label(25, 809, 100, "作業folder:");
    let mut work_dir = Input::new(125, 809, 660, 26, None);
    if let Some(path) = &saved.work_dir {
        work_dir.set_value(&path.to_string_lossy());
    }

    section(10, 860, 820, 130, "空白ページ判定");
    let blank_detection = check(25, 885, "空白判定", saved.blank_detection_enabled);
    label(180, 885, 95, "dark差:");
    let mut blank_dark_delta = IntInput::new(275, 885, 70, 26, None);
    blank_dark_delta.set_value(&saved.blank_dark_delta.to_string());
    label(370, 885, 115, "最大dark比:");
    let mut blank_max_dark = FloatInput::new(485, 885, 100, 26, None);
    blank_max_dark.set_value(&saved.blank_max_dark_ratio.to_string());
    label(25, 922, 145, "edge輝度差:");
    let mut blank_edge_threshold = IntInput::new(170, 922, 70, 26, None);
    blank_edge_threshold.set_value(&saved.blank_edge_threshold.to_string());
    label(275, 922, 145, "最大edge比:");
    let mut blank_max_edge = FloatInput::new(420, 922, 100, 26, None);
    blank_max_edge.set_value(&saved.blank_max_edge_ratio.to_string());

    let mut run_button = Button::new(285, 1010, 290, 38, "本モードを実行");
    run_button.set_color(Color::from_rgb(72, 125, 90));
    run_button.set_label_color(Color::White);
    let mut status = Frame::new(25, 1060, 775, 32, "待機中");
    status.set_align(Align::Center | Align::Inside);
    status.set_frame(FrameType::DownBox);

    scroll.end();
    window.end();
    window.make_resizable(true);

    {
        let mut input = input_path.clone();
        browse_pdf.set_callback(move |_| {
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseFile);
            chooser.set_title("入力PDFを選択");
            chooser.set_filter("PDF Files\t*.pdf");
            chooser.show();
            let path = chooser.filename();
            if !path.as_os_str().is_empty() {
                input.set_value(&path.to_string_lossy());
            }
        });
    }
    {
        let mut input = input_path.clone();
        browse_folder.set_callback(move |_| {
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseDir);
            chooser.set_title("入力画像フォルダを選択");
            chooser.show();
            let path = chooser.filename();
            if !path.as_os_str().is_empty() {
                input.set_value(&path.to_string_lossy());
            }
        });
    }
    {
        let mut output = output_path.clone();
        browse_output.set_callback(move |_| {
            let mut chooser =
                dialog::NativeFileChooser::new(dialog::NativeFileChooserType::BrowseSaveFile);
            chooser.set_title("出力PDFを選択");
            chooser.set_filter("PDF Files\t*.pdf");
            chooser.show();
            let path = chooser.filename();
            if !path.as_os_str().is_empty() {
                let mut value = path.to_string_lossy().to_string();
                if !value.to_ascii_lowercase().ends_with(".pdf") {
                    value.push_str(".pdf");
                }
                output.set_value(&value);
            }
        });
    }

    #[derive(Clone)]
    enum DialogMessage {
        Progress(BookScanProgress),
        Finished(Result<BookScanReport, String>),
    }
    let (result_sender, result_receiver) = app::channel::<DialogMessage>();
    let mut idle_button = run_button.clone();
    let mut idle_status = status.clone();
    app::add_idle3(move |_| {
        if let Some(message) = result_receiver.recv() {
            match message {
                DialogMessage::Progress(progress) => {
                    idle_status.set_label(&format!(
                        "[{}/{}] {}",
                        progress.stage, progress.total_stages, progress.message
                    ));
                }
                DialogMessage::Finished(result) => {
                    idle_button.activate();
                    match result {
                        Ok(report) => {
                            idle_status.set_label(&format!(
                                "完了: {}ページ（空白{}）/ {:.2}秒",
                                report.page_count,
                                report.blank_page_count,
                                report.elapsed.as_secs_f64()
                            ));
                            dialog::message_default(&format!(
                                "本モードPDFを保存しました。\n{}",
                                report.output_pdf.display()
                            ));
                        }
                        Err(error) => {
                            idle_status.set_label("エラー");
                            dialog::alert_default(&format!(
                                "本モード処理に失敗しました。\n\n{error}"
                            ));
                        }
                    }
                }
            }
        }
    });

    run_button.set_callback(move |button| {
        let result = (|| -> Result<BookScanConfig, String> {
            let input = input_path.value();
            let output = output_path.value();
            if input.trim().is_empty() || output.trim().is_empty() {
                return Err("入力と出力PDFを指定してください".to_string());
            }
            let scan_enabled = scantailor_enabled.value();
            let mut config = BookScanConfig {
                input_path: PathBuf::from(input.trim()),
                output_pdf: PathBuf::from(output.trim()),
                work_dir: optional_path(&work_dir.value()),
                pages: if pages.value().trim().is_empty() {
                    None
                } else {
                    Some(PageRange::parse(&pages.value()).ok_or("ページ範囲が不正です")?)
                },
                blank_detection_enabled: blank_detection.value(),
                blank_dark_delta: parse(&blank_dark_delta.value(), "空白判定dark差")?,
                blank_max_dark_ratio: parse(&blank_max_dark.value(), "空白判定最大dark比")?,
                blank_edge_threshold: parse(&blank_edge_threshold.value(), "空白判定edge輝度差")?,
                blank_max_edge_ratio: parse(&blank_max_edge.value(), "空白判定最大edge比")?,
                scantailor_enabled: scan_enabled,
                crop_enabled: scan_enabled && crop_enabled.value(),
                crop_exclude_pages: PageRange::parse_list(&crop_exclude_pages.value())
                    .ok_or("crop除外ページが不正です")?,
                normalize_illumination: scan_enabled && normalize.value(),
                color_normalization_enabled: color_normalize.value(),
                color_normalization_strength: parse(
                    &color_normalize_strength.value(),
                    "カラー紙面補正の強さ",
                )?,
                color_normalization_radius: parse(
                    &color_normalize_radius.value(),
                    "カラー紙面補正の半径",
                )?,
                ink_neutralization_enabled: ink_neutralize.value(),
                ink_neutralization_strength: parse(
                    &ink_neutralize_strength.value(),
                    "黒インク色差補正の強さ",
                )?,
                ink_neutralization_exclude_pages: PageRange::parse_list(
                    &ink_neutralize_exclude_pages.value(),
                )
                .ok_or("黒インク色差補正の除外ページが不正です")?,
                deskew_enabled: scan_enabled && deskew.value(),
                dewarp_enabled: scan_enabled && dewarp.value(),
                margins: parse(&margins.value(), "余白")?,
                dpi: parse(&dpi.value(), "入力DPI")?,
                output_dpi: parse(&output_dpi.value(), "出力DPI")?,
                despeckle: parse(&despeckle.value(), "despeckle")?,
                page_detection_tolerance: parse(&tolerance.value(), "ページ検出tolerance")?,
                super_resolution: match superres.value() {
                    0 => SuperResolutionMode::Off,
                    2 => SuperResolutionMode::Lanczos,
                    _ => SuperResolutionMode::Anime,
                },
                superres_output_scale: parse(&output_scale.value(), "最終画像倍率")?,
                superres_ai_scale: parse(&ai_scale.value(), "AI内部倍率")?,
                superres_model: model.value(),
                gpu_workers: parse(&gpu_workers.value(), "GPU worker数")?,
                cpu_workers: parse(&cpu_workers.value(), "CPU worker数")?,
                tile_size: parse(&tile.value(), "tile size")?,
                gpu_id: parse(&gpu_id.value(), "GPU ID")?,
                tta_enabled: tta.value(),
                stroke_enabled: stroke_enabled.value(),
                stroke_strength: parse(&stroke_strength.value(), "文字太さ")?,
                tone_boost_enabled: tone_boost.value(),
                tone_boost_strength: parse(&tone_boost_strength.value(), "tone boostの強さ")?,
                partial_grayscale: PartialGrayscaleMode::Off,
                partial_grayscale_strength: 100,
                tone_color_global_threshold: parse(
                    &tone_global_threshold.value(),
                    "全体カラー閾値",
                )?,
                tone_color_tile_threshold: parse(&tone_tile_threshold.value(), "局所カラー閾値")?,
                tone_exclude_pages: PageRange::parse_list(&tone_exclude_pages.value())
                    .ok_or("tone補正の除外ページが不正です")?,
                grayscale_pages: PageRange::parse_list(&grayscale_pages.value())
                    .ok_or("全体gray指定ページが不正です")?,
                pre_stroke_enabled: pre_stroke.value(),
                pre_stroke_strength: parse(&pre_stroke_strength.value(), "超解像前の線補強")?,
                jpeg_quality: parse(&jpeg_quality.value(), "JPEG品質")?,
                jpeg_sampling: match sampling.value() {
                    1 => JpegSampling::S422,
                    2 => JpegSampling::S420,
                    _ => JpegSampling::S444,
                },
                preserve_position: preserve_position.value(),
                scantailor_path: optional_path(&scantailor_path.value()),
                realesrgan_path: optional_path(&realesrgan_path.value()),
                realesrgan_model_dir: optional_path(&model_dir.value()),
                resume: resume.value(),
                keep_work: keep_work.value(),
            };
            if config.super_resolution == SuperResolutionMode::Off {
                config.superres_output_scale = 1;
            }
            config.validate()?;
            Ok(config)
        })();

        let config = match result {
            Ok(config) => config,
            Err(error) => {
                dialog::alert_default(&error);
                return;
            }
        };
        if let Ok(mut saved_config) = AppConfig::new() {
            saved_config.set_book_scan_config(config.clone());
            saved_config.save();
        }
        button.deactivate();
        status.set_label("処理中... 詳細はconsole logを確認してください");
        let sender = result_sender;
        std::thread::spawn(move || {
            let progress_sender = sender;
            let result = BookScanProcessor::run_with_progress(config, move |progress| {
                progress_sender.send(DialogMessage::Progress(progress));
            });
            sender.send(DialogMessage::Finished(result));
        });
    });

    window.show();
    WINDOWS.with(|windows| windows.borrow_mut().push(window));
}

fn section(x: i32, y: i32, width: i32, height: i32, title: &str) {
    let mut frame = Frame::new(x, y, width, height, title);
    frame.set_align(Align::TopLeft | Align::Inside);
    frame.set_frame(FrameType::EngravedBox);
}

fn label(x: i32, y: i32, width: i32, text: &str) {
    let mut frame = Frame::new(x, y, width, 26, text);
    frame.set_align(Align::Left | Align::Inside);
}

fn check(x: i32, y: i32, text: &str, value: bool) -> CheckButton {
    let mut button = CheckButton::new(x, y, 145, 26, text);
    button.set_value(value);
    button
}

fn optional_path(value: &str) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()))
}

fn parse<T>(value: &str, label: &str) -> Result<T, String>
where
    T: std::str::FromStr,
{
    value
        .trim()
        .parse()
        .map_err(|_| format!("{label}の値が不正です: {value}"))
}
