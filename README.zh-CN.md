# PortaWeave（随卷）

[English](README.md)

> 一个容器，两端工作；按需增长，自动回收。

目标是把 exFAT 上的动态目录容器挂载为 macOS/Windows 的**正常读写开发工作盘**。Codex 本地会话、Git、IDE、依赖安装、编译、测试和程序运行必须直接在卷内工作，不以只读预览、专用导入导出或隐藏的系统盘副本替代。

## 当前状态（2026-10-24）

**M1-C 实验性核心安全增量 + 初版兼容性探针。仍然不能挂载为系统磁盘，不是正式产品。** 已有真实数据段与 SQLite 目录、分块读写、目录操作、共享身份硬链接、打开后删除保护、读取租约和操作触发的回收；另有原件保全诊断/内容导出及独立维护状态。M1-C 新增宿主空间准入和数据段 I/O 故障测试。平台挂载、完整元数据/权限、空闲调度和桌面客户端尚未完成，严禁存放唯一副本。

实现范围见 [M1-A](docs/IMPLEMENTATION-M1A.zh-CN.md)、[M1-B](docs/IMPLEMENTATION-M1B.zh-CN.md)、[M1-C](docs/IMPLEMENTATION-M1C.zh-CN.md)，历史测试见[验证记录](docs/VALIDATION.zh-CN.md)。确切源码版本的测试结果记录在对应 PR；实现文档不是自动的通过声明。云端三系统核心测试不等于真实 exFAT、挂载盘或 Codex 已兼容。

FSKit/WinFsp 仍为待验收候选。核心硬链接不代表 WinFsp 接口限制已解决，不能为保留选型降低工作盘要求。

## 运行存储实验

使用 `rust-toolchain.toml` 锁定的工具链和 Cargo.lock。示例只接受全新可丢弃目录，不挂载或格式化磁盘。

```sh
cargo test --workspace --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p portaweave-core --example scratch -- /path/to/new-disposable-lab
```

回收仍在串行操作中执行，不是空闲后台线程。句柄和读取租约引用的数据不会提前删除；段统计不含数据库、WAL 和宿主分配开销。物理断电及 Windows 目录持久化尚未验证。

### 空间不足与维护状态

`space_status()` 和 `Stats.space` 展示宿主可用空间快照及软性准入预算。默认 16 MiB 软水位加 1 MiB 元数据余量，**不是已经物理分配的应急预留，也不是容量预约**。其他程序、簇分配和 SQLite 增长仍可能在预检之后耗尽空间。

stage 前的 `HostSpace` 拒绝不改变当前数据引用、不进入写保护；混合段整理空间不足时记为 `DeferredSpace`，保留旧段，在之后触发维护的操作中重试。实际 I/O/SQLite 错误仍进入写保护。提交后的维护错误记为 `Failed`，不能把成功业务误报为失败；真正 commit 错误可能结果不明，不能保证所有 Err 都回滚。故障注入接口只在测试特性启用，用户客户端不得开启。

## 原件保全诊断与内容导出

```sh
cargo run --locked -p portaweave-core --bin portaweave-recover -- verify SOURCE NEW_WORKSPACE
cargo run --locked -p portaweave-core --bin portaweave-recover -- export SOURCE NEW_WORKSPACE NEW_OUTPUT
```

恢复模块锁定原件并复制证据，只在另一个工作副本上恢复 SQLite/WAL，再严格校验和导出。原件不开启 SQLite、不执行回收、不修改内容；读取可能更新宿主访问时间，不是硬件写保护或完整损坏抢救算法。

所有目标必须全新，失败保留已有成果。`COMPLETE` 存在且没有 `INCOMPLETE` 才是完成。默认保留硬链接，目标不支持时报错；显式 `--copy-hardlinks` 才导出独立副本并报告差异。尚不支持完整 ACL/xattr/时间属性迁移，边界见 [M1-B](docs/IMPLEMENTATION-M1B.zh-CN.md)。这是恢复工具，不替代未来必须实现的真实工作盘接口。

## 文档

- [产品与存储规格](docs/SPEC.zh-CN.md)：目标，不是已实现清单
- [开发工作盘契约](docs/COMPATIBILITY.zh-CN.md)
- [路线](docs/ROADMAP.zh-CN.md)及[验证记录](docs/VALIDATION.zh-CN.md)
- [M1-A 核心](docs/IMPLEMENTATION-M1A.zh-CN.md)、[M1-B 恢复](docs/IMPLEMENTATION-M1B.zh-CN.md)、[M1-C 容量/I/O](docs/IMPLEMENTATION-M1C.zh-CN.md)
- [后端决策](docs/adr/0001-development-volume-backend-gate.md)、[资料](docs/SOURCES.md)、[工程规则](AGENTS.md)、[接管](WORK_HANDOFF.zh-CN.md)

## 普通宿主目录探针

Python 3.10+，对应 Git/Node 用例需要已安装工具。只指向专用测试目录，不指向原始容器：

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python -m unittest discover -s tests -v
```

探针不安装驱动、不联网、不启动 Codex、不改权限。通过不冒充挂载认证；正常沙盒、单一容器所有者、安全卸载和真实平台验证仍是门禁。

顶层许可证尚未由所有者确定，第三方许可独立保留。名称不代表商标已核验。
