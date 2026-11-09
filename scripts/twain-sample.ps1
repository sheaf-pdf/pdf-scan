<#
.SYNOPSIS
Installs, checks or removes the TWAIN Working Group's sample data source and the
TWAIN Data Source Manager (TWAINDSM.dll) that comes with it.

.DESCRIPTION
The sample data source is a virtual scanner that reads its images from disk, so TWAIN
code can run without a scanner, on a development machine or a CI runner.

Install downloads the TWAIN Working Group's sample release 2.5.0 (pinned by URL and
SHA-256) into -CacheDir, takes its 64-bit data source installer out of it, and runs that
installer silently. It installs both bitnesses:

  TWAINDSM.dll 2.5.0     64-bit in System32, 32-bit in SysWOW64
  TWAINDS_Sample64.ds    %WINDIR%\twain_64\sample2
  TWAINDS_Sample32.ds    %WINDIR%\twain_32\sample2

Windows Installer keeps a newer TWAINDSM.dll if one is installed already. The installer
itself isn't signed, so the pinned hashes are what vouch for it.

Install and Uninstall need administrator rights. In a shell that isn't elevated, only
msiexec is elevated, through a UAC prompt; an elevated shell (a CI runner) gets no prompt.

Uninstall removes the sample source and the DSM, unless another installed product shares
the DSM's Windows Installer component. On a machine with scanner drivers, check Status
afterwards.

The sample source feeds 2 pages per batch unless the scanning process's environment sets
CAP_XFERCOUNT to a page count; 0 or less gives a random count from 0 to 14. Its own
README says 1; its code says 2.

.PARAMETER Action
Download: fetch and check the installer, and stop.
Install: download if needed, install, then report as Status does.
Uninstall: remove what Install installed, then report what's left.
Status: report the DSMs and sample sources found, and each DSM's signature. Exits 1 if
either DSM or either source is missing, or a DSM's signature isn't valid.

.PARAMETER CacheDir
Where the download and msiexec's logs go. Defaults to a folder in the temp directory,
outside the repository.

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts\twain-sample.ps1 Install

.EXAMPLE
powershell -ExecutionPolicy Bypass -File scripts\twain-sample.ps1 Status
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [ValidateSet('Download', 'Install', 'Uninstall', 'Status')]
    [string] $Action,

    [string] $CacheDir = (Join-Path ([System.IO.Path]::GetTempPath()) 'twain-sample-2.5.0')
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'
# One full path for every caller: PowerShell's cmdlets resolve a relative path
# against its location, .NET and msiexec against the process's directory, and
# the two differ once a session has changed directory.
$CacheDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CacheDir)
# Windows PowerShell's Invoke-WebRequest is many times slower while it draws a progress bar.
$ProgressPreference = 'SilentlyContinue'

$Release = [pscustomobject]@{
    Url         = 'https://github.com/twain/twain-samples/releases/download/v2.5.0/Twain_sample01_02050000.zip'
    ZipName     = 'Twain_sample01_02050000.zip'
    ZipSha256   = 'EB08A690C887C2CE33EF4A6240E2168B6CC9F2CC1B4EB754961CC06CB64661EF'
    MsiEntry    = 'Twain_sample01_02050000/twainds.win64.installer.msi'
    MsiName     = 'twainds.win64.installer.msi'
    MsiSha256   = 'B25D679A36005A84C749C0D510ACC78D4AA193AD47D8875DD38E332F6759A235'
    ProductCode = '{FA2E386F-BAB5-4625-8322-20AC986E4D99}'
}

# The official DSM's Authenticode signer; its certificate changes between releases.
$DsmSigner = 'TWAIN Working Group'

function Assert-Platform {
    if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) {
        throw 'This script installs a Windows TWAIN data source; run it on Windows.'
    }
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw 'The sample installer is for 64-bit Windows.'
    }
}

function Test-Elevated {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal $identity
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-ExpectedFiles {
    $win = $env:WINDIR
    # A 32-bit PowerShell sees SysWOW64 under the name System32; Sysnative is the real one.
    $system64 = if ([Environment]::Is64BitProcess) { Join-Path $win 'System32' } else { Join-Path $win 'Sysnative' }
    @(
        [pscustomobject]@{ Name = 'DSM, 64-bit';           IsDsm = $true;  Path = (Join-Path $system64 'TWAINDSM.dll') }
        [pscustomobject]@{ Name = 'DSM, 32-bit';           IsDsm = $true;  Path = (Join-Path $win 'SysWOW64\TWAINDSM.dll') }
        [pscustomobject]@{ Name = 'sample source, 64-bit'; IsDsm = $false; Path = (Join-Path $win 'twain_64\sample2\TWAINDS_Sample64.ds') }
        [pscustomobject]@{ Name = 'sample source, 32-bit'; IsDsm = $false; Path = (Join-Path $win 'twain_32\sample2\TWAINDS_Sample32.ds') }
    )
}

function Test-Sha256([string] $Path, [string] $Expected) {
    return (Test-Path -LiteralPath $Path) -and ((Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash -eq $Expected)
}

function Assert-Sha256([string] $Path, [string] $Expected, [string] $What) {
    $hash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
    if ($hash -ne $Expected) {
        Remove-Item -LiteralPath $Path
        throw "The $What's SHA-256 is $hash, not the pinned $Expected."
    }
}

# Returns the path of the checked installer, downloading and extracting it if the cache lacks it.
function Save-Installer {
    New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null
    $msi = Join-Path $CacheDir $Release.MsiName
    if (Test-Sha256 $msi $Release.MsiSha256) {
        Write-Host "Installer already downloaded and checked: $msi"
        return $msi
    }

    $zip = Join-Path $CacheDir $Release.ZipName
    if (-not (Test-Sha256 $zip $Release.ZipSha256)) {
        $partial = "$zip.partial"
        Write-Host "Downloading $($Release.Url)"
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -UseBasicParsing -Uri $Release.Url -OutFile $partial
        Assert-Sha256 $partial $Release.ZipSha256 'download'
        Move-Item -Force -LiteralPath $partial -Destination $zip
    }

    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $partial = "$msi.partial"
    $archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
    try {
        $entry = $archive.GetEntry($Release.MsiEntry)
        if ($null -eq $entry) { throw "$($Release.MsiEntry) isn't in $zip." }
        [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $partial, $true)
    } finally {
        $archive.Dispose()
    }
    Assert-Sha256 $partial $Release.MsiSha256 'extracted installer'
    Move-Item -Force -LiteralPath $partial -Destination $msi
    Remove-Item -LiteralPath $zip
    Write-Host "Installer downloaded and checked: $msi"
    return $msi
}

function Test-ProductInstalled {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $state = $installer.GetType().InvokeMember('ProductState', [Reflection.BindingFlags]::GetProperty, $null, $installer, @($Release.ProductCode))
    return ($state -eq 5)  # msiInstallStateDefault: installed
}

# Runs msiexec silently with a verbose log, elevating through UAC if this shell isn't elevated.
function Invoke-Msiexec([string] $Arguments, [string] $Log) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = 'msiexec.exe'
    $psi.Arguments = "$Arguments /qn /norestart /l*v `"$Log`""
    if (Test-Elevated) {
        $psi.UseShellExecute = $false
    } else {
        Write-Host 'msiexec needs administrator rights: accept the UAC prompt.'
        $psi.UseShellExecute = $true
        $psi.Verb = 'runas'
    }
    Write-Host "msiexec $($psi.Arguments)"
    try {
        $process = [System.Diagnostics.Process]::Start($psi)
    } catch {
        throw "msiexec didn't start: $($_.Exception.Message)"
    }
    $process.WaitForExit()
    return $process.ExitCode
}

function Assert-MsiexecSucceeded([int] $Code, [string] $Log) {
    switch ($Code) {
        0       { return }
        3010    { Write-Warning 'msiexec succeeded and asks for a restart.'; return }
        1602    { throw 'The installation was cancelled (msiexec 1602).' }
        1618    { throw 'Another installation is in progress (msiexec 1618); try again when it finishes.' }
        1925    { throw "msiexec lacked administrator rights (1925). Log: $Log" }
        default { throw "msiexec failed with exit code $Code. Log: $Log" }
    }
}

# Prints what's installed; returns $true if both DSMs and both sources are present and both
# DSMs' signatures are valid.
function Show-Status {
    $ok = $true
    if (Test-ProductInstalled) { $state = 'installed' } else { $state = 'not installed' }
    Write-Host "Sample installer $($Release.ProductCode): $state"
    foreach ($file in (Get-ExpectedFiles)) {
        if (-not (Test-Path -LiteralPath $file.Path)) {
            Write-Host ('  missing  {0,-22} {1}' -f $file.Name, $file.Path)
            $ok = $false
            continue
        }
        $v = (Get-Item -LiteralPath $file.Path).VersionInfo
        $version = '{0}.{1}.{2}.{3}' -f $v.FileMajorPart, $v.FileMinorPart, $v.FileBuildPart, $v.FilePrivatePart
        $note = ''
        if ($file.IsDsm) {
            $sig = Get-AuthenticodeSignature -LiteralPath $file.Path
            $subject = ''
            if ($null -ne $sig.SignerCertificate) { $subject = $sig.SignerCertificate.Subject }
            if ($sig.Status -ne 'Valid') {
                $note = "signature: $($sig.Status)"
                $ok = $false
            } elseif ($subject -match ('(^|, )O=' + [regex]::Escape($DsmSigner) + '(,|$)')) {
                $note = "signed by $DsmSigner"
            } else {
                $note = "signed by: $subject"
                Write-Warning "$($file.Path) has a valid signature, but not $DsmSigner's."
            }
        }
        Write-Host ('  found    {0,-22} {1,-9} {2}  {3}' -f $file.Name, $version, $file.Path, $note)
    }
    return $ok
}

function Install-Sample {
    if (Test-ProductInstalled) {
        Write-Host 'The sample installer is installed already; run Uninstall first to reinstall.'
    } else {
        $msi = Save-Installer
        $log = Join-Path $CacheDir 'install.log'
        $code = Invoke-Msiexec "/i `"$msi`" ALLUSERS=1" $log
        Assert-MsiexecSucceeded $code $log
    }
    return (Show-Status)
}

function Uninstall-Sample {
    if (-not (Test-ProductInstalled)) {
        Write-Host 'The sample installer is not installed.'
    } else {
        New-Item -ItemType Directory -Force -Path $CacheDir | Out-Null
        $log = Join-Path $CacheDir 'uninstall.log'
        $code = Invoke-Msiexec "/x $($Release.ProductCode)" $log
        Assert-MsiexecSucceeded $code $log
    }
    [void](Show-Status)
}

Assert-Platform
switch ($Action) {
    'Download'  { [void](Save-Installer) }
    'Install'   { if (-not (Install-Sample)) { exit 1 } }
    'Uninstall' { Uninstall-Sample }
    'Status'    { if (-not (Show-Status)) { exit 1 } }
}
