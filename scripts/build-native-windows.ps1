[CmdletBinding()]
param(
    [ValidatePattern('^\d+\.\d+\.\d+$')]
    [string]$Version = '1.6.3',
    [ValidateSet('x64', 'arm64')]
    [string]$Architecture = 'x64',
    [string]$EnginePath,
    [string]$NsisPath,
    [string]$CrtDirectory,
    [string]$CargoTargetDirectory,
    [switch]$SkipStartupProbe
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-PeArchitecture {
    param([Parameter(Mandatory)][string]$Path)
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::ReadWrite)
    try {
        $reader = [IO.BinaryReader]::new($stream)
        if ($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5A4D) { return 'unknown' }
        $stream.Position = 0x3C
        $headerOffset = $reader.ReadInt32()
        if ($headerOffset -lt 0 -or $headerOffset -gt $stream.Length - 6) { return 'unknown' }
        $stream.Position = $headerOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { return 'unknown' }
        switch ($reader.ReadUInt16()) {
            0x8664 { return 'x64' }
            0xAA64 { return 'arm64' }
            default { return 'unknown' }
        }
    } finally {
        $stream.Dispose()
    }
}

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw 'Build the native Windows installer on Windows.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$artifactRoot = Join-Path $repositoryRoot 'artifacts'
$publishDirectory = [IO.Path]::GetFullPath((Join-Path $artifactRoot "native-win-$Architecture"))
$projectPath = Join-Path $repositoryRoot 'native\windows\SpiceRoute.Windows\SpiceRoute.Windows.csproj'
$installerScript = Join-Path $repositoryRoot 'native\windows\installer.nsi'
$coreManifest = Join-Path $repositoryRoot 'src-tauri\core\Cargo.toml'
$target = if ($Architecture -eq 'arm64') {
    @{
        Rust = 'aarch64-pc-windows-msvc'
        DotNet = 'win-arm64'
        Platform = 'ARM64'
        Crt = 'arm64'
        VsComponent = 'Microsoft.VisualStudio.Component.VC.Tools.ARM64'
        MsvcScript = 'msvc-env-arm64.cmd'
    }
} else {
    @{
        Rust = 'x86_64-pc-windows-msvc'
        DotNet = 'win-x64'
        Platform = 'x64'
        Crt = 'x64'
        VsComponent = 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64'
        MsvcScript = 'msvc-env.cmd'
    }
}
$msvcScript = Join-Path $PSScriptRoot $target.MsvcScript
$installerPath = Join-Path $artifactRoot "Spice-Route-$Version-windows-$Architecture-setup.exe"
$rustTarget = $target.Rust

foreach ($requiredPath in @($projectPath, $installerScript, $msvcScript)) {
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
$engineArchitecture = Get-PeArchitecture -Path $EnginePath
if ($engineArchitecture -ne $Architecture) {
    throw "The sync engine architecture is $engineArchitecture, but this package targets $Architecture."
}

# This exact generated directory is the only recursive removal in this script.
# Reject links and check its resolved parent before clearing a previous publish.
$expectedPublishDirectory = Join-Path ([IO.Path]::GetFullPath($artifactRoot)) "native-win-$Architecture"
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
& $dotnetCommand.Source publish $projectPath --configuration Release --runtime $target.DotNet --self-contained true --output $publishDirectory `
    "-p:Version=$Version" "-p:Platform=$($target.Platform)" '-p:WindowsPackageType=None' '-p:WindowsAppSDKSelfContained=true' `
    '-p:WindowsAppSdkDeploymentManagerInitialize=false' '-p:PublishSingleFile=false' '-p:PublishTrimmed=false'
if ($LASTEXITCODE -ne 0) { throw 'The native Windows app publish failed.' }
Copy-Item -LiteralPath $EnginePath -Destination (Join-Path $publishDirectory 'SpiceRoute.Engine.exe') -Force

# App-local CRT files allow the Rust engine and native UI libraries to start on a
# clean PC without a separate Visual C++ redistributable installer.
if (-not $CrtDirectory) {
    $redistVersionRoots = [Collections.Generic.List[string]]::new()
    if ($env:VCToolsRedistDir -and (Test-Path -LiteralPath $env:VCToolsRedistDir -PathType Container)) {
        $redistVersionRoots.Add([IO.Path]::GetFullPath($env:VCToolsRedistDir))
    }

    $visualStudioInstallations = [Collections.Generic.List[string]]::new()
    if ($env:VSINSTALLDIR -and (Test-Path -LiteralPath $env:VSINSTALLDIR -PathType Container)) {
        $visualStudioInstallations.Add([IO.Path]::GetFullPath($env:VSINSTALLDIR))
    }

    $vswhereCandidates = @(
        $(if (${env:ProgramFiles(x86)}) { Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe' }),
        $(if ($env:ProgramFiles) { Join-Path $env:ProgramFiles 'Microsoft Visual Studio\Installer\vswhere.exe' })
    )
    $vswherePath = $vswhereCandidates |
        Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } |
        Select-Object -First 1
    if ($vswherePath) {
        $locatedInstallations = & $vswherePath -all -products '*' -requires $target.VsComponent -property installationPath
        if ($LASTEXITCODE -ne 0) { throw 'Visual Studio discovery failed while locating the C++ runtime.' }
        foreach ($installation in $locatedInstallations) {
            if ($installation -and (Test-Path -LiteralPath $installation -PathType Container)) {
                $visualStudioInstallations.Add([IO.Path]::GetFullPath($installation))
            }
        }
    }

    # Preserve compatibility with machines that have the original VS 2022
    # Build Tools layout but no usable vswhere installation.
    if (${env:ProgramFiles(x86)}) {
        $legacyInstallation = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\2022\BuildTools'
        if (Test-Path -LiteralPath $legacyInstallation -PathType Container) {
            $visualStudioInstallations.Add([IO.Path]::GetFullPath($legacyInstallation))
        }
    }

    foreach ($installation in ($visualStudioInstallations | Select-Object -Unique)) {
        $redistRoot = Join-Path $installation 'VC\Redist\MSVC'
        if (-not (Test-Path -LiteralPath $redistRoot -PathType Container)) { continue }
        Get-ChildItem -LiteralPath $redistRoot -Directory |
            Where-Object { $_.Name -match '^\d+\.\d+\.\d+$' } |
            Sort-Object { [version]$_.Name } -Descending |
            ForEach-Object { $redistVersionRoots.Add($_.FullName) }
    }

    foreach ($versionRoot in ($redistVersionRoots | Select-Object -Unique)) {
        $candidate = Get-ChildItem -Path (Join-Path $versionRoot "$($target.Crt)\Microsoft.VC*.CRT") -Directory -ErrorAction SilentlyContinue |
            Select-Object -First 1
        if ($candidate) {
            $CrtDirectory = $candidate.FullName
            break
        }
    }
}
if (-not $CrtDirectory -or -not (Test-Path -LiteralPath $CrtDirectory -PathType Container)) {
    throw "The Visual C++ $Architecture CRT redistribution folder was not found. Pass -CrtDirectory from your Visual Studio installation."
}
$crtFiles = @(Get-ChildItem -LiteralPath $CrtDirectory -File -Filter '*.dll')
if ($crtFiles.Count -eq 0) {
    throw "The Visual C++ $Architecture CRT redistribution folder contains no DLLs."
}
foreach ($crtFile in $crtFiles) {
    $crtArchitecture = Get-PeArchitecture -Path $crtFile.FullName
    # The current VC ARM64 redist includes an x64-only vcruntime140_1.dll even
    # though ARM64 .NET and WinUI payloads do not import it. Never carry that
    # wrong-architecture, unused file into the app directory.
    if ($Architecture -eq 'arm64' -and
        $crtFile.Name -eq 'vcruntime140_1.dll' -and
        $crtArchitecture -eq 'x64') {
        $staleCrt = Join-Path $publishDirectory $crtFile.Name
        if (Test-Path -LiteralPath $staleCrt -PathType Leaf) {
            [IO.File]::Delete($staleCrt)
        }
        continue
    }
    if ($crtArchitecture -ne $Architecture) {
        throw "The selected Visual C++ runtime contains $($crtFile.Name) for $crtArchitecture, but this package targets $Architecture."
    }
    # Architecture takes precedence over file version. dotnet publish can leave
    # a same-version CRT from the build host in a cross-architecture output.
    Copy-Item -LiteralPath $crtFile.FullName -Destination (Join-Path $publishDirectory $crtFile.Name) -Force
}

$requiredFiles = @('SpiceRoute.exe', 'SpiceRoute.Engine.exe', 'SpiceRoute.runtimeconfig.json', 'SpiceRoute.pri', 'App.xbf', 'MainWindow.xbf', 'coreclr.dll', 'hostfxr.dll', 'Microsoft.UI.Xaml.dll', 'vcruntime140.dll', 'msvcp140.dll')
if ($Architecture -eq 'x64') { $requiredFiles += 'vcruntime140_1.dll' }
foreach ($requiredFile in $requiredFiles) {
    if (-not (Test-Path -LiteralPath (Join-Path $publishDirectory $requiredFile) -PathType Leaf)) {
        throw "The self-contained package is missing $requiredFile. The installer was not produced."
    }
}
$packagedCrtFiles = @(Get-ChildItem -LiteralPath $publishDirectory -File | Where-Object {
    $_.Name -match '^(vcruntime|msvcp|concrt|vccorlib)140.*\.dll$'
})
foreach ($crtFile in $packagedCrtFiles) {
    $crtArchitecture = Get-PeArchitecture -Path $crtFile.FullName
    if ($crtArchitecture -ne $Architecture) {
        throw "The packaged Visual C++ runtime $($crtFile.Name) is $crtArchitecture, but this package targets $Architecture. The installer was not produced."
    }
}
$appArchitecture = Get-PeArchitecture -Path (Join-Path $publishDirectory 'SpiceRoute.exe')
if ($appArchitecture -ne $Architecture) {
    throw "The Windows app architecture is $appArchitecture, but this package targets $Architecture. The installer was not produced."
}

$hostArchitecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
$canRunTarget = $hostArchitecture -eq $Architecture
if (-not $SkipStartupProbe -and $canRunTarget) {
    Write-Host 'Starting the packaged sync engine with disposable data...'
    $temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    $engineProbeDirectory = [IO.Path]::GetFullPath((Join-Path $temporaryRoot "SpiceRoute.EngineProbe.$([guid]::NewGuid().ToString('N'))"))
    if (-not $engineProbeDirectory.StartsWith($temporaryRoot, [StringComparison]::OrdinalIgnoreCase) -or
        -not [IO.Path]::GetFileName($engineProbeDirectory).StartsWith('SpiceRoute.EngineProbe.', [StringComparison]::Ordinal)) {
        throw 'The disposable engine-probe directory could not be confirmed.'
    }
    New-Item -ItemType Directory -Path $engineProbeDirectory | Out-Null
    $engineProbe = $null
    try {
        $engineProbeInfo = [Diagnostics.ProcessStartInfo]::new((Join-Path $publishDirectory 'SpiceRoute.Engine.exe'))
        $engineProbeInfo.UseShellExecute = $false
        $engineProbeInfo.CreateNoWindow = $true
        $engineProbeInfo.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
        $engineProbeInfo.RedirectStandardInput = $true
        $engineProbeInfo.RedirectStandardOutput = $true
        $engineProbeInfo.RedirectStandardError = $true
        $engineProbeInfo.WorkingDirectory = $publishDirectory
        $engineProbeInfo.ArgumentList.Add('--data-dir')
        $engineProbeInfo.ArgumentList.Add($engineProbeDirectory)
        $engineProbe = [Diagnostics.Process]::Start($engineProbeInfo)
        if (-not $engineProbe) { throw 'The packaged sync engine could not start.' }
        $engineProbe.StandardInput.WriteLine('{"id":"startup","method":"get_protocol_info","params":{}}')
        $engineProbe.StandardInput.Flush()
        $responseTask = $engineProbe.StandardOutput.ReadLineAsync()
        if (-not $responseTask.Wait(15000)) {
            $engineProbe.Kill($true)
            throw 'The packaged sync engine did not answer its startup check within 15 seconds.'
        }
        $engineOutput = $responseTask.Result
        # Keep stdin open until the worker has returned its response. Closing it
        # earlier asks the engine to cancel outstanding work and can race a fast
        # one-request probe on slower ARM computers.
        $engineProbe.StandardInput.Close()
        if (-not $engineProbe.WaitForExit(15000)) {
            $engineProbe.Kill($true)
            throw 'The packaged sync engine did not stop after its startup check.'
        }
        $engineError = $engineProbe.StandardError.ReadToEnd()
        if ($engineProbe.ExitCode -ne 0) {
            throw "The packaged sync engine failed its startup check with exit code $($engineProbe.ExitCode): $engineError"
        }
        if ([string]::IsNullOrWhiteSpace($engineOutput)) {
            throw "The packaged sync engine returned no startup response: $engineError"
        }
        $engineResponse = $engineOutput | ConvertFrom-Json -ErrorAction Stop
        $resultProperty = $engineResponse.PSObject.Properties['result']
        $result = if ($resultProperty) { $resultProperty.Value } else { $null }
        if ($engineResponse.id -ne 'startup' -or -not $result -or $result.protocolVersion -ne 1) {
            $errorProperty = $engineResponse.PSObject.Properties['error']
            $engineMessage = if ($errorProperty -and $errorProperty.Value) {
                $messageProperty = $errorProperty.Value.PSObject.Properties['message']
                if ($messageProperty) { $messageProperty.Value } else { $null }
            } else {
                $null
            }
            if (-not $engineMessage) { $engineMessage = 'The response did not contain protocol version 1.' }
            throw "The packaged sync engine returned an invalid startup response: $engineMessage"
        }
    } finally {
        if ($engineProbe) { $engineProbe.Dispose() }
        if (Test-Path -LiteralPath $engineProbeDirectory) {
            Remove-Item -LiteralPath $engineProbeDirectory -Recurse -Force
        }
    }

    Write-Host 'Running the packaged app with a disposable profile in a hidden full startup probe...'
    $appExecutable = Join-Path $publishDirectory 'SpiceRoute.exe'
    $startupProbe = Start-Process -FilePath $appExecutable -ArgumentList '--startup-probe' `
        -WorkingDirectory $publishDirectory -WindowStyle Hidden -PassThru
    try {
        if (-not $startupProbe.WaitForExit(30000)) {
            $startupProbe.Kill($true)
            throw 'The packaged app did not finish its hidden full startup probe within 30 seconds.'
        }
        $startupProbe.Refresh()
        if ($startupProbe.ExitCode -ne 0) {
            throw "The packaged app failed its hidden startup probe with exit code $($startupProbe.ExitCode)."
        }
    }
    finally { $startupProbe.Dispose() }
} elseif (-not $SkipStartupProbe) {
    Write-Host "Skipping the packaged startup probe because this $hostArchitecture build computer cannot run the $Architecture app natively."
}

$payloadBytes = (Get-ChildItem -LiteralPath $publishDirectory -Recurse -File | Measure-Object Length -Sum).Sum
$estimatedSizeKiB = [math]::Ceiling($payloadBytes / 1024)
Write-Host 'Building the per-user Windows installer...'
& $NsisPath '/V2' '/NOCD' "/DVERSION=$Version" "/DVERSION_QUAD=$Version.0" "/DARCHITECTURE=$Architecture" "/DPROJECT_ROOT=$repositoryRoot" `
    "/DPUBLISH_DIR=$publishDirectory" "/DOUTPUT_FILE=$installerPath" "/DESTIMATED_SIZE_KB=$estimatedSizeKiB" $installerScript
if ($LASTEXITCODE -ne 0) { throw 'The Windows installer build failed.' }
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) { throw 'NSIS did not produce the expected installer.' }

$digest = (Get-FileHash -LiteralPath $installerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$checksumPath = "$installerPath.sha256"
[IO.File]::WriteAllText($checksumPath, "$digest  $([IO.Path]::GetFileName($installerPath))`n", [Text.Encoding]::ASCII)
Write-Host "Installer: $installerPath"
Write-Host "SHA-256: $digest"
if ($SkipStartupProbe) {
    Write-Host 'Build complete. The installer and app were not launched.'
} else {
    Write-Host 'Build complete. The installer was not launched. Startup checks were hidden and used only disposable data.'
}
