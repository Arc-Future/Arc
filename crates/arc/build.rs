//! Windows PE 版本资源（VERSIONINFO）：把作者/版权/产品信息编译进 arc.exe
//! ——资源管理器文件属性与 Get-ItemProperty VersionInfo 可见。
//! 非 Windows 目标为空操作。图标：仓库暂无 .ico，跳过（不影响资源编译）。

fn main() {
    if std::env::var("CARGO_CFG_WINDOWS").is_err() {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    // 版本四元组：CARGO_PKG_VERSION（语义三位）补 .0 凑齐 FileVersion 四段。
    let semver = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let file_version = format!("{semver}.0");
    res.set("FileDescription", "Arc Compiler CLI");
    res.set("CompanyName", "LUSIDA (Start)");
    res.set("ProductName", "Arc");
    res.set("LegalCopyright", "Copyright (C) 2026 LUSIDA (Start)");
    res.set("OriginalFilename", "arc.exe");
    res.set("FileVersion", &file_version);
    res.set("ProductVersion", &file_version);
    if let Err(e) = res.compile() {
        // 资源编译失败（如 rc.exe 缺失）不应静默——但也不阻断非发布构建：
        // 打印警告继续（未嵌资源的 exe 仅缺属性面板元数据）。
        println!("cargo:warning=Windows resource compile failed: {e}");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
