use img2pdf::{cli, gui, image_processor};
use std::env;
use std::time::Instant;

fn main() {
    let mut raw_args = env::args();
    let program_name = raw_args.next().unwrap_or_else(|| "img2pdf".to_string());
    let mut args: Vec<String> = raw_args.collect();
    let invoked_as_pdf2img = std::path::Path::new(&program_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("pdf2img"))
        .unwrap_or(false);

    if invoked_as_pdf2img && args.first().map(|a| a.as_str()) != Some("pdf2img") {
        args.insert(0, "pdf2img".to_string());
    }

    if let Some(cli_args) = cli::parse_args(&args) {
        image_processor::ImageProcessor::set_max_performance_mode(cli_args.max_performance);
        // rayon スレッドプールは CLI 設定を反映した後に初期化する
        image_processor::init_thread_pool();

        // CLI モード: 引数が揃っていれば ディスプレイなしで処理
        let start = Instant::now();
        let result = cli::run(&cli_args);
        let elapsed = start.elapsed();
        let elapsed_secs = elapsed.as_secs_f64();

        if result.success {
            eprintln!(
                "完了: {} 件を \"{}\" に保存しました（エラー: {} 件）",
                result.success_count, result.output_path, result.error_count
            );
            eprintln!("実行時間: {:.3} 秒", elapsed_secs);
            std::process::exit(0);
        } else {
            eprintln!("処理失敗:");
            for e in &result.errors {
                eprintln!("  {}: {}", e.file_path, e.message);
            }
            eprintln!("実行時間: {:.3} 秒", elapsed_secs);
            std::process::exit(1);
        }
    } else {
        // GUI モード: スレッドプールの初期化はユーザーの性能設定確定後に
        // GUI の実行コールバック内で行うため、ここでは初期化しない
        gui::run();
    }
}
