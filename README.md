# log2cc

从 `make` 构建日志中提取 **compile_commands.json**，生成目录树和源码复制脚本。

## 快速开始

```bash
# 直接运行
cargo run -- build.log
```

输出三个文件：
- `compile_commands.json` — 编译数据库（clangd/LSP 可用）
- `dir_tree.json` — 以公共根为起点的目录树
- `copy_sources.sh` — 源码复制脚本（含 tar 打包）

## 生成的脚本用法

```bash
./copy_sources.sh my_project
```

在运行目录下创建 `my_project/`，按原始层级复制全部源文件，最后打包为 `my_project.tgz`。

## 工作原理

1. **解析日志** — 追踪 `make[N]: Entering directory` 获取工作目录，识别编译器行（gcc/g++/clang/交叉编译链），通过 `-c` 标志区分编译和链接，从 `-c -o obj.o src.c` 提取源文件路径并解析为绝对路径
2. **构建目录树** — 对全部源文件路径求公共祖先根，按目录层级分组，文件按字母排序
3. **生成脚本** — 遍历目录树输出 `mkdir -p` + `cp` 命令，源文件不存在时打印 `SKIP` 并继续

## 项目结构

```
src/
└── main.rs    # 唯一入口，三步流水线
```
