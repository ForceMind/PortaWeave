# PortaWeave（随卷）

[English](README.md)

> 一个容器，两端工作；按需增长，自动回收。

目标是把 exFAT 移动硬盘上的目录容器挂载为 macOS/Windows 的**正常读写开发工作盘**。Codex 本地会话、Git、IDE、依赖安装、编译、测试和程序运行必须直接在卷内工作，不以只读预览、专用导入导出或暗中复制到系统盘替代。

## 当前状态（2026-10-24）

**M1-B 实验性核心安全增量 + 初版兼容性探针。仍然不能挂载为系统磁盘，不是正式产品。** 核心已有真实数据段和 SQLite 目录、分块读写、目录操作、共享对象硬链接、打开后删除保护、读取租约以及操作触发的自动回收。本次增加独立证据副本诊断/内容导出、提交后维护失败状态。平台挂载、完整元数据/权限语义、空闲维护调度和桌面客户端尚未完成；严禁存放唯一副本。

实现范围见 [M1-A 核心](docs/IMPLEMENTATION-M1A.zh-CN.md)、[M1-B 安全增量](docs/IMPLEMENTATION-M1B.zh-CN.md)，测试范围见 [验证记录](docs/VALIDATION.zh-CN.md)。在云端 Mac/Windows 上运行核心库测试，不等于真实 exFAT、挂载盘或 Codex 工作区已兼容。

FSKit、WinFsp 仍为待验收候选。核心硬链接不代表 WinFsp 的接口限制已解决；不能为了保留选型而降低开发工作盘要求。

## 运行存储实验

使用 `rust-toolchain.toml` 的锁定工具链和 Cargo.lock。示例只接受全新、可丢弃的目录；不会挂载磁盘或格式化设备。

```sh
cargo test --workspace --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p portaweave-core --example scratch -- /path/to/new-disposable-lab
```

当前自动回收在串行操作中执行，不是空闲后台线程。打开句柄或读取租约引用的数据不会提前删除；段字节统计不包括数据库、WAL 和宿主分配开销。物理断电和 Windows 目录持久化尚未验证。

## 原件保全诊断与内容导出

```sh
cargo run --locked -p portaweave-core --bin portaweave-recover -- verify SOURCE NEW_WORKSPACE
cargo run --locked -p portaweave-core --bin portaweave-recover -- export SOURCE NEW_WORKSPACE NEW_OUTPUT
```

`Recovery::prepare` 和 `portaweave-recover` 持有原件所有权锁复制证据，仅在独立工作副本上恢复 SQLite/WAL，随后严格校验、导出文件。原件不开启 SQLite、不回收、不修改文件内容。读取可能更新宿主访问时间；不是硬件写保护或完整损坏抢救器。

工作目录和导出目录必须全新；失败保留已生成成果，不覆盖旧文件。只有 `COMPLETE` 存在且 `INCOMPLETE` 不存在才是导出完成。默认保留硬链接；目标不支持时失败，只有显式 `--copy-hardlinks` 才导出独立副本并记录差异。只导出当前格式已有的内容、名称及选择的链接语义，尚不支持完整 ACL/xattr/时间属性迁移。默认预算和完整边界见 [M1-B 说明](docs/IMPLEMENTATION-M1B.zh-CN.md)。

业务提交后的 GC 错误通过 `maintenance_status()` 和 Stats 单独展示，并进入写保护，不把已提交写入误报为失败。真正的提交错误仍可能结果不明，不能承诺所有 Err 都是回滚。

## 文档

- [完整产品与存储规格](docs/SPEC.zh-CN.md)：设计目标，不是已完成功能列表
- [开发工作盘兼容性契约](docs/COMPATIBILITY.zh-CN.md)
- [研发路线与发布门禁](docs/ROADMAP.zh-CN.md)
- [M1-A 实现](docs/IMPLEMENTATION-M1A.zh-CN.md)、[M1-B 安全增量](docs/IMPLEMENTATION-M1B.zh-CN.md)
- [验证方法与证据状态](docs/VALIDATION.zh-CN.md)
- [候选后端决策](docs/adr/0001-development-volume-backend-gate.md)
- [官方资料](docs/SOURCES.md)、[工程规则](AGENTS.md)、[工作接管指令](WORK_HANDOFF.zh-CN.md)

## 普通宿主目录探针

Python 3.10+，对应 Git/Node 用例需要已安装工具。仅指向专用测试目录，不要指向原始容器：

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python -m unittest discover -s tests -v
```

探针不安装驱动、不联网、不启动 Codex、不改权限。通过只证明对应环境中的用例，不冒充真实卷认证。正常沙盒、单一容器所有者、安全卸载和真实平台验证仍是产品门禁。

项目顶层许可证尚未由所有者确定，第三方许可独立保留。名称不代表已完成商标核验。
