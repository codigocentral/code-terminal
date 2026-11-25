param(
    [ValidateSet("windows", "linux", "macos", "all")]
    [string]$Target = "windows"
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $repoRoot

function Invoke-WindowsInstaller {
    & (Join-Path $PSScriptRoot "build-windows-installer.ps1")
    if ($LASTEXITCODE -ne 0) {
        throw "Falha ao gerar o instalador Windows."
    }
}

function Write-NativeOnlyMessage {
    param(
        [string]$Platform,
        [string]$Script
    )

    Write-Host "$Platform precisa ser gerado em uma maquina $Platform nativa."
    Write-Host "Comando nessa maquina:"
    Write-Host "  $Script"
}

switch ($Target) {
    "windows" {
        Invoke-WindowsInstaller
    }
    "linux" {
        Write-NativeOnlyMessage "Linux" "./scripts/build-linux-installer.sh"
    }
    "macos" {
        Write-NativeOnlyMessage "macOS" "./scripts/build-macos-installer.sh"
    }
    "all" {
        Invoke-WindowsInstaller
        Write-NativeOnlyMessage "Linux" "./scripts/build-linux-installer.sh"
        Write-NativeOnlyMessage "macOS" "./scripts/build-macos-installer.sh"
    }
}
