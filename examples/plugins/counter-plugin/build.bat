@echo off
REM Build script for counter-plugin (Windows)

setlocal enabledelayedexpansion

echo 🔨 Building Counter Plugin...

REM Check if wasm32-wasi target is installed
rustup target list --installed | findstr "wasm32-wasi" >nul
if errorlevel 1 (
    echo ⚠️  wasm32-wasi target not found. Installing...
    rustup target add wasm32-wasi
)

REM Clean previous builds
echo 🧹 Cleaning previous builds...
cargo clean

REM Build release
echo 📦 Building release...
cargo build --release --target wasm32-wasi

REM Check if build succeeded
if exist "target\wasm32-wasi\release\counter-plugin.wasm" (
    echo ✅ Build successful!
    
    REM Show file size
    for %%F in ("target\wasm32-wasi\release\counter-plugin.wasm") do (
        echo 📊 WASM file size: %%~zF bytes
    )
    
    REM Optimize with wasm-opt if available
    where wasm-opt >nul 2>&1
    if !errorlevel! equ 0 (
        echo 🚀 Optimizing with wasm-opt...
        wasm-opt -O3 -o target\wasm32-wasi\release\counter-plugin-optimized.wasm ^
            target\wasm32-wasi\release\counter-plugin.wasm
        
        for %%F in ("target\wasm32-wasi\release\counter-plugin-optimized.wasm") do (
            echo 📊 Optimized size: %%~zF bytes
        )
    )
    
    REM Validate WASM if wasm2wat is available
    where wasm2wat >nul 2>&1
    if !errorlevel! equ 0 (
        echo 🔍 Validating WASM...
        wasm2wat target\wasm32-wasi\release\counter-plugin.wasm >nul 2>&1
        if !errorlevel! equ 0 (
            echo ✅ WASM validation passed
        ) else (
            echo ⚠️  WASM validation failed
        )
    )
    
    echo.
    echo 📁 Output files:
    echo   - target\wasm32-wasi\release\counter-plugin.wasm
    if exist "target\wasm32-wasi\release\counter-plugin-optimized.wasm" (
        echo   - target\wasm32-wasi\release\counter-plugin-optimized.wasm
    )
    
    echo.
    echo 📝 Next steps:
    echo   1. Copy counter-plugin.wasm to your plugins directory
    echo   2. Copy plugin.toml to the same directory
    echo   3. Restart Code Terminal or reload plugins
    
) else (
    echo ❌ Build failed
    exit /b 1
)

endlocal
