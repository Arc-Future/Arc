# find-crash-obj.ps1 - 在所有链接 .o 中搜索崩溃指令字节签名
# 崩溃点指令序列（无重定位字节）：movq 0x38(%rsp),%rcx; movq 0x8(%rcx),%rax; callq *0x18(%rax)
$sig = [byte[]](0x48,0x8B,0x4C,0x24,0x38,0x48,0x8B,0x41,0x08,0xFF,0x50,0x18)
$arcHome = if ($env:ARC_HOME) { $env:ARC_HOME } else { Join-Path $env:USERPROFILE ".arc" }
$dirs = @(
    (Join-Path $arcHome "rt_cache\x86_64-pc-windows-msvc_debug_nog"),
    "examples\ArmlDemo\obj",
    "std\\UI\\Core"
)
$files = @()
foreach ($d in $dirs) {
    if (Test-Path $d) { $files += Get-ChildItem $d -Recurse -Filter *.o -ErrorAction SilentlyContinue }
}
Write-Output ("scanning {0} .o files" -f $files.Count)
foreach ($f in $files) {
    $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
    for ($i = 0; $i -le $bytes.Length - $sig.Length; $i++) {
        $m = $true
        for ($j = 0; $j -lt $sig.Length; $j++) {
            if ($bytes[$i + $j] -ne $sig[$j]) { $m = $false; break }
        }
        if ($m) {
            Write-Output ("MATCH {0} @fileoff 0x{1:X}" -f $f.FullName, $i)
        }
    }
}
Write-Output "done"
