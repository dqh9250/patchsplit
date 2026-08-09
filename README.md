# patchsplit

`patchsplit` 是一个 Rust CLI，用来从 GitHub 下载 Pull Request 的 `.patch`
文件，并按 commit 拆分成多个独立 patch 文件。

## 用法

```sh
patchsplit <owner/repo> <pr-number> [--out <dir>] [--force]
patchsplit <owner> <repo> <pr-number> [--out <dir>] [--force]
```

示例：

```sh
patchsplit rust-lang/rust 12345
patchsplit openai codex 42 -o pr-42-patches
```

默认输出目录是 `patches/`。输出文件会使用四位序号和 commit subject 命名：

```text
patches/
  0001-add-parser.patch
  0002-wire-cli.patch
```

如果输出文件已存在，命令默认拒绝覆盖。需要覆盖时传入 `--force`。

## 参数

- `-o, --out <dir>`：指定拆分后 patch 文件的输出目录。
- `-f, --force`：允许覆盖已存在的 patch 文件。
- `-h, --help`：显示帮助。
- `-V, --version`：显示版本。

## 依赖

当前实现不引入第三方 Rust crate。下载 GitHub `.patch` 时会调用系统里的
`curl`，因此运行环境需要能在 `PATH` 中找到 `curl`。

## 构建

```sh
cargo build --release
```

生成的二进制在：

```text
target/release/patchsplit
```
