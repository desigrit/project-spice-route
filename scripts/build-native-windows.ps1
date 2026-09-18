[CmdletBinding()]
param(
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version = '1.4.2',
    [string]$EnginePath,
    [string]$NsisPath,
    [string]$CrtDirectory,
    [string]$CargoTargetDirectory,
    [switch]$SkipStartupProbe
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Build the native Windows installer on Windows.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$artifactRoot = Join-Path $repositoryRoot 'artifacts'
$publishDirectory = [IO.Path]::GetFullPath((Join-Path $artifactRoot 'native-win-x64'))
$projectPath = Join-Path $repositoryRoot 'native\windows\SpiceRoute.Windows\SpiceRoute.Windows.csproj'
$installerScript = Join-Path $repositoryRoot 'native\windows\installer.nsi'
$coreManifest = Join-Path $repositoryRoot 'src-tauri\core\Cargo.toml'
$msvcScript = Join-Path $PSScriptRoot 'msvc-env.cmd'
$installerPath = Join-Path $artifactRoot "Spice-Route-$Version-windows-x64-setup.exe"
$rustTarget = 'x86_64-pc-windows-msvc'

foreach ($requiredPath in @($projectPath, $installerScript)) {
    if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
        throw "Required build input is missing: $requiredPath"
    }
}
$dotnetCommand = Get-Command dotnet -ErrorAction Stop
$sdkLines = & $dotnetCommand.Source --list-sdks
if ($LASTEXITCODE -ne 0 -or -not ($sdkLines | Where-Object { $_ -match '^(9|[1-9][0-9])\.' })) {
    throw 'Install the .NET 9 SDK or newer before building the native Windows app.'
}

if (-not $NsisPath) {
    $nsisCommand = Get-Command makensis -ErrorAction SilentlyContinue
    $nsisCandidates = @(
        $(if ($nsisCommand) { $nsisCommand.Source }),
        (Join-Path $env:LOCALAPPDATA 'tauri\NSIS\makensis.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe')
    )
    $NsisPath = $nsisCandidates | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
}
if (-not $NsisPath -or -not (Test-Path -LiteralPath $NsisPath -PathType Leaf)) {
    throw 'NSIS was not found. Install NSIS 3 or pass -NsisPath with the path to makensis.exe.'
}

New-Item -ItemType Directory -Path $artifactRoot -Force | Out-Null

if (-not $EnginePath) {
    Write-Host 'Building the shared Rust sync engine...'
    $previousCargoTarget = $env:CARGO_TARGET_DIR
    $nativeCargoTarget = if ($CargoTargetDirectory) {
        [IO.Path]::GetFullPath($CargoTargetDirectory)
    } elseif ($previousCargoTarget) {
        [IO.Path]::GetFullPath($previousCargoTarget)
    } else {
        Join-Path $repositoryRoot 'src-tauri\target'
    }
    try {
        $env:CARGO_TARGET_DIR = $nativeCargoTarget
        & $msvcScript cargo build --release --manifest-path $coreManifest --target $rustTarget --bin spice-route-engine
        if ($LASTEXITCODE -ne 0) { throw 'The Rust sync engine build failed.' }
        $EnginePath = Join-Path $nativeCargoTarget "$rustTarget\release\spice-route-engine.exe"
    } finally {
        $env:CARGO_TARGET_DIR = $previousCargoTarget
    }
}
$EnginePath = [IO.Path]::GetFullPath($EnginePath)
if (-not (Test-Path -LiteralPath $EnginePath -PathType Leaf)) {
    throw "The sync engine executable is missing: $EnginePath"
}

# This exact generated directory is the only recursive removal in this script.
# Reject links and check its resolved parent before clearing a previous publish.
$expectedPublishDirectory = Join-Path ([IO.Path]::GetFullPath($artifactRoot)) 'native-win-x64'
if (-not $publishDirectory.Equals($expectedPublishDirectory, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The native publish directory resolved outside its expected build location.'
}
foreach ($directory in @($artifactRoot, $publishDirectory)) {
    if ((Test-Path -LiteralPath $directory) -and
        ((Get-Item -LiteralPath $directory -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "The build output cannot be a filesystem link: $directory"
    }
}
if (Test-Path -LiteralPath $publishDirectory) {
    Remove-Item -LiteralPath $publishDirectory -Recurse -Force
}

Write-Host 'Publishing WinUI 3 with the .NET and Windows App SDK runtimes...'
& $dotnetCommand.Source publish $projectPath --configuration Release --runtime win-x64 --self-contained true --output $publishDirectory `
    "-p:Version=$Version" '-p:Platform=x64' '-p:WindowsPackageType=None' '-p:WindowsAppSDKSelfContained=true' `
    '-p:WindowsAppSdkDeploymentManagerInitialize=false' '-p:PublishSingleFile=false' '-p:PublishTrimmed=false'
if ($LASTEXITCODE -ne 0) { throw 'The native Windows app publish failed.' }
Copy-Item -LiteralPath $EnginePath -Destination (Join-Path $publishDirectory 'SpiceRoute.Engine.exe') -Force

# App-local CRT files allow the Rust engine and native UI libraries to start on a
# clean PC without a separate Visual C++ redistributable installer.
if (-not $CrtDirectory) {
    $redistRoot = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\BuildTools\VC\Redist\MSVC'
    if (Test-Path -LiteralPath $redistRoot -PathType Container) {
        $CrtDirectory = Get-ChildItem -LiteralPath $redistRoot -Directory |
            Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } |
            Sort-Object { [version]$_.Name } -Descending |
            ForEach-Object { Get-ChildItem -Path (Join-Path $_.FullName 'x64\Microsoft.VC*.CRT') -Directory -ErrorAction SilentlyContinue } |
            Select-Object -First 1 -ExpandProperty FullName
    }
}
if (-not $CrtDirectory -or -not (Test-Path -LiteralPath $CrtDirectory -PathType Container)) {
    throw 'The Visual C++ x64 CRT redistribution folder was not found. Pass -CrtDirectory from your Visual Studio installation.'
}
Get-ChildItem -LiteralPath $CrtDirectory -File -Filter '*.dll' | ForEach-Object {
    $destination = Join-Path $publishDirectory $_.Name
    $replace = -not (Test-Path -LiteralPath $destination -PathType Leaf)
    if (-not $replace) {
        $existingInfo = (Get-Item -LiteralPath $destination).VersionInfo
        $sourceInfo = $_.VersionInfo
        $existingVersion = [version]"$($existingInfo.FileMajorPart).$($existingInfo.FileMinorPart).$($existingInfo.FileBuildPart).$($existingInfo.FilePrivatePart)"
        $sourceVersion = [version]"$($sourceInfo.FileMajorPart).$($sourceInfo.FileMinorPart).$($sourceInfo.FileBuildPart).$($sourceInfo.FilePrivatePart)"
        $replace = $sourceVersion -gt $existingVersion
    }
    if ($replace) { Copy-Item -LiteralPath $_.FullName -Destination $destination -Force }
}

foreach ($requiredFile in @('SpiceRoute.exe', 'SpiceRoute.Engine.exe', 'SpiceRoute.runtimeconfig.json', 'SpiceRoute.pri', 'App.xbf', 'MainWindow.xbf', 'coreclr.dll', 'hostfxr.dll', 'Microsoft.UI.Xaml.dll', 'vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll')) {
    if (-not (Test-Path -LiteralPath (Join-Path $publishDirectory $requiredFile) -PathType Leaf)) {
        throw "The self-contained package is missing $requiredFile. The installer was not produced."
    }
}

if (-not $SkipStartupProbe) {
    Write-Host 'Loading the packaged WinUI resources in a hidden startup probe...'
    $appExecutable = Join-Path $publishDirectory 'SpiceRoute.exe'
    $startupProbe = Start-Process -FilePath $appExecutable -ArgumentList '--startup-probe' `
        -WorkingDirectory $publishDirectory -WindowStyle Hidden -PassThru
    try {
        if (-not $startupProbe.WaitForExit(15000)) {
            $startupProbe.Kill($true)
            throw 'The packaged app did not finish its hidden startup probe within 15 seconds.'
        }
        $startupProbe.Refresh()
        if ($startupProbe.ExitCode -ne 0) {
            throw "The packaged app failed its hidden startup probe with exit code $($startupProbe.ExitCode)."
        }
    }
    finally { $startupProbe.Dispose() }
}

$payloadBytes = (Get-ChildItem -LiteralPath $publishDirectory -Recurse -File | Measure-Object Length -Sum).Sum
$estimatedSizeKiB = [math]::Ceiling($payloadBytes / 1024)
Write-Host 'Building the per-user Windows installer...'
& $NsisPath '/V2' '/NOCD' "/DVERSION=$Version" "/DVERSION_QUAD=$Version.0" "/DPROJECT_ROOT=$repositoryRoot" `
    "/DPUBLISH_DIR=$publishDirectory" "/DOUTPUT_FILE=$installerPath" "/DESTIMATED_SIZE_KB=$estimatedSizeKiB" $installerScript
if ($LASTEXITCODE -ne 0) { throw 'The Windows installer build failed.' }
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) { throw 'NSIS did not produce the expected installer.' }

$digest = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumPath = "$installerPath.sha256"
[IO.File]::WriteAllText($checksumPath, "$digest  $([IO.Path]::GetFileName($installerPath))`n", [Text.Encoding]::ASCII)
Write-Host "Installer: $installerPath"
Write-Host "SHA-256: $digest"
Write-Host 'Build complete. The installer was not launched. The hidden startup probe did not show a window or start the sync engine.'
