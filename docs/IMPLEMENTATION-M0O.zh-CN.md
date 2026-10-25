# M0-O：FSKit setattr 直接使用事务内属性回执

原 FSVolume.setAttributes 在完成大小/属性修改后再次调用 attributes 查询，构造 SDK 回执。该查询若失败，已经提交的业务可能被报告为失败，且此前已标记的 consumed 字段与错误回调矛盾。本增量把回执读取和所有可能拒绝的数量/时间转换移到业务 commit 前，成功后仅构造 SDK 对象、消费已实现字段和报告独立健康告警。

## 共享核心与原生接口

新增可信管理观察 `ObjectInfo`，明确 v1 的 metadata=None，不伪造 uid/gid/mode/时间。`set_metadata_with_reply` 和 `truncate_with_reply` 在同一业务事务内读取对象状态，再运行纯的可失败投影。拒绝投影或 BeforeCommit 故障回滚 size/blocks/元数据/generation；AfterCommit 维护失败继续返回完整业务回执，并单独警告/写保护。原普通/混合 API 及格式规则保持，v1 仅允许空 metadata 的 legacy size 操作，无自动迁移。

`pw_fskit_metadata_set`、`pw_fskit_file_set_attributes` 返回预验证的 PwFsKitAttributes。它们仍是**管理接口，不提供调用者授权**；file setter 只收管理 token，不能混用策略 grant。必须提供 expected object，句柄/对象不符在 commit 前拒绝。空输出/非法字段先拒绝；失败输出不变。metadata-only 空请求是观察，v1 保持 metadata_valid=0，不启动维护。

投影在提交前校验 file link count 的 UInt32 范围及所有时间的 native Int/纳秒范围；Core ROOT=1 映射 FSKit rootDirectory=2，UID/GID 和纳秒不经过浮点数。Swift 在修改前验证本 item 的 wireID；C 层在提交前验证实际对象一致。提交后没有再次查询数据库、throws 转换或失败分支；受支持的 macOS 64-bit target 与 native 模块保持同架构。返回字段仅为已实现的 type/size/fileID、普通文件 linkCount 和 v2 元数据。

## SDK 接入与验证

FSVolume.setAttributes 的大小/混合/metadata-only/空请求统一使用新接口；未支持属性仍先拒绝，只有成功提交才消费字段。实际 SDK 合同新增 root 空请求身份/属性/不增加 generation、目录不伪造 linkCount、metadata-only 完整回执纳秒精度，以及 v1 size 回复不伪造元数据；保留全部旧 mixed setter/非法时间/不支持字段无变更测试。

新增核心 4/2 项（all-feature/default），包含 v1/v2、root、混合/metadata-only、投影拒绝、旧租约、BeforeCommit 回滚与 AfterCommit 独立维护故障/重开；native ABI 新增 2 项（累计 13），覆盖管理/策略 token 隔离、expected object 错配回滚、空指针、v1 不升级及精确属性。真实 C caller 调用新接口；跨 OS 交接程序比较 mixed/metadata-only 回执与同一容器状态，新增 ATTRIBUTE_REPLY_HANDOVER_OK。

Work Linux 完整 default/all-feature workspace、clippy/fmt、C caller/C++ header 及本地四阶段程序实际通过。macOS Swift/真实 SDK 对象运行与完整 App/Extension 构建，必须看本增量准确源码 PR 的 CI；Linux 不预报为 PASS。所有 setter 仍只用于 laboratory contract；FSKit access/probe/load/activate/mount 不开放，原生 caller/ACL/缓存/完整同步、真实挂载、Codex 沙盒及物理介质门禁继续未通过。该修复也不承诺 create/intern 等所有后续路径已获得相同的事务回执设计。
