use super::PageRecord;

/// Largest Processing Time first。ページのクロップ画素数をGPU負荷の近似値にする。
pub fn assign_lpt(pages: &[PageRecord], workers: usize) -> Vec<Vec<usize>> {
    let worker_count = workers.max(1).min(pages.len().max(1));
    let mut assignments = vec![Vec::new(); worker_count];
    let mut loads = vec![0u64; worker_count];
    let mut order = (0..pages.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| std::cmp::Reverse(pages[index].crop_pixels()));

    for page_index in order {
        let worker = loads
            .iter()
            .enumerate()
            .min_by_key(|(index, load)| (**load, *index))
            .map(|(index, _)| index)
            .unwrap_or(0);
        loads[worker] += pages[page_index].crop_pixels();
        assignments[worker].push(page_index);
    }

    for assignment in &mut assignments {
        assignment.sort_by_key(|&index| pages[index].index);
    }
    assignments
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book_scan::BookScanStage;
    use std::path::PathBuf;

    fn page(index: usize, pixels: u32) -> PageRecord {
        PageRecord {
            index,
            page_number: index + 1,
            stem: format!("p{:04}", index + 1),
            source_path: PathBuf::new(),
            source_width: pixels,
            source_height: 1,
            is_blank: false,
            blank_metrics: None,
            crop_path: None,
            crop_width: pixels,
            crop_height: 1,
            restore_x: 0.0,
            restore_y: 0.0,
            processed_path: None,
            jpeg_path: None,
            stage: BookScanStage::Extracted,
            attempts: 0,
            error: None,
        }
    }

    #[test]
    fn lpt_balances_large_pages_first() {
        let pages = vec![page(0, 10), page(1, 9), page(2, 8), page(3, 7)];
        let assignments = assign_lpt(&pages, 2);
        let loads = assignments
            .iter()
            .map(|indices| indices.iter().map(|&i| pages[i].crop_pixels()).sum::<u64>())
            .collect::<Vec<_>>();
        assert_eq!(loads, vec![17, 17]);
    }
}
