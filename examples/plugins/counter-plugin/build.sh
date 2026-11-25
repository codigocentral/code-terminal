#!/bin/bash
# Build script for counter-plugin (Unix)

set -e

echo "🔨 Building Counter Plugin..."

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if wasm32-wasi target is installed
if ! rustup target list --installed | grep -q "wasm32-wasi"; then
    echo -e "${YELLOW}⚠️  wasm32-wasi target not found. Installing...${NC}"
    rustup target add wasm32-wasi
fi

# Clean previous builds
echo "🧹 Cleaning previous builds..."
cargo clean

# Build release
echo "📦 Building release..."
cargo build --release --target wasm32-wasi

# Check if build succeeded
if [ -f "target/wasm32-wasi/release/counter-plugin.wasm" ]; then
    echo -e "${GREEN}✅ Build successful!${NC}"
    
    # Show file size
    SIZE=$(ls -lh target/wasm32-wasi/release/counter-plugin.wasm | awk '{print $5}')
    echo "📊 WASM file size: $SIZE"
    
    # Optimize with wasm-opt if available
    if command -v wasm-opt &> /dev/null; then
        echo "🚀 Optimizing with wasm-opt..."
        wasm-opt -O3 -o target/wasm32-wasi/release/counter-plugin-optimized.wasm \
            target/wasm32-wasi/release/counter-plugin.wasm
        
        OPT_SIZE=$(ls -lh target/wasm32-wasi/release/counter-plugin-optimized.wasm | awk '{print $5}')
        echo "📊 Optimized size: $OPT_SIZE"
    fi
    
    # Validate WASM
    if command -v wasm2wat &> /dev/null; then
        echo "🔍 Validating WASM..."
        wasm2wat target/wasm32-wasi/release/counter-plugin.wasm > /dev/null
        echo -e "${GREEN}✅ WASM validation passed${NC}"
    fi
    
    echo ""
    echo "📁 Output files:"
    echo "  - target/wasm32-wasi/release/counter-plugin.wasm"
    [ -f "target/wasm32-wasi/release/counter-plugin-optimized.wasm" ] && \
        echo "  - target/wasm32-wasi/release/counter-plugin-optimized.wasm"
    
    echo ""
    echo "📝 Next steps:"
    echo "  1. Copy counter-plugin.wasm to your plugins directory"
    echo "  2. Copy plugin.toml to the same directory"
    echo "  3. Restart Code Terminal or reload plugins"
    
else
    echo -e "${YELLOW}❌ Build failed${NC}"
    exit 1
fi
