//! RFC 017 M4-link Phase B §D2.1：codegen 发射角色。

/// 决定全局 dbg / 属性表符号的所有权。
///
/// - `MainObject`：主程序 `.o`，发射 `@__arc_dbg_table` 等 **external** 强符号
/// - `DynamicLibrary`：`arc build --dynamic` 共享库 `.o`——**发射** dbg 表
///   （共享库内嵌完整 runtime，`rt_debug.o` 硬引用 `__arc_dbg_table`/
///   `__arc_dbg_count`，Windows PE 链接须就地解析）；同时导出 Entry wrapper
///   与资源符号
///
/// 默认 ctor `__ctor::Class` 不受本枚举影响（由所有权过滤 / `linkonce_odr`
/// 独立决定），与 `EmitRole` 正交。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum EmitRole {
    /// 主程序 `.o`：发射 external 强符号全局表。
    #[default]
    MainObject,
    /// `arc build --dynamic` 共享库 `.o`：发射 dbg 表 + 导出 Entry 包装与资源。
    DynamicLibrary,
}
