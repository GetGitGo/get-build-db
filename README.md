# log2cc

从 `make` 构建日志中提取 **compile_commands.json**，生成目录树和源码复制脚本。

## 要求

| 组件 | 说明 |
|------|------|
| Rust | 1.70+（edition 2021） |
| 输入 | shell 脚本调用 `make` 的构建日志（UTF-8，含非 UTF-8 时会 lossy 解码） |

## 快速开始

```bash
git clone https://github.com/GetGitGo/get-build-db.git
cd get-build-db
cargo run -- /path/to/build.log
```

输出三个文件（本地生成，已在 `.gitignore` 中忽略）：

| 文件 | 说明 |
|------|------|
| `compile_commands.json` | 编译数据库（clangd / LSP 可用） |
| `dir_tree.json` | 以公共根为起点的目录树 |
| `copy_sources.sh` | 源码复制脚本（含 tar 打包） |

## 生成的脚本用法

```bash
chmod +x copy_sources.sh
./copy_sources.sh my_project
```

在**当前工作目录**下：

1. 创建 `my_project/`
2. 按原始目录层级复制全部源文件（源文件不存在时打印 `SKIP` 并继续）
3. 打包为 `my_project.tgz`

## 工作原理

1. **解析日志** — 追踪 `make[N]: Entering directory` 获取工作目录；识别编译器行（gcc / g++ / clang / 交叉编译链）；用 `-c` 区分编译与链接；从 `-c -o obj.o src.c` 提取源文件并解析为绝对路径
2. **构建目录树** — 对全部源文件路径求公共祖先根，按目录层级分组，文件按字母排序
3. **生成脚本** — 遍历目录树输出 `mkdir -p` 与 `cp` 命令

## 项目结构

```
.
├── Cargo.toml
├── src/
│   └── main.rs    # 唯一入口，三步流水线
└── LICENSE
```

## License

MIT License — see [LICENSE](LICENSE) for details.

Copyright (c) 2026 [GetGitGo](https://github.com/GetGitGo)
