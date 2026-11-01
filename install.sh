#!/usr/bin/env bash
# sconc v0.1 Installer & Environment Setup for Linux/macOS
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "================================================================"
echo "      sconc v0.1: 한국어 다층 말뭉치 통합 검색기 환경 설치기"
echo "================================================================"

# 1. Check Python
if command -v python3 >/dev/null 2>&1; then
    echo "[OK] Python 3 감지됨: $(python3 --version)"
elif command -v python >/dev/null 2>&1; then
    echo "[OK] Python 감지됨: $(python --version)"
else
    echo "[Warning] Python이 설치되어 있지 않습니다. 말뭉치 자동 변환기 사용을 위해 설치를 권장합니다."
fi

# 2. Check Rust
if ! command -v cargo >/dev/null 2>&1; then
    echo "[안내] Rust(Cargo) 툴체인이 감지되지 않아 rustup을 설치합니다..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi
echo "[OK] Cargo 감지됨: $(cargo --version)"

# 3. Build engine
echo "[*] sconc 릴리스 엔진 빌드 중..."
cargo build --release --manifest-path "engine/Cargo.toml"

# 4. Global symlink or wrapper
mkdir -p "$HOME/.local/bin"
cat << 'EOF' > "$HOME/.local/bin/sconc"
#!/usr/bin/env bash
SCRIPT_DIR="$(dirname "$(readlink -f "$0" 2>/dev/null || echo "$0")")"
export SCONC_HOME="@SCONC_HOME@"
exec "$SCONC_HOME/engine/target/release/sconc" --port 8787 --open "$@"
EOF
sed -i.bak "s|@SCONC_HOME@|$SCRIPT_DIR|g" "$HOME/.local/bin/sconc"
rm -f "$HOME/.local/bin/sconc.bak"
chmod +x "$HOME/.local/bin/sconc"

echo "================================================================"
echo "sconc v0.1 설치 완료! 터미널에서 'sconc'를 실행하세요."
echo "================================================================"
