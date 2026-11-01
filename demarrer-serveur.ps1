$ErrorActionPreference = "Stop"

$serverRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$executable = [System.IO.Path]::GetFullPath((Join-Path $serverRoot "target\debug\geo-server.exe"))
$pidFile = Join-Path $serverRoot ".geo-server.pid"

function Get-ManagedServerProcess {
    Get-Process -Name "geo-server" -ErrorAction SilentlyContinue | Where-Object {
        try {
            [System.IO.Path]::GetFullPath($_.Path) -ieq $executable
        }
        catch {
            $false
        }
    }
}

$existing = @(Get-ManagedServerProcess)
if ($existing.Count -gt 0) {
    $existing[0].Id | Set-Content -LiteralPath $pidFile -Encoding ascii
    Write-Host "Le serveur Geo Data est déjà démarré."
    Write-Host "Pour l'arrêter : double-cliquez sur arreter-serveur.cmd"
    exit 0
}

if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
    Push-Location $serverRoot
    try {
        & cargo build --bin geo-server
        if ($LASTEXITCODE -ne 0) {
            throw "La compilation de geo-server a échoué."
        }
    }
    finally {
        Pop-Location
    }
}

$process = Start-Process `
    -FilePath $executable `
    -ArgumentList @("--config", "geo.toml") `
    -WorkingDirectory $serverRoot `
    -WindowStyle Hidden `
    -RedirectStandardOutput (Join-Path $serverRoot "server.stdout.log") `
    -RedirectStandardError (Join-Path $serverRoot "server.stderr.log") `
    -PassThru

$process.Id | Set-Content -LiteralPath $pidFile -Encoding ascii
Start-Sleep -Milliseconds 400
if ($process.HasExited) {
    Remove-Item -LiteralPath $pidFile -Force -ErrorAction SilentlyContinue
    throw "Le serveur s'est arrêté au démarrage. Consultez server.stderr.log."
}

Write-Host "Serveur Geo Data démarré sur http://127.0.0.1:8080/"
Write-Host "Pour l'arrêter : double-cliquez sur arreter-serveur.cmd"
