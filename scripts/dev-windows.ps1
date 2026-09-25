# Run the shared POSIX scripts from PowerShell with Git Bash's full PATH.
# Examples: ./scripts/dev-windows.ps1 build --workspace --locked --offline
#           ./scripts/dev-windows.ps1 verify --gui
$ErrorActionPreference = 'Stop'
$trRoot = Split-Path $PSScriptRoot -Parent
$trGit = Get-Command git.exe -ErrorAction SilentlyContinue
$trBash = if ($trGit) {
    Join-Path (Split-Path (Split-Path $trGit.Source -Parent) -Parent) 'bin/bash.exe'
}
if (-not $trBash -or -not (Test-Path -LiteralPath $trBash)) {
    throw 'Install Git for Windows (including Git Bash) and add git.exe to PATH.'
}
Push-Location $trRoot
try {
    if ($args.Count -gt 0 -and $args[0] -eq 'verify') {
        $trArguments = @($args | Select-Object -Skip 1)
        & $trBash -l scripts/verify.sh @trArguments
    } else {
        & $trBash -l scripts/cargo-local.sh @args
    }
    $trExitCode = $LASTEXITCODE
} finally {
    Pop-Location
}
exit $trExitCode
