//! PDF 导出的异步调度层，与 `pandoc_export` 同一套不变量：
//! 1. 后台线程完成后必须 `request_repaint()`，否则 egui 不会主动跑下一帧；
//! 2. 连续触发时旧任务靠代际号作废，迟到的结果不能覆盖新任务。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

type Outcome = Result<PathBuf, String>;

pub(super) struct PdfExport {
    generation: Arc<AtomicU64>,
    rx: Option<Receiver<(u64, Outcome)>>,
    busy: bool,
}

impl PdfExport {
    pub(super) fn new() -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            rx: None,
            busy: false,
        }
    }

    pub(super) fn is_busy(&self) -> bool {
        self.busy
    }

    pub(super) fn start(&mut self, request: crate::pdf::PdfRequest, ctx: &egui::Context) {
        let generation_id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;

        let token = Arc::clone(&self.generation);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let cancelled = || token.load(Ordering::SeqCst) != generation_id;
            let result = crate::pdf::convert(request, cancelled);
            if cancelled() {
                return;
            }
            let _ = tx.send((generation_id, result));
            ctx2.request_repaint();
        });
    }

    pub(super) fn poll(&mut self, ctx: &egui::Context) -> Option<Outcome> {
        let rx = self.rx.take()?;
        match rx.try_recv() {
            Ok((generation_id, result)) => {
                self.busy = false;
                if generation_id == self.generation.load(Ordering::SeqCst) {
                    Some(result)
                } else {
                    None
                }
            }
            Err(TryRecvError::Empty) => {
                self.rx = Some(rx);
                ctx.request_repaint_after(Duration::from_millis(200));
                None
            }
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_ignores_stale_generation() {
        let mut job = PdfExport::new();
        let ctx = egui::Context::default();
        job.generation.fetch_add(1, Ordering::SeqCst);
        job.busy = true;
        let (tx, rx) = mpsc::channel();
        tx.send((0, Ok(PathBuf::from("stale.pdf")))).unwrap();
        job.rx = Some(rx);
        assert!(job.poll(&ctx).is_none());
        assert!(!job.is_busy());
    }

    #[test]
    fn poll_accepts_current_generation() {
        let mut job = PdfExport::new();
        let ctx = egui::Context::default();
        job.generation.fetch_add(1, Ordering::SeqCst);
        job.busy = true;
        let (tx, rx) = mpsc::channel();
        tx.send((1, Ok(PathBuf::from("fresh.pdf")))).unwrap();
        job.rx = Some(rx);
        match job.poll(&ctx) {
            Some(Ok(p)) => assert_eq!(p, PathBuf::from("fresh.pdf")),
            _ => panic!("当前代际的结果应当被采纳"),
        }
        assert!(!job.is_busy());
    }
}
