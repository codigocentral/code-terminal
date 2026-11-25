# Instalador do Code Terminal para Windows (NSIS): compila em release e gera
# dist\CodeTerminal-<versao>-windows-x64-setup.exe (por usuario, sem admin)
# e dist\CodeTerminal-<versao>-windows-x64-portable.zip.
#
# O makensis vem do NSIS instalado ou do cache do NSIS que o build do Tauri
# (Scrutin) mantem no perfil do usuario; MAKENSIS aponta outro caminho.
# Antes era um .msi pelo WiX (dotnet); trocado por NSIS em 21/11/2026, sem
# instalar nada na VM de build.
#
# Este arquivo fica em ASCII: o PowerShell 5.1 le UTF-8 sem BOM como ANSI.
[CmdletBinding(DefaultParameterSetName = "Standalone")]
param(
    [switch]$SkipBuild,
    [Parameter(Mandatory, ParameterSetName = "Store")]
    [switch]$Store,
    [Parameter(Mandatory, ParameterSetName = "Store")]
    [ValidatePattern('^[a-zA-Z0-9.-]{3,50}$')]
    [string]$PackageName,
    [Parameter(Mandatory, ParameterSetName = "Store")]
    [ValidateNotNullOrEmpty()]
    [string]$PackagePublisher,
    [Parameter(Mandatory, ParameterSetName = "Store")]
    [ValidateNotNullOrEmpty()]
    [string]$PublisherDisplayName
)

$ErrorActionPreference = "Stop"

$repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$distDir = Join-Path $repoRoot "dist"
$nsiDir = Join-Path $repoRoot "packaging\windows"
$cargoToml = Join-Path $repoRoot "Cargo.toml"

function Invoke-Checked {
    param(
        [string]$FilePath,
        [string[]]$Arguments
    )

    & $FilePath @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Comando falhou com codigo ${LASTEXITCODE}: $FilePath $($Arguments -join ' ')"
    }
}

Set-Location $repoRoot

if ($Store) {
    $makeAppx = (Get-Command makeappx.exe -ErrorAction SilentlyContinue).Source
    if (-not $makeAppx) {
        $sdkBin = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
        $makeAppx = Get-ChildItem -Path "$sdkBin\10.*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
            Sort-Object { [version]$_.Directory.Parent.Name } -Descending |
            Select-Object -First 1 -ExpandProperty FullName
    }
    if (-not $makeAppx) {
        throw "MakeAppx.exe nao encontrado. Instale o Windows SDK ou use o Developer PowerShell."
    }
}

$cargoTomlContent = Get-Content -LiteralPath $cargoToml -Raw
if ($cargoTomlContent -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Nao foi possivel ler a versao do Cargo.toml."
}
$version = $Matches[1]

# -j 1 por padrao (LTO com codegen-units=1 gasta muita memoria por crate);
# CARGO_BUILD_JOBS troca (o package:windows usa 2 na VM de 16 GB).
if (-not $SkipBuild) {
    $jobs = if ($env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS } else { "1" }
    Invoke-Checked "cargo" @("build", "--release", "--bins", "-j", $jobs)
}

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repoRoot "target" }
$releaseDir = Join-Path $targetDir "release"
$exePath = Join-Path $releaseDir "code-terminal.exe"

function Get-PeSubsystem {
    param([string]$Path)
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ($peOffset -lt 0 -or $peOffset -gt $bytes.Length - 96) {
        return -1
    }
    $optionalHeaderOffset = $peOffset + 4 + 20
    $subsystemOffset = $optionalHeaderOffset + 0x44
    if ($subsystemOffset -gt $bytes.Length - 2) {
        return -1
    }
    return [BitConverter]::ToUInt16($bytes, $subsystemOffset)
}

$subsystem = Get-PeSubsystem -Path $exePath
# IMAGE_SUBSYSTEM_WINDOWS_GUI = 2, IMAGE_SUBSYSTEM_WINDOWS_CUI = 3
if ($subsystem -eq 3) {
    throw 'code-terminal.exe foi compilado como console subsystem. Verifique se #![windows_subsystem = "windows"] esta ativo para release.'
}
if ($subsystem -ne 2) {
    Write-Warning "Nao foi possivel confirmar o subsystem de code-terminal.exe (valor: $subsystem). Esperado 2 (GUI)."
}

New-Item -ItemType Directory -Force -Path $distDir | Out-Null

$exes = @("code-terminal.exe", "code-terminal-cli.exe", "code-terminal-browser.exe") |
    ForEach-Object { Join-Path $releaseDir $_ }
if ($Store) {
    $stage = Join-Path ([System.IO.Path]::GetTempPath()) ("code-terminal-msix-" + [guid]::NewGuid())
    $assets = Join-Path $stage "Assets"
    New-Item -ItemType Directory -Path $stage, $assets | Out-Null
    try {
        foreach ($exe in $exes) {
            $stream = [System.IO.File]::OpenRead($exe)
            $reader = [System.IO.BinaryReader]::new($stream)
            try {
                $stream.Position = 0x3C
                $stream.Position = $reader.ReadUInt32()
                if ($reader.ReadUInt32() -ne 0x00004550 -or $reader.ReadUInt16() -ne 0x8664) {
                    throw "O pacote x64 requer um executavel x64: $exe"
                }
            } finally {
                $reader.Dispose()
            }
        }
        Copy-Item -LiteralPath $exes -Destination $stage
        Copy-Item -LiteralPath @("LICENSE", "LICENSE-PLUGIN-EXCEPTION.md", "THIRD_PARTY_NOTICES.md") -Destination $stage
        $storeFiles = Join-Path $nsiDir "store"
        Copy-Item -Path (Join-Path $storeFiles "Logo*.png") -Destination $assets

        [xml]$manifest = Get-Content -LiteralPath (Join-Path $storeFiles "AppxManifest.xml") -Raw
        $manifest.Package.Identity.SetAttribute("Name", $PackageName)
        $manifest.Package.Identity.SetAttribute("Publisher", $PackagePublisher)
        $manifest.Package.Identity.SetAttribute("Version", "$version.0")
        $manifest.Package.Properties.PublisherDisplayName = $PublisherDisplayName
        $manifest.Save((Join-Path $stage "AppxManifest.xml"))

        $msix = Join-Path $distDir "CodeTerminal-$version-windows-x64.msix"
        Invoke-Checked $makeAppx @("pack", "/o", "/d", $stage, "/p", $msix)
        Write-Host "Pacote Microsoft Store gerado: $msix"
    } finally {
        Remove-Item -LiteralPath $stage -Recurse -Force
    }
    return
}

# Portatil: os tres .exe num .zip (sai mesmo se o makensis faltar).
$zip = Join-Path $distDir "CodeTerminal-$version-windows-x64-portable.zip"
Compress-Archive -Force -Path $exes -DestinationPath $zip
Write-Host "Portatil gerado: $zip"

# makensis: MAKENSIS, PATH, NSIS instalado, cache do Tauri, cache do cargo-packager.
$candidates = @()
if ($env:MAKENSIS) { $candidates += $env:MAKENSIS }
$inPath = Get-Command makensis -ErrorAction SilentlyContinue
if ($inPath) { $candidates += $inPath.Source }
$candidates += @(
    (Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"),
    (Join-Path $env:ProgramFiles "NSIS\makensis.exe"),
    (Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"),
    (Join-Path $env:LOCALAPPDATA "tauri\NSIS\Bin\makensis.exe"),
    (Join-Path $env:LOCALAPPDATA ".cargo-packager\NSIS\makensis.exe")
)
$makensis = $candidates | Where-Object { $_ -and (Test-Path -LiteralPath $_) } | Select-Object -First 1
if (-not $makensis) {
    throw "makensis nao encontrado (procurado em: $($candidates -join '; ')). Defina MAKENSIS."
}
Write-Host "makensis: $makensis"

$setup = Join-Path $distDir "CodeTerminal-$version-windows-x64-setup.exe"
Push-Location $nsiDir
try {
    Invoke-Checked $makensis @(
        "/V3",
        "/DVERSION=$version",
        "/DSOURCE_DIR=$releaseDir",
        "/DOUT_FILE=$setup",
        "code-terminal.nsi"
    )
} finally {
    Pop-Location
}
if (-not (Test-Path -LiteralPath $setup)) {
    throw "makensis terminou sem gerar $setup"
}
Write-Host "Instalador gerado: $setup"
