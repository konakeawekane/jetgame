<# Build a Windows prototype bundle with BOTH native joining/discovery and Python hosting.
   Example: .\scripts\package.ps1 -PythonRuntime C:\path\to\python-embed -DirectoryEndpoint directory.example.com:29000
   Supply an extracted, supported x64 CPython embeddable package. No download or global install occurs.
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [Parameter(Mandatory)][string]$PythonRuntime,
    [Parameter(Mandatory)][string]$DirectoryEndpoint,
    [string]$OutputDirectory = 'dist/jetgame',
    [switch]$Development
)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$runtimeSource = (Resolve-Path -LiteralPath $PythonRuntime).Path
if (-not (Test-Path -LiteralPath (Join-Path $runtimeSource 'python.exe')) -or
    -not (Test-Path -LiteralPath (Join-Path $runtimeSource 'python3.dll')) -or
    -not (Get-ChildItem -LiteralPath $runtimeSource -Filter 'python*._pth')) {
    throw 'Supply an extracted Windows embeddable Python package, including python.exe, python3.dll and its ._pth file.'
}
if ($DirectoryEndpoint -notmatch '^(\[[^\]]+\]|[^:\s]+):([0-9]{1,5})$') { throw 'DirectoryEndpoint must be HOST:PORT.' }
$directoryHost = $Matches[1]
$directoryPort = [int]$Matches[2]
if ($directoryPort -lt 1 -or $directoryPort -gt 65535) { throw 'Invalid directory port.' }
if (-not $Development -and $directoryHost -in @('localhost','127.0.0.1','0.0.0.0','[::1]','[::]')) {
    throw 'Release bundles must configure the shared public directory; use -Development for a loopback prototype.'
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
    Copy-Item -LiteralPath (Join-Path $projectRoot 'assets') -Destination $outputPath -Recurse
    $serverDestination = Join-Path $outputPath 'networking'
    New-Item -ItemType Directory -Path (Join-Path $serverDestination 'jetgame_net') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'networking/server_main.py') -Destination $serverDestination
    Get-ChildItem -LiteralPath (Join-Path $projectRoot 'networking/jetgame_net') -Filter '*.py' |
        Copy-Item -Destination (Join-Path $serverDestination 'jetgame_net')
    Copy-Item -LiteralPath (Join-Path $projectRoot 'networking/README.md') -Destination $serverDestination
    $runtimeDestination = Join-Path $outputPath 'runtime/python'
    New-Item -ItemType Directory -Path $runtimeDestination -Force | Out-Null
    Get-ChildItem -Force -LiteralPath $runtimeSource | Copy-Item -Destination $runtimeDestination -Recurse
    @{directory=$DirectoryEndpoint;python='runtime/python/python.exe';server_script='networking/server_main.py';
      host_bind='0.0.0.0';host_port=29010;public_port=$null} |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputPath 'networking.json') -Encoding UTF8
    Write-Output "Prototype bundle created at $outputPath. Internet deployment/NAT traversal and gameplay sync remain unfinished."
} finally { Pop-Location }
