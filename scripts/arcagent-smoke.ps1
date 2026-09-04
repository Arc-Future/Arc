# arcagent-smoke.ps1 — 真实连通冒烟：读环境变量，用真实 API 跑一句话，断言返回非错误。
#
# 用法：
#   $env:ARC_AGENT_API_KEY = "sk-xxx"
#   powershell -File scripts/arcagent-smoke.ps1 --provider deepseek
#   powershell -File scripts/arcagent-smoke.ps1 --provider agnes
#   powershell -File scripts/arcagent-smoke.ps1 --provider openai
#
# 环境变量（key 只读环境变量，绝不落盘）：
#   ARC_AGENT_API_KEY    必填。真实 API 密钥。
#   ARC_AGENT_BASE_URL   可选。覆盖 provider 默认 base URL（会补 /chat/completions）。
#   ARC_AGENT_MODEL      可选。覆盖 provider 默认模型。
#
# 参数：
#   --provider <deepseek|agnes|openai>    默认 deepseek。
#
# 退出码：0 = 冒烟通过（返回非错误且 choices 非空）；非 0 = 失败。
param(
    [string]$Provider = "deepseek"
)

$ErrorActionPreference = "Stop"

function Resolve-Defaults {
    param([string]$Name)
    switch ($Name.ToLowerInvariant()) {
        "deepseek" { return @{ BaseUrl = "https://api.deepseek.com"; Model = "deepseek-v4-pro" } }
        "agnes"    { return @{ BaseUrl = "https://apihub.agnes-ai.com/v1"; Model = "agnes-2.0-flash" } }
        "openai"   { return @{ BaseUrl = "https://api.openai.com/v1"; Model = "gpt-4o-mini" } }
        default    { return $null }
    }
}

$defaults = Resolve-Defaults $Provider
if ($null -eq $defaults) {
    Write-Host "error: unknown provider '$Provider' (supported: deepseek|agnes|openai)"
    exit 2
}

$apiKey = $env:ARC_AGENT_API_KEY
if ([string]::IsNullOrWhiteSpace($apiKey)) {
    Write-Host "error: ARC_AGENT_API_KEY is not set - pass a real key via environment variable (never write it to disk)"
    exit 2
}

$baseUrl = $env:ARC_AGENT_BASE_URL
if ([string]::IsNullOrWhiteSpace($baseUrl)) {
    $baseUrl = $defaults.BaseUrl
}

$model = $env:ARC_AGENT_MODEL
if ([string]::IsNullOrWhiteSpace($model)) {
    $model = $defaults.Model
}

# 端点归一：去尾斜杠，补 /chat/completions（除非已含）。
$endpoint = $baseUrl.TrimEnd('/')
if (-not $endpoint.EndsWith("/chat/completions")) {
    $endpoint = $endpoint + "/chat/completions"
}

$body = @{
    model      = $model
    messages   = @(
        @{ role = "user"; content = "Reply with the single word PONG." }
    )
    max_tokens = 16
} | ConvertTo-Json -Depth 5

$headers = @{
    Authorization = "Bearer $apiKey"
    "Content-Type" = "application/json"
}

# TLS 1.2（Windows PowerShell 5.1 默认可能仅 TLS 1.0）。
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

try {
    $response = Invoke-RestMethod -Uri $endpoint -Method Post -Headers $headers -Body $body -TimeoutSec 60
}
catch {
    Write-Host "error: smoke request failed for '$Provider' ($endpoint): $($_.Exception.Message)"
    exit 1
}

# 断言：返回非错误（error 字段）且 choices 非空。
if ($null -ne $response.error) {
    Write-Host "error: provider returned error: $($response.error.message)"
    exit 1
}
if ($null -eq $response.choices -or $response.choices.Count -eq 0) {
    Write-Host "error: provider returned no choices"
    exit 1
}

$content = $response.choices[0].message.content
if ([string]::IsNullOrWhiteSpace($content)) {
    $content = $response.choices[0].message.reasoning_content
}
if ([string]::IsNullOrWhiteSpace($content)) {
    $content = "(empty content)"
}

Write-Host "arcagent-smoke OK [$Provider / $model]"
Write-Host "  endpoint: $endpoint"
Write-Host "  reply: $content"
exit 0
