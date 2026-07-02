use super::ImageProcessor;
use std::sync::atomic::{AtomicBool, Ordering};

static MAX_PERFORMANCE_MODE: AtomicBool = AtomicBool::new(false);

/// CPU コア数の約 70% のスレッド数を算出する
///
/// rayon のグローバルプールに設定することで、長時間処理中もシステムへの
/// 負荷を抑え、CPU 稼働を 70% 程度に収める。
pub fn calc_worker_threads() -> usize {
    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    if ImageProcessor::max_performance_mode() {
        return cpu_count;
    }
    // 70% に丸める（最低 1 スレッド）
    std::cmp::max(1, (cpu_count as f64 * 0.7).round() as usize)
}

/// rayon グローバルスレッドプールを CPU 数の 70% に初期化する
///
/// アプリ起動直後に一度呼べばよい。既に初期化済みの場合は無視される。
pub fn init_thread_pool() {
    let num_threads = calc_worker_threads();
    // エラー（二重初期化など）は無視して続行
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads)
        .build_global();
}

impl ImageProcessor {
    /// 最大性能モードが有効か判定する
    ///
    /// CLI オプションから設定されたプロセス内フラグを参照する。
    pub fn max_performance_mode() -> bool {
        MAX_PERFORMANCE_MODE.load(Ordering::Relaxed)
    }

    /// 最大性能モードを設定する
    pub fn set_max_performance_mode(enabled: bool) {
        MAX_PERFORMANCE_MODE.store(enabled, Ordering::Relaxed);
    }
}
