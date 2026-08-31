use std::sync::atomic::{AtomicUsize, Ordering};

pub type ProgressCallback<'a> = dyn Fn(&str, usize, usize) + Sync + 'a;

/// 並列処理から低コストで進捗を通知するカウンタ。
///
/// 20ページまでは1ページごと、それ以上は最大約20回に間引く。
pub struct ProgressCounter<'a> {
    phase: &'a str,
    total: usize,
    completed: AtomicUsize,
    report_step: usize,
    callback: &'a ProgressCallback<'a>,
}

impl<'a> ProgressCounter<'a> {
    pub fn new(phase: &'a str, total: usize, callback: &'a ProgressCallback<'a>) -> Self {
        Self {
            phase,
            total,
            completed: AtomicUsize::new(0),
            report_step: total.div_ceil(20).max(1),
            callback,
        }
    }

    pub fn advance(&self) {
        let completed = self.completed.fetch_add(1, Ordering::Relaxed) + 1;
        if completed == 1 || completed == self.total || completed.is_multiple_of(self.report_step) {
            (self.callback)(self.phase, completed, self.total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn reports_every_item_for_small_jobs() {
        let reports = Mutex::new(Vec::new());
        let callback = |_: &str, done, _| reports.lock().unwrap().push(done);
        let counter = ProgressCounter::new("test", 3, &callback);
        counter.advance();
        counter.advance();
        counter.advance();
        assert_eq!(*reports.lock().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn throttles_large_jobs() {
        let reports = Mutex::new(Vec::new());
        let callback = |_: &str, done, _| reports.lock().unwrap().push(done);
        let counter = ProgressCounter::new("test", 1_000, &callback);
        for _ in 0..1_000 {
            counter.advance();
        }
        let reports = reports.lock().unwrap();
        assert_eq!(reports.first(), Some(&1));
        assert_eq!(reports.last(), Some(&1_000));
        assert!(reports.len() <= 21);
    }
}
