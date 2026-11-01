$ErrorActionPreference = "Stop"

$projectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$binaryNames = @("geo-server", "geo-data", "geo-export", "geo-stl", "geo-stl-layers")
$exitCode = 0

Push-Location $projectRoot
try {
    & cargo build --workspace --bins --release
    $exitCode = $LASTEXITCODE
}
finally {
    Pop-Location
}

if ($exitCode -ne 0) {
    exit $exitCode
}

$releaseDirectory = Join-Path $projectRoot "target\release"
foreach ($binaryName in $binaryNames) {
    $source = Join-Path $releaseDirectory "$binaryName.exe"
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Exécutable release absent : $source"
    }
    Copy-Item -LiteralPath $source -Destination $projectRoot -Force
}

Write-Host "Exécutables release copiés à la racine de $projectRoot :"
$binaryNames | ForEach-Object { Write-Host "  $_.exe" }
