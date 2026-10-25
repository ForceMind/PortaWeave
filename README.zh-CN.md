# PortaWeave（随卷）

[English](README.md)

> 一个容器，两端工作；按需增长，自动回收。

目标是把 exFAT 上的动态目录容器挂载为 macOS/Windows 的**正常读写开发工作盘**。Codex 本地会话、Git、IDE、依赖安装、编译、测试和程序运行必须直接在卷内工作，不以只读预览、专用导入导出或隐藏的系统盘副本替代。

## Disk Studio（磁盘工作台）｜新增长期功能方向

参考 DiskGenius 的分区管理、镜像、克隆与恢复等**功能分类**，但不会复制其源码/UI；详见 [完整规划](docs/DISK-STUDIO.zh-CN.md)、[ADR 0010](docs/adr/0010-partition-manager-separate-block-plane.md)、[Issue #28](https://github.com/ForceMind/PortaWeave/issues/28)。

**本轮 M0 只实现普通 RAW/IMG 文件上的 GPT/MBR 分区表只读检查器**，不支持物理硬盘读写、分区增删/格式化、VHDX 容器解析或 OS 挂载；不改变原有 .pwvol 容器格式。示例：

~~~sh
cargo run --locked -p portaweave-partition-inspector -- inspect /path/to/disposable.raw --sector-size 512
cargo test --locked -p portaweave-partition-inspector
~~~

支持 512/4096 字节扇区、GPT 主备 CRC 校验和异常报告；MBR 只解析四个主分区，扩展 EBR 链暂不支持。只有一份 GPT 有效时报告 DEGRADED 并且绝不自动修复。后续独立规划逻辑子卷、动态块设备 .pwdisk 和受控真实磁盘管理，**不能用“分区 UI”掩盖当前尚未完成的双端工作盘、Codex/Git 验收**。

## 当前状态（2026-10-25）

**M0-N WinFsp 授权文件合同与 M0-P 实验性 FSVolume 回调、macOS App/Extension 无签名构建、实验性 WinFsp Dispatcher 与 M1-M 原子写入/元数据核心。尚未实测真正挂载，也不是正式产品。** 已有真实数据段与 SQLite 目录、分块读写、目录操作、共享身份硬链接、打开后删除保护、读取租约和操作触发的回收；另有原件保全诊断/内容导出及独立维护状态。平台适配、有效权限、完整元数据/安全语义、空闲调度和可用桌面客户端尚未完成，严禁存放唯一副本。

实现范围见 [M1-A](docs/IMPLEMENTATION-M1A.zh-CN.md)、[M1-B](docs/IMPLEMENTATION-M1B.zh-CN.md)、[M1-C](docs/IMPLEMENTATION-M1C.zh-CN.md)，历史测试见[验证记录](docs/VALIDATION.zh-CN.md)。确切源码版本的测试结果记录在对应 PR；实现文档不是自动的通过声明。云端三系统核心测试不等于真实 exFAT、挂载盘或 Codex 已兼容。

FSKit/WinFsp 仍为待验收候选。核心硬链接不代表 WinFsp 接口限制已解决，不能为保留选型降低工作盘要求。


### 原生接口（实验）

新增 `portaweave-native` C ABI 与 [`include/portaweave_native.h`](include/portaweave_native.h)，提供实际核心目录与文件操作、句柄和健康状态，详见 [M0-E 说明](docs/IMPLEMENTATION-M0E.zh-CN.md)。**这不是 FSKit/WinFsp 真正挂载：** 系统回调、安全授权、文件监听、安全卸载与完整同步语义仍然没有实现，不得把 ABI 通过冒充 Codex 工作盘验收。

## M0-G WinFsp 容量与持久化契约（实验）

既有原生 ABI 新增逻辑配额、保守宿主可用空间快照及 SQLite WAL FULL 检查点；**这不等同于整个卷的 fsync 或 Windows 文件 Flush 成功**。Windows CI 使用固定的 WinFsp 2.1 官方头文件编译并在模拟 WinFsp 对象上测试真实回调签名；没有安装驱动，也没有真正挂载盘符。权限与 Flush 尚未实现，相关回调明确拒绝而不是虚假成功。详见 [M0-G](docs/IMPLEMENTATION-M0G.zh-CN.md)。

## M0-H 实验性 WinFsp Dispatcher 服务

`platform/windows/winfsp_service.c` 已编写真正 WinFsp 文件系统创建、设置盘符、启动与停止分发器的代码；CI **只编译并执行 `--self-test`**，不安装驱动、不挂载盘符。必须明确手动运行的 `--lab-mount R:` 只能创建临时目录里的全新测试卷，且依赖系统已经安装 WinFsp；**该路径尚未真实运行验证**。出于安全原因，所有用户文件 Open 均拒绝，Flush 返回未支持，绝不能用于真实工作或保存数据。详见 [M0-H 边界](docs/IMPLEMENTATION-M0H.zh-CN.md)。

## M0-J macOS 宿主 App 与 ExtensionKit 构建

仓库新增真正的 Xcode App 和嵌入 FSKit Extension target，均静态链接共享 Rust 核心。在 macOS 运行 `bash ci/build-macos-lab.sh` 构建并检查无签名实验包；脚本不安装、启用或挂载。FSVolume 回调、有效权限、同步与签名仍未完成。详见 [M0-J 构建范围](docs/IMPLEMENTATION-M0J.zh-CN.md)。

## M1-E 范围读取租约

`acquire_range_view` 仅保留指定区间的块索引，保存原 EOF 并共用 owner 级预算。原生 C begin/read/close 接口支持源句柄关闭后的旧版本读取；活动租约阻止关闭卷。这不是 OS mmap 或挂载验收。详见 [M1-E 范围与测试](docs/IMPLEMENTATION-M1E.zh-CN.md)。

## M1-F catalog 结构门禁

普通打开和保全恢复共用实验 schema 检查，在自动回收前拒绝未知表、列、索引、view 和 trigger，兼容源码 checkout 的 LF/CRLF 差异。这不是完整恶意 SQLite 解析器防护，普通 open 也不是只读诊断。详见 [M1-F 实现与回归记录](docs/IMPLEMENTATION-M1F.zh-CN.md)。

## FSKit FSVolume 回调（实验）

新增真正 FSVolume 子类，通过每卷串行队列接入共享核心的目录、句柄、写入/截断、硬链接、同步及分页枚举。直接 SDK 合同验证不等于 OS 挂载；缺失元数据和权限明确拒绝，unary load、activate、mount 继续关闭。详见 [M0-K 范围](docs/IMPLEMENTATION-M0K.zh-CN.md)。

M0-L 将 v2 持久 UUID、所有者/权限位/时间接入 FSVolume 身份、属性读取和目录枚举。M0-M 在实验合同中添加 v2 单事务大小/元数据设置和明确所有者/mode 创建。v1 缺失属性仍拒绝；有效授权及 access/load/mount 门槛继续关闭。准确范围和 SDK 测试见 [M0-L](docs/IMPLEMENTATION-M0L.zh-CN.md)、[M0-M](docs/IMPLEMENTATION-M0M.zh-CN.md)。

## 后端存储同步

`pw_storage_synchronize` / `Volume::synchronize_storage` 在 Unix 实际刷新数据段、catalog/WAL 与容器目录，保留活动句柄和租约，忙碌/错误明确返回。Windows 返回未支持。原生缓存排空、更强设备刷新和安全弹出仍需分别实现及验收。详见 [M1-G](docs/IMPLEMENTATION-M1G.zh-CN.md) 与 [ADR 0004](docs/adr/0004-backend-storage-barrier.md)。

## M1-H 显式元数据格式

`Volume::create_with_metadata` / `pw_volume_create_with_metadata` 显式新建实验 v2 容器，持久化 UUID、uid/gid/mode 和纳秒时间。元数据与内容/名字空间变更共用事务，覆盖硬链接和替换后旧打开对象。默认创建仍为 v1，不自动升级已有容器。这些是受信任的存储接口，尚不实施有效 OS 权限或认证调用者。见[范围、测试及限制](docs/IMPLEMENTATION-M1H.zh-CN.md)与 [ADR 0005](docs/adr/0005-opt-in-metadata-format.md)。

M1-I 增加大小/元数据单事务变更，明确时间优先级，验证提交前故障与提交后维护失败，旧内容租约继续有效。见 [M1-I](docs/IMPLEMENTATION-M1I.zh-CN.md)；原生有效权限仍需完成。

## M1-J 目录权限策略

`Volume::posix(Credentials)` 与新增 C 接口实际约束选定目录操作的 owner/group/other、祖先搜索、粘滞删除/改名、caller 所有权、umask 与 setgid 目录继承。凭据是显式受信任的声明，尚不认证原生调用者、不控制裸数据句柄、不实施 Windows ACL。默认 v1 不变，原生 access/load/mount 门槛继续关闭。见 [M1-J 范围/测试](docs/IMPLEMENTATION-M1J.zh-CN.md)与 [ADR 0006](docs/adr/0006-trusted-posix-policy.md)。

## M1-K 打开文件能力与属性授权

不透明策略文件在 chmod/chown/rename/unlink 后保留打开时的读写能力，直到关闭；属性变更另查当前 owner/group，混合大小/属性仍使用单事务。C 的策略与管理 token 分表，共享总限额及 BUSY 规则。受信任凭据测试不认证 OS caller，也不开放挂载。见 [M1-K](docs/IMPLEMENTATION-M1K.zh-CN.md)与 [ADR 0007](docs/adr/0007-granted-files-and-metadata-policy.md)。

## M1-L 受限读能力的范围租约

只有 READ 文件能力可申请不可变范围租约，只写能力拒绝。原 FD 关闭或内容变化后仍保留原范围/EOF/字节，共用核心准入预算，释放前 volume_close 保持 BUSY。C 策略/管理 view 分表。这是进程内读能力，不是 OS mmap 或调用者身份认证。见 [M1-L](docs/IMPLEMENTATION-M1L.zh-CN.md)。

## M0-P FSKit 原子创建

实验 create 在修改前检查原生句柄总额、预留 token 并准备 Swift item；同一事务返回预验证属性和已持有的管理文件 pin，成功后不再二次 intern/open。准入失败不留名字、不消费字段；身份与挂载门槛仍关闭。见 [M0-P](docs/IMPLEMENTATION-M0P.zh-CN.md) 和 [ADR 0011](docs/adr/0011-created-node-pins-and-replies.md)。

## M0-O FSKit 事务内属性回执

实验 setattr 的大小、混合和 metadata-only 请求统一使用提交前完成投影的属性回执，消除已成功修改后再次查询失败而误报业务失败的路径；v1 大小回执明确不含元数据，后续维护告警单独报告。仍是管理回调，原生 caller/load/mount 不开放。见 [M0-O](docs/IMPLEMENTATION-M0O.zh-CN.md)。

## M0-N WinFsp 授权文件回调合同

Windows 适配器增加普通文件信息、追加/EOF 裁剪写入和 EOF 调整，读取明确处理 EOF，关闭保留状态；Windows 时间/大小投影在提交前校验。Open/安全/Flush 仍关闭，allocation 请求明确不支持。Windows CI 使用固定的真实回调签名执行合同，不安装驱动、不挂载。见 [M0-N](docs/IMPLEMENTATION-M0N.zh-CN.md) 和 [ADR 0009](docs/adr/0009-fallible-windows-reply-projection.md)。

## M1-M 原子写请求

授权文件在同一 owner 请求内解析追加位置和 EOF 裁剪；写入/截断在提交前取得身份、大小、generation 和属性回执，提交后维护失败继续独立报告。原生 ABI 包含真实多线程并发追加回归。这不开放 WinFsp 安全访问，不证明缓存/Flush 或系统挂载。见 [M1-M](docs/IMPLEMENTATION-M1M.zh-CN.md) 与 [ADR 0008](docs/adr/0008-transaction-captured-io-replies.md)。

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
