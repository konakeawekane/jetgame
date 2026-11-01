<# Check the complete private runtime and import networking modules without system Python. #>
[CmdletBinding()]
param([Parameter(Mandatory)][string]$RuntimeDirectory)
$ErrorActionPreference = 'Stop'
$runtimePath = (Resolve-Path -LiteralPath $RuntimeDirectory).Path
$pathFiles = @(Get-ChildItem -LiteralPath $runtimePath -Filter 'python*._pth' |
    Where-Object { $_.BaseName -match '^python[0-9]+$' })
if ($pathFiles.Count -ne 1) { throw "Expected one Python ._pth file in $runtimePath" }
$stem = $pathFiles[0].BaseName
$required = @('python.exe', 'python3.dll', "$stem.dll", "$stem.zip", 'vcruntime140.dll',
    'vcruntime140_1.dll', '_socket.pyd', '_overlapped.pyd', 'select.pyd', 'LICENSE.txt')
foreach ($file in $required) {
    if (-not (Test-Path -LiteralPath (Join-Path $runtimePath $file) -PathType Leaf)) {
        throw "Incomplete bundled Python: missing $file in $runtimePath"
    }
}
$startInfo = New-Object System.Diagnostics.ProcessStartInfo
$startInfo.FileName = Join-Path $runtimePath 'python.exe'
$startInfo.Arguments = '-I -c "import asyncio, socket, zlib, json, ssl, hashlib, ctypes, sys; assert sys.version_info >= (3, 11); print(sys.version.split()[0])"'
$startInfo.WorkingDirectory = $runtimePath
$startInfo.UseShellExecute = $false
$startInfo.CreateNoWindow = $true
$startInfo.RedirectStandardOutput = $true
$startInfo.RedirectStandardError = $true
$startInfo.EnvironmentVariables['PATH'] = Join-Path $env:SystemRoot 'System32'
$process = [Diagnostics.Process]::Start($startInfo)
try {
    if (-not $process.WaitForExit(10000)) {
        $process.Kill()
        throw 'Bundled Python validation timed out.'
    }
    $output = $process.StandardOutput.ReadToEnd().Trim()
    $errorOutput = $process.StandardError.ReadToEnd().Trim()
    if ($process.ExitCode -ne 0) { throw "Bundled Python cannot load (exit $($process.ExitCode)): $errorOutput" }
    Write-Output "Validated private Python $output at $runtimePath"
} finally { $process.Dispose() }
