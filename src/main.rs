use img2pdf::{cli, gui, image_processor};
use std::env;
use std::time::Instant;

fn main() {
    // rayon スレッドプールを CPU 数の 70% に初期化
    image_processor::init_thread_pool();

    // 引数を収集（プログラム名を除く）
    let args: Vec<String> = env::args().skip(1).collect();

    if let Some(cli_args) = cli::parse_args(&args) {
        // CLI モード: 引数が揃っていれば ディスプレイなしで処理
        let start = Instant::now();
        let result = cli::run(&cli_args);
        let elapsed = start.elapsed();
        let elapsed_secs = elapsed.as_secs_f64();

        if result.success {
            eprintln!(
                "完了: {} 枚を \"{}\" に保存しました（エラー: {} 枚）",
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
        // GUI モード: 引数なし（または不足）のとき FLTK を起動
        gui::run();
    }
}
