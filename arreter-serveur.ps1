$ErrorActionPreference = "Stop"

$serverRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$executable = [System.IO.Path]::GetFullPath((Join-Path $serverRoot "target\debug\geo-server.exe"))
$pidFile = Join-Path $serverRoot ".geo-server.pid"

function Is-ManagedServerProcess($process) {
    if ($null -eq $process) {
        return $false
    }
    try {
        return [System.IO.Path]::GetFullPath($process.Path) -ieq $executable
    }
    catch {
        return $false
    }
}

$managed = @()
if (Test-Path -LiteralPath $pidFile -PathType Leaf) {
    $savedPid = (Get-Content -Raw -LiteralPath $pidFile).Trim()
    if ($savedPid -match '^\d+$') {
        $savedProcess = Get-Process -Id ([int]$savedPid) -ErrorAction SilentlyContinue
        if (Is-ManagedServerProcess $savedProcess) {
            $managed += $savedProcess
        }
    }
}

if ($managed.Count -eq 0) {
    $managed = @(Get-Process -Name "geo-server" -ErrorAction SilentlyContinue | Where-Object {
        Is-ManagedServerProcess $_
    })
}

if ($managed.Count -eq 0) {
    Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
    Write-Host "Le serveur Geo Data est déjà arrêté."
    exit 0
}

foreach ($process in $managed) {
    # /T arrête également le moteur geo-data/geo-export éventuellement lancé par ce serveur.
    & taskkill.exe /PID $process.Id /T /F | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw "Impossible d'arrêter le serveur Geo Data."
    }
}

Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
Write-Host "Serveur Geo Data arrêté."
