# Lance le serveur en local avec les jetons de .env.
#   .\tools\dev.ps1              (vraie navette)
#   .\tools\dev.ps1 --simulate   (navette fictive pour tester la page)

$root = Split-Path $PSScriptRoot -Parent
Get-Content (Join-Path $root ".env") | ForEach-Object {
    if ($_ -match '^\s*([A-Z_]+)\s*=\s*(.*)$') {
        [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2])
    }
}
Set-Location $root
cargo run --release -- config.toml @args
