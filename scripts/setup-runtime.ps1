<# Fetch the game's private Windows x64 Python. No installer or global changes. #>
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$spec = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'runtime/python-runtime.json') | ConvertFrom-Json
$runtimePath = Join-Path $projectRoot 'runtime/python'
$marker = Join-Path $runtimePath 'jetgame-runtime.json'
if (Test-Path -LiteralPath $marker) {
    $existing = Get-Content -Raw -LiteralPath $marker | ConvertFrom-Json
    if ($existing.sha256 -eq $spec.sha256 -and
        (Test-Path -LiteralPath (Join-Path $runtimePath 'python.exe')) -and
        (Test-Path -LiteralPath (Join-Path $runtimePath 'python314.zip'))) {
        & (Join-Path $PSScriptRoot 'validate-runtime.ps1') -RuntimeDirectory $runtimePath
        Write-Output "Bundled Python $($spec.version) is ready at $runtimePath"
        return
    }
}
if ((Test-Path -LiteralPath $runtimePath) -and (Get-ChildItem -Force -LiteralPath $runtimePath | Select-Object -First 1)) {
    throw 'runtime/python is already populated. Choose how to preserve or replace it before rerunning setup.'
}
$cachePath = Join-Path $projectRoot 'target/runtime-downloads'
New-Item -ItemType Directory -Path $cachePath -Force | Out-Null
$archivePath = Join-Path $cachePath "python-$($spec.version)-embed-amd64.zip"
if (-not (Test-Path -LiteralPath $archivePath)) {
    Invoke-WebRequest -UseBasicParsing -Uri $spec.url -OutFile $archivePath
}
$digest = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash
if ($digest -ne $spec.sha256) {
    throw "Bundled Python checksum mismatch: $archivePath"
}
Expand-Archive -LiteralPath $archivePath -DestinationPath $runtimePath
& (Join-Path $PSScriptRoot 'validate-runtime.ps1') -RuntimeDirectory $runtimePath
$spec | ConvertTo-Json | Set-Content -LiteralPath $marker -Encoding UTF8
Write-Output "Bundled Python $($spec.version) is ready at $runtimePath"
