//! 终端布局树：分屏与页签。
//!
//! 一棵 `Layout` 描述终端面板主体区域怎么被切成若干「窗格」(`Pane`)：
//! - `Pane` 是一组页签（多个 shell），同一时刻只显示 active 的那个；
//! - `Split` 把一个区域按方向切成两块，可以任意嵌套，于是能「继续再分屏」。
//!
//! 所有 shell 都存在上层 `Terminal::sessions` 里，布局树只持有它们的 id。

/// 分屏方向。`Horizontal` = 左右排列（中间竖分隔线）；`Vertical` = 上下排列。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SplitDir {
    Horizontal,
    Vertical,
}

/// 一个窗格：一组页签 + 当前显示哪个。
#[derive(Clone, Debug)]
pub(super) struct Pane {
    pub(super) tabs: Vec<u64>,
    pub(super) active: u64,
}

/// 终端布局树。
#[derive(Clone, Debug)]
pub(super) enum Layout {
    Pane(Pane),
    Split {
        dir: SplitDir,
        /// 第一块（左/上）占的比例，0.15..=0.85。
        ratio: f32,
        a: Box<Layout>,
        b: Box<Layout>,
    },
}

impl Layout {
    pub(super) fn new(session_id: u64) -> Self {
        Self::Pane(Pane {
            tabs: vec![session_id],
            active: session_id,
        })
    }

    fn is_empty(&self) -> bool {
        matches!(self, Self::Pane(p) if p.tabs.is_empty())
    }

    fn contains(&self, id: u64) -> bool {
        match self {
            Self::Pane(p) => p.tabs.contains(&id),
            Self::Split { a, b, .. } => a.contains(id) || b.contains(id),
        }
    }

    /// 当前 active session 所在的窗格。
    pub(super) fn active_pane(&self, active: u64) -> Option<&Pane> {
        match self {
            Self::Pane(p) if p.tabs.contains(&active) => Some(p),
            Self::Split { a, b, .. } => a.active_pane(active).or_else(|| b.active_pane(active)),
            _ => None,
        }
    }

    pub(super) fn active_pane_mut(&mut self, active: u64) -> Option<&mut Pane> {
        match self {
            Self::Pane(p) if p.tabs.contains(&active) => Some(p),
            Self::Split { a, b, .. } => {
                if a.contains(active) {
                    a.active_pane_mut(active)
                } else {
                    b.active_pane_mut(active)
                }
            }
            _ => None,
        }
    }

    /// 把 active 所在的窗格一分为二，新窗格里放 `new_id` 这个会话，并成为 active。
    pub(super) fn split(&mut self, active: u64, dir: SplitDir, new_id: u64) -> bool {
        match self {
            Self::Pane(p) if p.active == active => {
                let original = std::mem::replace(
                    p,
                    Pane {
                        tabs: vec![],
                        active: 0,
                    },
                );
                *self = Self::Split {
                    dir,
                    ratio: 0.5,
                    a: Box::new(Self::Pane(original)),
                    b: Box::new(Self::new(new_id)),
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { a, b, .. } => {
                if a.contains(active) {
                    a.split(active, dir, new_id)
                } else {
                    b.split(active, dir, new_id)
                }
            }
        }
    }

    /// 从树里移除一个会话：若它是所在窗格的最后一个页签，整个窗格一并删除，
    /// 由兄弟子树顶替它的位置（保持布局紧凑）。返回是否找到并移除。
    pub(super) fn remove(&mut self, id: u64) -> bool {
        match self {
            Self::Pane(p) => {
                let before = p.tabs.len();
                p.tabs.retain(|t| *t != id);
                if p.active == id || !p.tabs.contains(&p.active) {
                    p.active = p.tabs.first().copied().unwrap_or(0);
                }
                before != p.tabs.len()
            }
            Self::Split { a, b, .. } => {
                let removed = if a.contains(id) {
                    let ok = a.remove(id);
                    if a.is_empty() {
                        let spare = Box::new(Self::Pane(Pane {
                            tabs: vec![],
                            active: 0,
                        }));
                        let replacement = std::mem::replace(b, spare);
                        *self = *replacement;
                    }
                    ok
                } else if b.contains(id) {
                    let ok = b.remove(id);
                    if b.is_empty() {
                        let spare = Box::new(Self::Pane(Pane {
                            tabs: vec![],
                            active: 0,
                        }));
                        let replacement = std::mem::replace(a, spare);
                        *self = *replacement;
                    }
                    ok
                } else {
                    false
                };
                removed
            }
        }
    }

    #[cfg(test)]
    pub(super) fn pane_count(&self) -> usize {
        match self {
            Self::Pane(p) => usize::from(!p.tabs.is_empty()),
            Self::Split { a, b, .. } => a.pane_count() + b.pane_count(),
        }
    }

    pub(super) fn session_count(&self) -> usize {
        match self {
            Self::Pane(p) => p.tabs.len(),
            Self::Split { a, b, .. } => a.session_count() + b.session_count(),
        }
    }

    /// 任一窗格里第一个页签，用于关闭当前会话后挑一个替补。
    pub(super) fn first_active(&self) -> Option<u64> {
        match self {
            Self::Pane(p) => p.tabs.first().copied(),
            Self::Split { a, b, .. } => a.first_active().or_else(|| b.first_active()),
        }
    }

    /// 把包含 `id` 的窗格的 active 设为它（切换页签）。
    pub(super) fn set_active(&mut self, id: u64) -> bool {
        match self {
            Self::Pane(p) if p.tabs.contains(&id) => {
                p.active = id;
                true
            }
            Self::Split { a, b, .. } => {
                if a.contains(id) {
                    a.set_active(id)
                } else {
                    b.set_active(id)
                }
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn p(id: u64) -> Layout {
        Layout::new(id)
    }

    #[test]
    fn split_then_active_pane_is_new() {
        let mut l = p(1);
        assert!(l.split(1, SplitDir::Horizontal, 2));
        assert_eq!(l.pane_count(), 2);
        assert_eq!(l.active_pane(2).unwrap().active, 2);
        assert_eq!(l.active_pane(1).unwrap().active, 1);
    }

    #[test]
    fn nested_split_keeps_all_sessions() {
        let mut l = p(1);
        assert!(l.split(1, SplitDir::Horizontal, 2)); // [1 | 2]
        assert!(l.split(2, SplitDir::Vertical, 3)); // [1 | (2 / 3)]
        assert_eq!(l.session_count(), 3);
        assert!(l.active_pane(3).is_some());
        assert!(l.active_pane(2).is_some());
        assert!(l.active_pane(1).is_some());
    }

    #[test]
    fn add_tab_into_active_pane() {
        let mut l = p(1);
        l.split(1, SplitDir::Vertical, 2);
        let pane = l.active_pane_mut(2).unwrap();
        pane.tabs.push(3);
        pane.active = 3;
        assert_eq!(l.session_count(), 3);
        assert_eq!(l.pane_count(), 2, "新增页签不应增加窗格数");
        assert_eq!(l.active_pane(3).unwrap().tabs, vec![2, 3]);
    }

    #[test]
    fn removing_last_tab_collapses_pane() {
        let mut l = p(1);
        l.split(1, SplitDir::Horizontal, 2); // [1 | 2]
        l.active_pane_mut(2).unwrap().tabs.push(3);
        assert!(l.remove(3));
        assert_eq!(l.session_count(), 2);
        assert_eq!(l.pane_count(), 2);
        // 删掉窗格 2 的最后一个会话后，窗格消失，只剩窗格 1。
        assert!(l.remove(2));
        assert_eq!(l.pane_count(), 1);
        assert_eq!(l.active_pane(1).unwrap().tabs, vec![1]);
    }

    #[test]
    fn removing_only_session_leaves_empty_pane() {
        let mut l = p(1);
        assert!(l.remove(1));
        assert_eq!(l.session_count(), 0);
        assert_eq!(l.pane_count(), 0);
    }
}
