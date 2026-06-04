#!/usr/bin/env bash
# 将源目录下各级子目录中的全部文件平铺复制到新建的目的目录（目的目录内无子目录）；重名追加递增序号。
set -euo pipefail

usage() {
    echo "用法: $0 <源目录> <目的目录>" >&2
    echo "  源目录必须存在且含至少一个文件；目的目录必须不存在（脚本会新建）。" >&2
    echo "  所有文件平铺到目的目录根下，不保留子目录结构。" >&2
    exit 1
}

die() {
    echo "错误: $*" >&2
    exit 1
}

# 在 dir 下为 name 选取不冲突的目标路径：name -> name_1.ext -> name_2.ext ...
unique_dest_path() {
    local dir="$1"
    local name="$2"
    local candidate="$dir/$name"

    if [[ ! -e "$candidate" ]]; then
        echo "$candidate"
        return 0
    fi

    local base ext
    if [[ "$name" == *.* && "$name" != .* ]]; then
        base="${name%.*}"
        ext=".${name##*.}"
    else
        base="$name"
        ext=""
    fi

    local n=1
    while [[ -e "$dir/${base}_${n}${ext}" ]]; do
        ((n++)) || true
    done
    echo "$dir/${base}_${n}${ext}"
}

[[ $# -eq 2 ]] || usage

SRC_INPUT=$1
DEST_INPUT=$2

[[ -d "$SRC_INPUT" ]] || die "源目录不存在: $SRC_INPUT"

if ! SRC=$(cd "$SRC_INPUT" && pwd); then
    die "无法解析源目录: $SRC_INPUT"
fi

if [[ -z "$(find "$SRC" -type f -print -quit 2>/dev/null)" ]]; then
    die "源目录下没有可复制的文件: $SRC_INPUT"
fi

if [[ -e "$DEST_INPUT" ]]; then
    die "目的目录已存在，必须指定尚未创建的路径: $DEST_INPUT"
fi

mkdir -p "$DEST_INPUT"
if ! DEST=$(cd "$DEST_INPUT" && pwd); then
    die "无法创建目的目录: $DEST_INPUT"
fi

copied=0
renamed=0

while IFS= read -r -d '' src_file; do
    rel=${src_file#"$SRC"/}
    filename=$(basename "$rel")

    dest_file=$(unique_dest_path "$DEST" "$filename")
    if [[ "$dest_file" != "$DEST/$filename" ]]; then
        ((renamed++)) || true
        echo "  重名: $rel -> $(basename "$dest_file")"
    fi

    cp -p "$src_file" "$dest_file"
    ((copied++)) || true
done < <(find "$SRC" -type f -print0)

echo "完成: 复制 $copied 个文件到 $DEST（重命名 $renamed 个）"
