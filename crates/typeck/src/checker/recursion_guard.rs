//! 栈式 in-progress 环检测（区别于 `instantiated` 持久 memoize）。
//!
//! 深度哨兵已全部移除（L2 收口），环检测由本模块统一承接；编译器自身栈溢出
//! 由 `check_expr_inner` 的统一高阈值背板（`TYPE_CHECK_RECURSION_DEPTH`）兜底。

use indexmap::IndexSet;
use std::cell::RefCell;
use std::rc::Rc;

/// `enter` 检测到同一 key 已在栈上时返回的环路径。
pub struct Cycle<K> {
    path: Vec<K>,
}

struct Inner<K> {
    in_progress: IndexSet<K>,
    path: Vec<K>,
}

/// 栈式 in-progress 环检测。`enter` 返回 `Ok(令牌)` = 首次进入；
/// `Err(Cycle)` = 同一 key 已在栈上（真环，应短路而非报错）。
pub struct RecursionGuard<K> {
    inner: Rc<RefCell<Inner<K>>>,
}

/// `enter` 首次进入时颁发的令牌；Drop 时移除对应 key。
pub struct GuardToken<K: Eq + std::hash::Hash> {
    key: K,
    inner: Rc<RefCell<Inner<K>>>,
}

impl<K: Clone + Eq + std::hash::Hash> RecursionGuard<K> {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner {
                in_progress: IndexSet::new(),
                path: Vec::new(),
            })),
        }
    }

    pub fn enter(&self, key: &K) -> Result<GuardToken<K>, Cycle<K>> {
        let mut inner = self.inner.borrow_mut();
        if inner.in_progress.contains(key) {
            let mut path = inner.path.clone();
            if let Some(pos) = path.iter().position(|k| k == key) {
                path.drain(..pos);
            }
            path.push(key.clone());
            return Err(Cycle { path });
        }
        inner.in_progress.insert(key.clone());
        inner.path.push(key.clone());
        drop(inner);
        Ok(GuardToken {
            key: key.clone(),
            inner: self.inner.clone(),
        })
    }
}

impl<K: Eq + std::hash::Hash> Drop for GuardToken<K> {
    fn drop(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.in_progress.swap_remove(&self.key);
        if let Some(pos) = inner.path.iter().rposition(|k| k == &self.key) {
            inner.path.remove(pos);
        }
    }
}

impl<K: std::fmt::Display> RecursionGuard<K> {
    /// 当前 in-progress path 快照（供统一背板崩溃诊断输出「哪条路径超深」）。
    pub fn render_path(&self) -> String {
        let inner = self.inner.borrow();
        if inner.path.is_empty() {
            return String::from("(empty)");
        }
        inner
            .path
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(" → ")
    }
}

impl<K: std::fmt::Display> Cycle<K> {
    /// `A → B → A` 环路径渲染（风格对齐 `arc-cycle-001`）。
    pub fn render(&self) -> String {
        self.path
            .iter()
            .map(|k| k.to_string())
            .collect::<Vec<_>>()
            .join(" → ")
    }

    /// `ARC_DEBUG_RECUR` 门控的 debug 输出。
    pub fn report(&self, kind: &str) {
        if std::env::var("ARC_DEBUG_RECUR").is_ok() {
            eprintln!("{kind} 递归环：{}", self.render());
        }
    }
}
