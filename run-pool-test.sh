#!/bin/bash

echo "🚀 开始运行MockPool测试..."

# 确保本地验证器正在运行
echo "🔧 检查本地验证器状态..."
if ! pgrep -f solana-test-validator > /dev/null; then
    echo "❌ 本地验证器未运行，正在启动..."
    solana-test-validator --reset --quiet &
    VALIDATOR_PID=$!
    echo "⏳ 等待验证器启动..."
    sleep 10
    NEED_CLEANUP=true
else
    echo "✅ 本地验证器已在运行"
    NEED_CLEANUP=false
fi

# 设置环境变量
export ANCHOR_PROVIDER_URL="http://127.0.0.1:8899"
export ANCHOR_WALLET="~/.config/solana/id.json"

echo "🔨 构建项目..."
anchor build

echo "🧪 运行MockPool测试..."
anchor test tests/mock-pool-test.ts --skip-local-validator

# 清理（如果我们启动了验证器）
if [ "$NEED_CLEANUP" = true ]; then
    echo "🧹 清理验证器进程..."
    kill $VALIDATOR_PID 2>/dev/null
fi

echo "✅ 测试完成!" 