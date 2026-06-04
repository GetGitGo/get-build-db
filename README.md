# log2cc

从 `make` 构建日志中提取 **compile_commands.json**，生成目录树和源码复制脚本。

## 要求

| 组件 | 说明 |
|------|------|
| Rust | 1.70+（edition 2021） |
| 输入 | 两个日志文件（UTF-8，含非 UTF-8 时会 lossy 解码） |

## 构建日志（两个文件）

| 文件 | 内容 |
|------|------|
| `*_stdout.log` | 完整 make 过程：`make[N]: Entering directory`、编译命令、警告等 |
| `*_depends.log` | Makefile `-MD -MP` 生成的依赖块（`cat *.d >>` 写入） |

**背景：** `-M` 与 `-c` 同条命令时依赖通常不出现在 stdout；用 **`-MD`** 写 `.d`，再单独收集到 `depends.log`。

在 `CXXFLAGS` 增加 `-MD -MP`，编译规则中（行首 Tab）：

```makefile
CXXFLAGS = $(CFLAGS) -MD -MP

%.o: %.cpp
	$(CXX) $(CXXFLAGS) -c -o $@ $<
	@cat $(basename $@).d >> ./depends.log
	@rm $(basename $@).d
```

完整构建示例：

```bash
make clean && make … 2>&1 | tee project_stdout.log
# depends.log 由 Makefile 各 %.o 规则追加
```

`depends.log` 依赖块示例：

```makefile
main.o: main.cpp \
  /path/to/project/common/config.h \
  /opt/toolchain/.../usr/include/stdio.h \
  ...
```

系统/toolchain 头文件会过滤；项目 `.h` / `.hpp` 写入 `inc_dir_tree.json` 的 `files`。

## 快速开始

```bash
git clone https://github.com/GetGitGo/get-build-db.git
cd get-build-db
cargo run -- /path/to/project_stdout.log /path/to/project_depends.log
```

输出前缀取自 **stdout** 文件名（去扩展名后按 `_` 或 `-` 第一节），例如 `project_stdout.log` → `project`：

| 文件 | 说明 |
|------|------|
| `<prefix>_compile_commands.json` | 编译数据库（clangd / LSP 可用） |
| `<prefix>_cpp_dir_tree.json` | 以公共根为起点的 C/C++ 源文件目录树 |
| `<prefix>_inc_dir_tree.json` | include 目录树（含参与编译的头文件列表） |
| `<prefix>_copy_sources.sh` | 源码复制脚本（绝对路径 `cp`，含 tar 打包） |

## 生成的脚本用法

```bash
chmod +x project_copy_sources.sh
./project_copy_sources.sh my_project
```

在**任意目录**执行均可；脚本内 `cp` 使用源文件绝对路径。

## 工作原理

1. **解析 stdout** — 追踪 `make[N]: Entering directory` 得工作目录；识别带 `-c` 的编译命令；解析源文件绝对路径
2. **构建 C/C++ 目录树** — 对源文件路径求公共根，按目录分组
3. **解析 depends** — 从 `depends.log` 的 `-MD` 块提取头文件（过滤标准库），写入 `inc_dir_tree`
4. **生成脚本** — 按目录树输出 `mkdir` 与绝对路径 `cp`

## 项目结构

```
.
├── Cargo.toml
├── src/
│   └── main.rs    # 唯一入口，四步流水线
└── LICENSE
```

## License

MIT License — see [LICENSE](LICENSE) for details.

Copyright (c) 2026 [GetGitGo](https://github.com/GetGitGo)
