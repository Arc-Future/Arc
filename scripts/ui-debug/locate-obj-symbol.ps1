# locate-obj-symbol.ps1 - 在 COFF .o 中按文件偏移定位所属函数符号
# 用法：直接修改下面两行硬编码参数（终端代理对命名参数传参不稳定）
$Obj = "examples\ArmlDemo\obj\Debug\code\ArmlDemo\out.o"
$FileOffHex = "EEB8F"
$llvm = "C:\Program Files\LLVM\bin"
$FileOff = [Convert]::ToInt64($FileOffHex.Replace("0x", ""), 16)
Write-Output ("FileOff=0x{0:X}" -f $FileOff)
# 1) 段表：找包含 FileOff 的段索引
$secText = & "$llvm\llvm-readobj.exe" --sections $Obj 2>$null | Out-String -Width 4096
Write-Output ("secText len={0}" -f $secText.Length)
$secMatches = [regex]::Matches($secText, '(?s)Section \{\s*Number: (\d+).*?Name: ([^ ]+).*?RawDataSize: (\d+).*?PointerToRawData: 0x([0-9A-F]+)')
Write-Output ("sections parsed: {0}" -f $secMatches.Count)
$targetSec = -1; $targetBase = 0
foreach ($m in $secMatches) {
    $num = [int]$m.Groups[1].Value
    $size = [long]$m.Groups[3].Value
    $ptr = [Convert]::ToInt64($m.Groups[4].Value, 16)
    if ($FileOff -ge $ptr -and $FileOff -lt ($ptr + $size)) {
        $targetSec = $num; $targetBase = $ptr
        Write-Output ("section #{0} name={1} ptr=0x{2:X} size=0x{3:X} offInSec=0x{4:X}" -f $num, $m.Groups[2].Value, $ptr, $size, ($FileOff - $ptr))
        break
    }
}
if ($targetSec -lt 0) { Write-Output "section not found"; exit 1 }
# 2) 符号表：找该段内 Value <= offInSec 的最大函数符号
$offInSec = $FileOff - $targetBase
$symText = & "$llvm\llvm-readobj.exe" --symbols $Obj 2>$null | Out-String -Width 4096
Write-Output ("symText len={0}" -f $symText.Length)
$symMatches = [regex]::Matches($symText, '(?s)Symbol \{\s*Name: ([^\r\n]+).*?Value: 0x([0-9A-F]+).*?Section: (?:[^\r\n]*\()?(\d+)(?:\))?')
Write-Output ("symbols parsed: {0}" -f $symMatches.Count)
$best = $null; $bestVal = -1
foreach ($m in $symMatches) {
    $name = $m.Groups[1].Value
    $val = [Convert]::ToInt64($m.Groups[2].Value, 16)
    $sec = [int]$m.Groups[3].Value
    if ($sec -eq $targetSec -and $val -le $offInSec -and $val -gt $bestVal) {
        $bestVal = $val; $best = $name
    }
}
Write-Output ("function: {0} +0x{1:X}" -f $best, ($offInSec - $bestVal))
