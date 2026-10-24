# M1-I：文件大小与元数据的原子混合变更

新增 v2 `Volume::truncate_with_metadata` / `pw_file_truncate_with_metadata`，在同一 SQLite 业务事务提交大小、尾块归零/引用和 uid/gid/mode/显式时间；generation 只增加一次。避免原生 setattr 使用两个独立提交，导致大小已变但时间/权限设置失败。v1 返回 unsupported，不隐式升级；原普通 truncate 继续支持 v1/v2。

显式 mtime 优先于截断时钟；ctime 始终取本次时间。截断（包括同尺寸请求）与普通文件 ownership 变化均清除 setuid/setgid，即使同一请求提供了这些 mode 位也不能带回；保留执行位等其余位。参数和纳秒范围在数据 staging 前校验，配额/非法参数失败不改变元数据、不锁写。新实现与 metadata-only setter 共用事务内部 helper，setter 的 BeforeCommit 故障也可验证回滚。

活跃旧读取租约仍保留旧内容，不把新 metadata API 冒称成元数据快照。提交前故障保留旧大小/字节/全部属性/generation；提交后维护故障仍返回业务成功，并单独标记维护失败及写保护，重开看见完整已提交配对。实际 SQLite commit 错误依然可能结果不明，不承诺所有错误回滚。

新增 4 项 core 场景（其中 2 项需 fault-injection），元数据套件累计 all-feature 13 / default 10 项：混合变更一次 generation 与旧视图、非法字段/配额无 staging、BeforeCommit 同时回滚（含 metadata-only）、AfterCommit 成功与维护错误分离。实际 C caller 编译调用混合 API，重开核验 size/mode/精确 mtime。原 v1/v2 模型、损坏检查、恢复和跨 OS 链继续运行；metadata_handover 新增同一文件的大小/owner/mode/时间配对及截断再增长读零验证。

Work Linux 完整 workspace 默认/全部特性、clippy/fmt 和真实 C caller 的实际结果记录在对应 PR；Linux 四阶段程序运行不能代替真实 macOS/Windows 交接。三平台准确源码 CI 通过后再合并。无需更改依赖、格式或迁移。

这是供原生适配器使用的受信任存储 primitive，不认证 OS caller，不执行 ACL/路径权限。FSKit 混合 setter 尚未接入，现有拒绝先于截断；load/access/activate/mount 与 WinFsp security/Flush 保持关闭。真实系统挂载、卷内 Codex/开发工具/沙盒负向、物理 exFAT/USB/断电仍 NOT_RUN/BLOCKED。
