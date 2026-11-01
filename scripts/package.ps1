<# Build a Windows bundle with Rust gameplay and Python networking.
   Example: .\scripts\package.ps1
   Supply an extracted, supported x64 CPython embeddable package. No download or global install occurs.
#>
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$PythonRuntime = 'runtime/python',
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
$runtimeSource = (Resolve-Path -LiteralPath $(if ([IO.Path]::IsPathRooted($PythonRuntime)) { $PythonRuntime } else { Join-Path $projectRoot $PythonRuntime })).Path
if (-not (Test-Path -LiteralPath (Join-Path $runtimeSource 'python.exe')) -or
    -not (Test-Path -LiteralPath (Join-Path $runtimeSource 'python3.dll')) -or
    -not (Get-ChildItem -LiteralPath $runtimeSource -Filter 'python*._pth')) {
    throw 'Supply an extracted Windows embeddable Python package, including python.exe, python3.dll and its ._pth file.'
}
& (Join-Path $PSScriptRoot 'validate-runtime.ps1') -RuntimeDirectory $runtimeSource
$outputPath = [IO.Path]::GetFullPath((Join-Path $projectRoot $OutputDirectory))
$distRoot = [IO.Path]::GetFullPath((Join-Path $projectRoot 'dist')) + [IO.Path]::DirectorySeparatorChar
if (-not $outputPath.StartsWith($distRoot,[StringComparison]::OrdinalIgnoreCase)) { throw 'OutputDirectory must be a new directory under this project''s dist/ folder.' }
if ((Test-Path -LiteralPath $outputPath) -and (Get-ChildItem -Force -LiteralPath $outputPath | Select-Object -First 1)) {
    throw 'OutputDirectory is not empty. Choose a new output directory; existing packages are never overwritten.'
}
if (Test-Path -LiteralPath "$outputPath.zip") { throw 'The output ZIP already exists. Choose a new OutputDirectory.' }
if (-not $PSCmdlet.ShouldProcess($outputPath,'Build and assemble the multiplayer game bundle')) { return }
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
    & (Join-Path $PSScriptRoot 'validate-runtime.ps1') -RuntimeDirectory $runtimeDestination
    $networkConfig = Get-Content -Raw -LiteralPath (Join-Path $projectRoot 'networking.json') | ConvertFrom-Json
    $networkConfig.python = 'runtime/python/python.exe'
    $networkConfig |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $outputPath 'networking.json') -Encoding UTF8
    Copy-Item -LiteralPath (Join-Path $projectRoot 'game.json') -Destination $outputPath
    Copy-Item -LiteralPath (Join-Path $projectRoot 'readme.md') -Destination $outputPath
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::Open("$outputPath.zip", [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in (Get-ChildItem -Recurse -Force -LiteralPath $outputPath -File)) {
            $relative = $file.FullName.Substring($outputPath.Length + 1).Replace('\', '/')
            $entryName = (Split-Path -Leaf $outputPath) + '/' + $relative
            [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $file.FullName,
                $entryName, [IO.Compression.CompressionLevel]::Optimal)
        }
    } finally { $zip.Dispose() }
    $zip = [IO.Compression.ZipFile]::OpenRead("$outputPath.zip")
    try {
        foreach ($file in (Get-ChildItem -Force -LiteralPath $runtimeDestination -File)) {
            $entryName = (Split-Path -Leaf $outputPath) + '/runtime/python/' + $file.Name
            $entry = $zip.GetEntry($entryName)
            if ($null -eq $entry -or $entry.Length -ne $file.Length) { throw "Release ZIP is missing or truncates $entryName" }
        }
    } finally { $zip.Dispose() }
    (Get-FileHash -LiteralPath "$outputPath.zip" -Algorithm SHA256).Hash |
        Set-Content -LiteralPath "$outputPath.zip.sha256" -Encoding ASCII
    Write-Output "Portable release ready: $outputPath.zip. Extract the entire ZIP before launching."
} finally { Pop-Location }
