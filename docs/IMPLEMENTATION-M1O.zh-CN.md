# M1-O：显式 v3 目录 inode 生命周期

日期：2026-10-27。接续 M0-T/U 与 M1-N；这是 Rust 核心、可信 C ABI 和平台直接调用合同增量，**不是已可安装、挂载的系统文件卷**。架构决定见 [ADR 0015](adr/0015-explicit-v3-directory-inode-lifetimes.md)。准确 source/tree、CI 与合并结果记录在对应 PR。

## 格式与兼容边界

| 创建入口 | marker / user_version | capabilities | 目录最后名字删除 |
|---|---|---|---|
| `Volume::create` / `pw_volume_create` | 既有 v1 / 1 | 无元数据能力 | 既有立即删除；孤儿目录仍非法 |
| `create_with_metadata` / C 同名入口 | 既有 v2 / 2 | 1 | 既有立即删除；孤儿目录仍非法 |
| `create_with_directory_lifetimes` / C 同名入口 | `experimental-m1o-directory-v3` / 3 | 3 | 可在 pin 存续期间保留空 inode |

第三种入口只创建全新目录；不改变默认入口，不迁移 v1/v2，不改写旧 schema。marker、精确 schema、config、user_version 与 UUID/capabilities 继续一起校验。新格式仍不稳定，禁止存放唯一副本。新 metadata capability 不等于 OS 权限已经生效。

CI 从固定提交 `a578785591976e9d71658dc5b93bf1c234c9db7f` 单独构建真正的旧 native cdylib，以另一个进程调用旧 `pw_volume_open` 两次，要求 UNSUPPORTED、输出哨兵不变和原件全部普通文件 SHA-256 不变。旧读取器不通过时不能改 marker、跳过校验或悄悄迁移。Work Linux 已实际执行此验证；其他 OS 必须读取准确源码 CI，不能从阶段名字推断。

## 核心行为

新增不同类型的管理 `DirectoryHandle` 与可信策略 `PosixDirectory`。策略 open 在当时检查目录 READ+SEARCH 及全部祖先搜索；已经授予的 info/枚举权在 chmod、移动、祖先撤权和最后 unlink 后存续到 close。info 返回当前存储属性，不返回旧快照、不重新解析原名。新名字可指向另一 inode，旧 grant 不会跟随它。

这些引用与所有管理/策略文件共享核心 4096 pin 限额；原始 Rust 入口同样不能绕过。创建目录本身仍不消耗 pin。复制 Rust token 不增加引用，close 会使其所有副本失效；foreign、stale 与跨进程 token 不是有效授权。

v3 rmdir/目录替换在同一 SQLite 事务删除名字、更新旧 inode 元数据；被替换对象保留到自己的最后一个引用关闭。未命名非根目录只允许 size=0、没有任何子项/块。原始及策略名字空间修改都拒绝向它创建文件/目录、链接或移动新项；已知 ID 也不能恢复路径授权。根目录的内部名字引用数同样为零，但根不是孤儿。内部 stat.links 不是原生目录 st_nlink，FSKit 不宣告目录 linkCount，WinFsp HardLinks 仍为零且接口缺口 BLOCKED。

已获目录读权不授予 chmod/chown 权限。`set_directory_metadata_with_reply` 每次采用当前 owner/group 策略；换主后旧 owner 必须被拒绝。Windows 时间投影仍在业务 commit 前完成，拒绝投影不修改 inode，不改输出。提交后维护失败保留业务成功和独立健康/写保护；保护下仍可释放引用。

## 恢复与自动回收

普通 open 在完整验证后回收无当前 pin 的空孤儿目录；每次正常 close/后续变更继续触发串行回收，不声称空闲后台调度。回收前先拒绝有子项、块或非零大小的孤儿，不能级联删除可疑数据。

`Recovery::prepare` 仍先保全原件，只在独立工作 catalog 恢复 SQLite。verify 新增 `orphan_directories`，严格检查所有对象；导出报告 `unnamed_orphan_directories_not_exported=N`，不猜旧名字。v1/v2 孤儿目录继续非法；断开的环/多重目录链接/不可达子树继续非法。内容导出不应用源 uid/gid/mode/时间/ACL/xattr。

`directory_handover` 为 v3 建立独立 seed→macOS→Windows→Linux artifact 链，保留原 v1/v2 链。每个发送阶段故意留下一个仍被 owner pin 住、已 unlink 的空目录；退出 owner 后下一站先保全恢复/导出并核对原件全部文件 SHA-256，再普通重开核验 GC。UUID、同一文件硬链接身份和 stage 内容一起验证。**这是明确的 abandoned-owner 夹具，不是安全卸载或硬件断电。** 核心测试另用真正子进程 exit(86) 验证 rmdir commit 前后状态，禁止 raw fork 继承 live owner。

## 原生接口与平台合同

新增 `pw_posix_directory_open/info/close/set_metadata_info`、显式 v3 创建，以及 Windows `begin_granted`、`handle_info`、`handle_set_basic_info`。目录 grant 与 file、view、metadata cursor 分表，包括 BUSY 和预算。失败输出保持原值。它们接受的是明确受信任测试凭据，不认证 OS caller，不能从不可信 IPC 接受 UID/组声明。

`begin_granted` 在旧目录读 grant 下准备新的不可变名字/属性副本，不增加 inode pin。副本使用既有 64 cursor/100000 retained entries 预算和固定 WinFsp UTF-16/104-byte/8-byte/ordinal/EOF 格式；源关闭并回收 inode 后仍可读。被删除的空目录不产生 `.`/`..`，空输出只有两字节 EOF。没有自动驱逐别人的 cursor/pin，逻辑数额也不是堆/峰值内存保证。

WinFsp 实验 C context 新增独立 `directory_grant`，GetFileInfo 和 SetBasicInfo 接入当前 inode，保留 DIRECTORY 标志和纯提交前回执；NULL metadata_caller 拒绝 setter，换主后旧测试身份拒绝。Close 分别释放 metadata snapshot 和 grant；读目录仍明确使用持有的 snapshot，不引入未经同步的自动 context token 替换。服务 Open/security/Flush 继续关闭。

Swift 实际 SDK 合同通过显式 C 入口获得**外部可信目录 pin**，随后用 FSVolume getAttributes 观察 cached FSItem 在删后仍存在、最后 close 后 ENOENT；它不是 FSKit 原生 Open。App/Extension 继续无签名构建，access/probe/load/activate/mount 不开放，不安装、不启用。

## 测试与尚未完成

Work Linux 已通过 full default/all-feature workspace、clippy `-D warnings`、fmt、真实 C11 caller、独立 C++17 公共头。新增 core `directory_lifetimes` all-feature 10 项/default 8 项；ignored helper 只由父测试在一次性夹具中以子进程调用，实际覆盖两次 exit(86)。原有 create_replies 4096 测试加入目录与管理/策略文件混合预算，不虚增测试数量。

新增 native 8 项（integration 累计 43，unit 3）：格式/旧卷拒绝、撤权/移动/最后 unlink、多引用回收/副本独立、当前 owner 与提交前 Windows 投影、非法/跨 owner/混用 token、输出哨兵、64 真实 cursor、4 个真实 info 线程与最后 close 竞争。原有满 4096 ABI 回归也加入目录 pin。

本地 v3 四阶段例程和实际旧二进制拒绝通过；这四个阶段都运行在 Linux。首轮 C caller 使用 uid=100 创建夹具却用既有 uid=501 申请 grant，正确返回 DENIED；修正夹具 owner 后通过，权限实现没有放宽。MSVC/SDK/bundle 和真正跨 OS artifact 必须依据此增量准确 CI 日志，不能从本地结果预报 PASS。

Windows 已安装 runtime/driver 的隔离实际挂载仍 BLOCKED/NOT_RUN，ACL/SID/共享访问/缓存一致性/完整 Flush 和硬链接接口未完成。macOS 签名、系统授权、原生 caller、目录 Open/缓存/同步与真正挂载未完成。符号链接、监听、OS mmap、原生执行、真实卷 SQLite/Git/依赖安装/IDE/Codex 正常沙盒及越界拒绝、物理 exFAT/USB/断电验收均未通过；本增量不关闭 Issue #1/#3。
