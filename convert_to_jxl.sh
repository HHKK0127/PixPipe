#!/bin/bash

# JPG to JXL ロスレス変換スクリプト
# EXIFデータ保持 + 変換後に元ファイル削除

PROCESS_DIR="${1:-.}"  # デフォルトはカレントディレクトリ
SUCCESS_COUNT=0
FAIL_COUNT=0

echo "=== JPG → JXL ロスレス変換開始 ==="
echo "処理ディレクトリ: $PROCESS_DIR"
echo ""

# JPGファイルを処理
for jpg_file in "$PROCESS_DIR"/*.{jpg,JPG,jpeg,JPEG}; do
    # ファイルが存在するか確認
    [[ -e "$jpg_file" ]] || continue

    jxl_file="${jpg_file%.*}.jxl"

    echo "処理中: $(basename "$jpg_file")"

    # cjxlでロスレス変換（-d 0でロスレス、EXIFは自動保持）
    if cjxl "$jpg_file" "$jxl_file" -d 0 -q 100; then
        # 変換成功 → 元ファイル削除
        if rm "$jpg_file"; then
            echo "  ✓ 完了: $jpg_file → $jxl_file（削除）"
            ((SUCCESS_COUNT++))
        else
            echo "  ✗ エラー: ファイル削除失敗"
            ((FAIL_COUNT++))
        fi
    else
        echo "  ✗ エラー: 変換失敗"
        ((FAIL_COUNT++))
    fi
done

echo ""
echo "=== 処理完了 ==="
echo "成功: $SUCCESS_COUNT件"
echo "失敗: $FAIL_COUNT件"
