# Public Windows installer; no credentials in download URLs or process arguments.
[CmdletBinding()]
param(
    [string]$Endpoint,
    [string]$InstallationId = 'default',
    [string]$Source = $(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }),
    [string]$Version = '0.2.0',
    [string]$InstallDir,
    [string]$Config,
    [string]$ReleaseBase,
    [switch]$NoStart,
    [switch]$Uninstall
)
$ErrorActionPreference = 'Stop'
if ($InstallationId -notmatch '^[a-zA-Z0-9._-]+$') { throw 'Invalid installation ID' }
if ($Version -notmatch '^\d+\.\d+\.\d+$') { throw 'Invalid release version' }
if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA "1flowbase\collectors\codex\$InstallationId" }
if (-not $Config) { $Config = Join-Path $InstallDir 'config.json' }
$BinaryPath = Join-Path $InstallDir 'bin\codex-logs-collector.exe'
$TaskName = "1flowbase-codex-logs-$InstallationId"
$User = [System.Security.Principal.WindowsIdentity]::GetCurrent()
# Include the current user's SID so independent user installations do not share a task.
$TaskName = "$TaskName-$($User.User.Value)"
if ($Uninstall) {
    $Task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($Task) {
        Stop-ScheduledTask -InputObject $Task
        Unregister-ScheduledTask -InputObject $Task -Confirm:$false
    }
    Remove-Item -LiteralPath $BinaryPath -Force -ErrorAction SilentlyContinue
    Write-Host 'Collector removed. Configuration, checkpoint and source logs retained.'
    return
}
if (-not $Endpoint) { throw '-Endpoint is required' }
if (-not $ReleaseBase) { throw '-ReleaseBase is required (copy the command from 1flowbase)' }
$ReleaseBase = $ReleaseBase.TrimEnd('/')
$ReleaseUri = [Uri]$ReleaseBase
if ($ReleaseUri.Scheme -notin @('https', 'http') -or $ReleaseUri.UserInfo -or $ReleaseUri.Query -or $ReleaseUri.Fragment) {
    throw 'Invalid public release URL'
}
$MachineArch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($MachineArch) { 'X64' { $Arch = 'amd64' } 'Arm64' { $Arch = 'arm64' } default { throw 'Unsupported architecture' } }
$ArchiveName = "codex-logs-collector-$Version-windows-$Arch.zip"
$Staging = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $Staging | Out-Null
$RestoreTask = $null
try {
    $Archive = Join-Path $Staging $ArchiveName
    Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -Uri "$ReleaseBase/$ArchiveName" -OutFile $Archive
    $ChecksumsPath = Join-Path $Staging 'checksums.txt'
    Invoke-WebRequest -UseBasicParsing -MaximumRedirection 0 -Uri "$ReleaseBase/checksums.txt" -OutFile $ChecksumsPath
    $Expected = @((Get-Content -LiteralPath $ChecksumsPath) | ForEach-Object {
        if ($_ -match '^([a-fA-F0-9]{64})\s+(.+)$' -and $Matches[2] -eq $ArchiveName) { $Matches[1] }
    })
    if ($Expected.Count -ne 1 -or (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash -ne $Expected[0]) {
        throw 'Release checksum mismatch or missing; existing installation retained'
    }
    # Extract only the selected executable, never arbitrary archive paths.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $Zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        $Entry = $Zip.GetEntry('codex-logs-collector.exe')
        if (-not $Entry) { throw 'Collector executable missing from release' }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($Entry, (Join-Path $Staging 'codex-logs-collector.exe'))
    } finally { $Zip.Dispose() }
    $NewBinary = Join-Path $Staging 'codex-logs-collector.exe'
    $BinaryVersion = & $NewBinary --version
    if ($LASTEXITCODE -ne 0 -or $BinaryVersion -ne "codex-logs-collector $Version") { throw 'Executable version differs from release' }
    New-Item -ItemType Directory -Path (Join-Path $InstallDir 'bin') -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path -Parent $Config) -Force | Out-Null
    & icacls.exe $InstallDir /inheritance:r /grant:r "*$($User.User.Value):(OI)(CI)F" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not make installation directory private' }
    # The config can be outside InstallDir; restrict its actual parent as well.
    & icacls.exe (Split-Path -Parent $Config) /inheritance:r /grant:r "*$($User.User.Value):(OI)(CI)F" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not make configuration directory private' }
    if ($env:FLOWBASE_AGENT_LOGS_API_KEY) {
        $CollectorKey = $env:FLOWBASE_AGENT_LOGS_API_KEY
    } else {
        $SecureKey = Read-Host 'Application API Key (saved only in local private config)' -AsSecureString
        $KeyPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($SecureKey)
        try { $CollectorKey = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($KeyPointer) }
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($KeyPointer) }
    }
    if (-not $CollectorKey) { throw 'API Key must not be empty' }
    $Task = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($Task -and $Task.State -eq 'Running') {
        Stop-ScheduledTask -InputObject $Task
        $RestoreTask = $Task
    }
    $CollectorKey | & $NewBinary configure --endpoint $Endpoint --source $Source --config $Config --key-stdin
    if ($LASTEXITCODE -ne 0) { throw 'Collector configuration failed; existing executable retained' }
    $CollectorKey = $null
    Copy-Item -LiteralPath $NewBinary -Destination $BinaryPath -Force
    if ($NoStart) {
        $RestoreTask = $null
        if ($Task) { Disable-ScheduledTask -InputObject $Task | Out-Null }
        Write-Host 'Collector configured. Background startup was explicitly disabled.'
        return
    }
    $Action = New-ScheduledTaskAction -Execute $BinaryPath -Argument ('watch --config "' + $Config + '"')
    $Trigger = New-ScheduledTaskTrigger -AtLogOn -User $User.Name
    $Principal = New-ScheduledTaskPrincipal -UserId $User.Name -LogonType Interactive -RunLevel Limited
    $Settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -MultipleInstances IgnoreNew
    Register-ScheduledTask -TaskName $TaskName -Action $Action -Trigger $Trigger -Principal $Principal -Settings $Settings -Force | Out-Null
    Start-ScheduledTask -TaskName $TaskName
    $RestoreTask = $null
    Write-Host 'Collector installed and started. It resumes after this user logs in. Source logs are read only.'
} finally {
    if ($RestoreTask) { Start-ScheduledTask -InputObject $RestoreTask }
    $CollectorKey = $null
    Remove-Item -LiteralPath $Staging -Recurse -Force
}
