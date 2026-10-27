# M0-V：FSKit v3 目录 FSItem 引用生命周期

日期：2026-10-27。接续 M1-O，修复真实 FSVolume 适配代码的目录引用缺口，**不是已安装/挂载的文件系统**。架构见 [ADR 0016](adr/0016-fskit-directory-item-references.md)；准确 source/tree、CI 和最终 main 在对应 PR 验收。

## 解决的问题

旧 SDK 测试依赖额外的可信 C 目录 grant 来保留删后 inode；FSVolume 缓存的目录 FSItem 自己不持有引用。适配层还通过 BUSY 拒绝打开目录的删除/替换。M1-O 的 v3 核心已能保留空孤儿，本轮将该能力接入实际 FSKit item 缓存，不再把目录名字或 OpenModes 当 inode 引用。

Apple 的 [reclaimItem 合同](https://developer.apple.com/documentation/fskit/fsvolume/commonoperations/reclaimitem%28_%3Areplyhandler%3A%29)规定上层放弃 item 引用后的回收调用；[OpenCloseOperations](https://developer.apple.com/documentation/fskit/fsvolume/opencloseoperations)描述保留的打开模式。实现保持当前 macOS26 Operations 签名，未混入 macOS27 Handler/FSContext，也未认证 native caller。

## 实现

| 入口/时点 | 行为 |
|---|---|
| v3 lookup 首次 intern 目录 | 原生准入并保存独立 administrative directory pin；重复 lookup 返回同一 item，不多占 pin |
| v3 createItem directory | 预分配 SDK item/cache、准入 native/core pin、事务内创建名字/初始元数据及纯回执投影，再发布引用和 item |
| openItem / closeItem | 继续使用 FSKit 的保留模式；目录写模式拒绝，close modes=[] 清除访问但保留缓存身份 |
| 删除/覆盖目录 | v3 保留原 inode，允许 read-open 的空目录删除/替换；仍核验预期 ID，不能按旧名字跟随替代 inode |
| get/setAttributes、枚举 | 保留 item 可观察/设置原 inode；删除后的空目录枚举为空，`.`/`..` 不虚构；不能再加入子项 |
| reclaimItem | 打开模式未清空时 BUSY；关闭自己的 snapshot 和 typed directory pin，再移除 item；旧 SDK 对象 ESTALE |
| deactivate | 任意 file/directory pin 或 OpenModes 存活时 BUSY；没有强制关闭/伪称安全卸载 |

根目录不会被 unlink，在 owner 存续期间不额外占用 pin。缓存目录与普通文件的身份引用都保持到 reclaim；这不等于每次 open 都创建一个新 pin，也不等于 closeItem 的 modes 是引用计数。

新增核心 `Volume::create_directory_with_pin` / `CreatedDirectory<T>`，复用既有创建事务与 reserve/publish，初始 uid/gid/mode/time、父元数据和回执一起提交。该入口只支持 v3，目录 size 即使为零也不支持。投影拒绝/提交前故障不留名字或 pin；提交后维护失败仍返回已提交结果/可关闭 pin，并单独报告保护。普通 mkdir/create_node_with_options/旧 C 创建目录仍不分配 pin。

新增 administrative C `pw_directory_open/close` 和 `pw_fskit_directory_create_with_state` / `PwFsKitDirectoryCreated`。directory token 不放进旧 `PwFsKitCreated.file`，不混用 file、policy directory、cursor 或 view。四类文件/目录 pin 共用 4096 核心/原生准入、计入 BUSY，失败输出不变；保护下仍可释放。v1/v2 返回 UNSUPPORTED，不改变原 schema/marker/capabilities，无迁移。

这些 SDK/C 入口是明确的**管理操作**，没有可信凭据参数，更不是 OS 身份授权。它们与 `PosixDirectory` 的 READ+SEARCH 策略 grant 保持独立。FSKit checkAccess/probe/load/activate/mount、WinFsp security/Open/Flush 继续关闭；不将 inode pin 误称 ACL、POSIX caller 映射或 Codex 沙盒通过。

## 验证与范围

Work Linux full all-feature/default workspace、clippy `-D warnings`、fmt、Python9、实际 C11 caller 与独立 C++17 公共头已通过。新增 core directory_create_pins all-feature4/default2 项，ignored helper 由父测试实际执行两次 exit(86)；验证提交前后真正进程退出的名字与恢复状态。新 native directory_item_pins 4 项（integration累计47/unit3），包括4个真实线程同时创建同名目录，只允许一个完整成功回执，其余冲突输出不变且不泄漏 pin。既有 core/native 4096 混合额度测试加入新 typed admin directory 和 atomic directory create，未虚增测试数量。

C caller 输出 `NATIVE_DIRECTORY_ITEM_REFERENCE_ABI_OK`、既有 v3/ABI 标记；本地 v3 四阶段每阶段增加 `DIRECTORY_CREATE_PIN_HANDOVER_OK`，确认初始状态/原 inode/关闭后 GC。固定 a578 实际旧 reader 仍拒绝 v3 并保全输出与原件 SHA-256。本地阶段全部在 Linux，不算 macOS/Windows 证据。

新 SDK 合同直接调用实际 FSVolume create/lookup/open/remove/rename/close/reclaim 和属性对象，覆盖删后身份、目录替换、外部 grant 释放后 SDK 自有 pin、完整4096额度下的 create/lookup拒绝/恢复、字段消费与父时间不变。枚举空目录使用既有 pack closure，仅验证算法；真实 packer/kernel dispatch 仍 NOT_RUN。新标记 `FSVOLUME_DIRECTORY_ITEM_REFERENCE_CONTRACT_OK` / `FSVOLUME_DIRECTORY_ADMISSION_CONTRACT_OK` 必须由准确源码 macOS CI 执行，不从 Linux 或 Swift 类型检查预报成功。完整 App/Extension、MSVC/Dispatcher、三平台回归、旧二进制拒绝及 v1/v2/v3 artifact 也须逐项核验。

未完成：真实已安装 WinFsp 的隔离挂载、macOS 签名/系统授权、原生身份/ACL/共享/缓存/完整同步、符号链接/xattr/监听/OS mmap、真实卷 Git/SQLite/依赖/编译/IDE/Codex 正常沙盒及越界拒绝、物理 exFAT/USB/断电。没有操作用户本机、安装驱动、关闭沙盒或运行付费代理；Issue #1/#3 保持开放。
