# M0-P：FSKit 原子创建、预先准入与文件身份 pin

旧 createItem 在提交名字和父目录元数据后，再调用 intern/open 获取文件 pin；原生总句柄预算不足等错误可能让已创建项被报告成失败。本增量在修改前完成 cache/item 准备、共享句柄预算和 token 准入，在同一核心业务事务内构造回执；成功后不再查询或打开文件。

`Volume::create_node_with_reply` 保留 v1/v2 与显式属性规则。普通文件先预留 owner 级 Handle 标识，目录不分配文件句柄。名字、对象、v2 元数据和父目录时间提交前执行纯回执投影；拒绝或 BeforeCommit 故障回滚，预留 token 不发布、不复用。成功 commit 后立即发布核心 pin，再进入既有维护；维护失败仍返回完整业务结果与独立健康警告，pin 在写保护下也可显式关闭。实际 commit 错误仍可能结果不明，不承诺所有 Err 回滚。

`pw_fskit_node_create` 在核心创建前检查管理+策略句柄总额 4096 并预留原生 token。返回对象、已持有的管理文件 pin 与预验证属性，目录 file=0；失败不写输出。Swift 在修改前分配 pending FSItem/准备 Dictionary 容量，成功后只绑定新 ID/pin、发布 cache 和消费支持字段；不会二次 intern/open。未发布的 object=0 不会交给 FSKit，已发布对象身份不再更改。此处是有界逻辑准入，不是精确堆字节预留；进程 OOM/abort 或 indeterminate commit 仍需依靠原有恢复。

新 API 是可信管理操作，不能把返回 pin 当作 POSIX READ/WRITE grant 或 OS 身份认证。本阶段未支持的创建时 size/v2 time 后续由 M0-Q 接入；符号链接/xattr 仍未完成；access/probe/load/activate/mount 不开放。目录 pin/原生生命周期完整语义和真实挂载必须另测。

新增核心 5/3 项（all-feature/default）：v1/v2、目录无文件 pin、创建后 unlink 保留内容、拒绝/冲突/非法名字无项与父元数据变化、4096 核心总预算、BeforeCommit 回滚无 pin、AfterCommit 维护失败完整回执/可关闭 pin/重开。native 新增 2 项（累计 15），包括管理/策略总预算不足时名字和父属性不变、目录不占文件额度、空输出与 token 类型隔离。真实 C caller 调用原子创建并验证 BUSY/显式关闭；交接程序每进程重新创建临时 pin，unlink 后读回再释放，新增 CREATE_REPLY_HANDOVER_OK。

SDK 合同新增真实 C 预算耗尽场景：在一个缓存文件之外打开 4095 原生句柄，创建普通文件必须在修改前 ENOMEM，字段不消费、名字不存在、父时间不变；目录可创建，释放后文件创建恢复，未 reclaim 的 pin 阻止 deactivate。CI 标记 FSVOLUME_ATOMIC_CREATE_CONTRACT_OK 只代表直接 SDK/ABI，不是 OS 分发或 mount。

Work Linux full default/all-feature workspace（包括已有 Disk Studio 只读检查器）、clippy/fmt、C caller/C++17 header、Python 9 项和本地四阶段程序实际通过。默认测试最初有一个仅故障特性使用的 import warning，已修正并重跑对应默认套件与 lint。准确 macOS SDK/完整 App+Extension、MSVC 与跨 OS 交接结果以本增量 PR/source SHA 为准。没有改变 schema/依赖/工作流，也没有修改 Disk Studio 模块或操作物理盘。原生权限/ACL/共享/缓存/完整同步、真实挂载、Codex 沙盒和物理 exFAT/USB/断电继续 BLOCKED/NOT_RUN。
