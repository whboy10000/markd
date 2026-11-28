//! Pandoc 导出的异步调度层。
//!
//! 核心不是「起个线程」这么简单，而是两条不变量：
//! 1. 后台线程完成后必须 `request_repaint()`——egui 是即时模式，没事做时不会
//!    主动跑下一帧，`try_recv` 也就永远等不到结果；
//! 2. 连续触发时旧任务必须作废——否则先启动但后完成的 pandoc 会把新任务
//!    写好的文件覆盖掉，UI 上的状态也会错乱。这里用单调递增的「代际号」
//!    实现：线程记住启动时的号，发现当前号变了就自行退出，不写盘、不回传。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

type Outcome = Result<PathBuf, String>;

pub(super) struct PandocExport {
    /// 当前代际。每次 start 自增；线程拿旧号比对，不一致即作废。
    generation: Arc<AtomicU64>,
    rx: Option<Receiver<(u64, Outcome)>>,
    busy: bool,
}

impl PandocExport {
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

    /// 启动一次转换。重复调用会让上一次任务在下一个检查点自行退出。
    pub(super) fn start(&mut self, request: crate::pandoc::PandocRequest, ctx: &egui::Context) {
        let generation_id = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = mpsc::channel();
        // 旧任务的结果通道直接丢弃：它的线程发现代际变化后不会再发消息。
        self.rx = Some(rx);
        self.busy = true;

        let token = Arc::clone(&self.generation);
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let cancelled = || token.load(Ordering::SeqCst) != generation_id;
            let result = crate::pandoc::convert(request, cancelled);
            if cancelled() {
                return;
            }
            let _ = tx.send((generation_id, result));
            ctx2.request_repaint();
        });
    }

    /// 每帧非阻塞地收一次结果。只认当前代际，旧任务迟到的消息直接丢弃。
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
                // 兜底唤醒：即便线程那句 request_repaint 因窗口失焦被吞，
                // 200ms 内也一定会再查一次。
                ctx.request_repaint_after(Duration::from_millis(200));
                None
            }
            Err(TryRecvError::Disconnected) => {
                // 线程 panic 了也要把 busy 摘掉，否则状态栏永远转圈。
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
        let mut job = PandocExport::new();
        let ctx = egui::Context::default();
        job.generation.fetch_add(1, Ordering::SeqCst); // 当前代际 = 1
        job.busy = true;
        let (tx, rx) = mpsc::channel();
        tx.send((0, Ok(PathBuf::from("stale.html")))).unwrap();
        job.rx = Some(rx);
        assert!(job.poll(&ctx).is_none());
        assert!(!job.is_busy());
    }

    #[test]
    fn poll_accepts_current_generation() {
        let mut job = PandocExport::new();
        let ctx = egui::Context::default();
        job.generation.fetch_add(1, Ordering::SeqCst);
        job.busy = true;
        let (tx, rx) = mpsc::channel();
        tx.send((1, Ok(PathBuf::from("fresh.html")))).unwrap();
        job.rx = Some(rx);
        match job.poll(&ctx) {
            Some(Ok(p)) => assert_eq!(p, PathBuf::from("fresh.html")),
            _ => panic!("当前代际的结果应当被采纳"),
        }
        assert!(!job.is_busy());
    }
}
