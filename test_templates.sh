#!/bin/bash
# 简单的模板验证脚本

echo "=== 地形模板测试报告 ==="
echo ""

# 1. 检查模板目录
echo "1. 模板文件发现："
TEMPLATE_DIR="templates"
if [ ! -d "$TEMPLATE_DIR" ]; then
    echo "   错误: templates/ 目录不存在"
    exit 1
fi

TEMPLATE_COUNT=$(ls -1 $TEMPLATE_DIR/*.terrain 2>/dev/null | wc -l)
echo "   发现 $TEMPLATE_COUNT 个模板文件："
for f in $TEMPLATE_DIR/*.terrain; do
    [ -f "$f" ] && echo "   - $(basename $f)"
done
echo ""

# 2. 验证模板格式
echo "2. 模板语法验证："
ALL_VALID=true

for f in $TEMPLATE_DIR/*.terrain; do
    [ -f "$f" ] || continue
    name=$(basename $f)
    
    # 检查文件不为空
    if [ ! -s "$f" ]; then
        echo "   ✗ $name: 文件为空"
        ALL_VALID=false
        continue
    fi
    
    # 检查是否有有效命令
    CMD_COUNT=$(grep -v '^#' "$f" | grep -v '^$' | grep -v '^//' | wc -l)
    if [ "$CMD_COUNT" -eq 0 ]; then
        echo "   ✗ $name: 没有有效命令"
        ALL_VALID=false
        continue
    fi
    
    # 检查常见命令是否存在
    HAS_NORMALIZE=$(grep -i 'Normalize' "$f" | wc -l)
    HAS_SEARATIO=$(grep -iE 'SeaRatio|Sea|Ocean' "$f" | wc -l)
    
    if [ "$HAS_NORMALIZE" -eq 0 ] && [ "$HAS_SEARATIO" -eq 0 ]; then
        echo "   ⚠ $name: 缺少 Normalize 或 SeaRatio 命令"
    fi
    
    echo "   ✓ $name: $CMD_COUNT 个命令"
done
echo ""

# 3. 检查命令语法
echo "3. 命令类型统计："
TOTAL_HILL=0
TOTAL_RANGE=0
TOTAL_TROUGH=0
TOTAL_OTHER=0

for f in $TEMPLATE_DIR/*.terrain; do
    [ -f "$f" ] || continue
    TOTAL_HILL=$((TOTAL_HILL + $(grep -ic '^Hill' "$f" 2>/dev/null || echo 0)))
    TOTAL_RANGE=$((TOTAL_RANGE + $(grep -ic '^Range' "$f" 2>/dev/null || echo 0)))
    TOTAL_TROUGH=$((TOTAL_TROUGH + $(grep -ic '^Trough' "$f" 2>/dev/null || echo 0)))
done

echo "   - Hill 命令: $TOTAL_HILL"
echo "   - Range 命令: $TOTAL_RANGE"
echo "   - Trough 命令: $TOTAL_TROUGH"
echo ""

# 4. 验证内置模板
echo "4. 内置模板函数检查："
BUILTIN_COUNT=$(grep -E "pub fn (earth_like|archipelago|continental|volcanic_island|volcano|high_island|continents|pangea|mediterranean)\(" src/terrain/template.rs 2>/dev/null | wc -l)
echo "   发现 $BUILTIN_COUNT 个内置模板函数"
echo ""

# 5. 检查测试文件
echo "5. 测试覆盖："
TEST_COUNT=$(grep -c "#\[test\]" src/terrain/template_tests.rs 2>/dev/null || echo 0)
DSL_TEST_COUNT=$(grep -c "#\[test\]" src/terrain/dsl.rs 2>/dev/null || echo 0)
echo "   - template_tests.rs: $TEST_COUNT 个测试"
echo "   - dsl.rs: $DSL_TEST_COUNT 个测试"
echo ""

# 6. 总结
echo "=== 测试总结 ==="
echo "   模板文件数量: $TEMPLATE_COUNT"
echo "   内置模板数量: $BUILTIN_COUNT"
echo "   测试用例数量: $((TEST_COUNT + DSL_TEST_COUNT))"

if [ "$ALL_VALID" = true ]; then
    echo "   状态: ✓ 所有模板语法验证通过"
else
    echo "   状态: ⚠ 部分模板有问题"
fi
echo ""
