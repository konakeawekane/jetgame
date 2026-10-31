<# Build a Windows bundle with Rust gameplay and Python networking.
   Example: .\scripts\package.ps1 -PythonRuntime C:\path\to\python-embed
   Supply an extracted, supported x64 CPython embeddable package. No download or global install occurs.
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)][string]$PythonRuntime,
    [string]$OutputDirectory = 'dist/jetgame'
)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$gameConfig = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'game.json') | ConvertFrom-Json
if ([IO.Path]::IsPathRooted($gameConfig.assets.root)) { throw 'Packaged assets.root must be project-relative.' }
$assetSource = [IO.Path]::GetFullPath((Join-Path $projectRoot $gameConfig.assets.root))
if (-not $assetSource.StartsWith(($projectRoot + [IO.Path]::DirectorySeparatorChar),[StringComparison]::OrdinalIgnoreCase)) {
    throw 'Packaged assets.root must stay inside the project.'
}
if (-not (Test-Path -LiteralPath $assetSource -PathType Container)) { throw 'Configured asset directory is missing.' }
$runtimeSource = (Resolve-Path -LiteralPath $PythonRuntime).Path
if (-not (Test-Path -LiteralPath (Join-Path $runtimeSource 'python.exe')) -or
    -not (Test-Path -LiteralPath (Join-Path $runtimeSource 'python3.dll')) -or
    -not (Get-ChildItem -LiteralPath $runtimeSource -Filter 'python*._pth')) {
    throw 'Supply an extracted Windows embeddable Python package, including python.exe, python3.dll and its ._pth file.'
}
$outputPath = [IO.Path]::GetFullPath((Join-Path $projectRoot $OutputDirectory))
$distRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'dist')) + [IO.Path]::DirectorySeparatorChar
if (-not $outputPath.StartsWith($distRoot,[StringComparison]::OrdinalIgnoreCase)) { throw 'OutputDirectory must be a new directory under this project''s dist/ folder.' }
if ((Test-Path -LiteralPath $outputPath) -and (Get-ChildItem -Force -LiteralPath $outputPath | Select-Object -First 1)) {
    throw 'OutputDirectory is not empty. Choose a new output directory; existing packages are never overwritten.'
}
if (-not $PSCmdlet.ShouldProcess($outputPath,'Build and assemble a hosting/discovery prototype bundle')) { return }
Push-Location -LiteralPath $projectRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Cargo release build failed.' }
    New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'target/release/jetgame.exe') -Destination $outputPath
    $assetDestination = Join-Path $outputPath $gameConfig.assets.root
    New-Item -ItemType Directory -Path $assetDestination -Force | Out-Null
    Get-ChildItem -Force -LiteralPath $assetSource | Copy-Item -Destination $assetDestination -Recurse
    $serverDestination = Join-Path $outputPath 'networking'
    New-Item -ItemType Directory -Path (Join-Path $serverDestination 'jetgame_net') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'networking/server_main.py') -Destination $serverDestination
    Copy-Item -LiteralPath (Join-Path $projectRoot 'networking/agent_main.py') -Destination $serverDestination
    Get-ChildItem -LiteralPath (Join-Path $projectRoot 'networking/jetgame_net') -Filter '*.py' |
        Copy-Item -Destination (Join-Path $serverDestination 'jetgame_net')
    Copy-Item -LiteralPath (Join-Path $projectRoot 'networking/README.md') -Destination $serverDestination
    $runtimeDestination = Join-Path $outputPath 'runtime/python'
    New-Item -ItemType Directory -Path $runtimeDestination -Force | Out-Null
    Get-ChildItem -Force -LiteralPath $runtimeSource | Copy-Item -Destination $runtimeDestination -Recurse
    $networkConfig = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'networking.json') | ConvertFrom-Json
    $networkConfig.python = 'runtime/python/python.exe'
    $networkConfig |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputPath 'networking.json') -Encoding UTF8
    Copy-Item -LiteralPath (Join-Path $projectRoot 'game.json') -Destination $outputPath
    Copy-Item -LiteralPath (Join-Path $projectRoot 'readme.md') -Destination $outputPath
    Write-Output "Bundle created at $outputPath with LAN discovery, invitation joining, and host-authoritative gameplay over TCP/UDP."
} finally { Pop-Location }
