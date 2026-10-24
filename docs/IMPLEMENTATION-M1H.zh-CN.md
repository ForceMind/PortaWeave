# M1-H：显式 v2 卷身份与 POSIX 元数据基础

新建 API `Volume::create_with_metadata` / `pw_volume_create_with_metadata` 创建独立实验 v2 格式；旧 `create` 仍创建 v1，现存 v1 不自动升级、不伪造缺失属性。v1 元数据请求返回 unsupported，基本读写继续支持。架构选择见 [ADR 0005](adr/0005-opt-in-metadata-format.md)。

## 已实现

v2 持久 UUID、明确 capability、逐对象 uid/gid/mode、创建/访问/修改/变更时间。时间使用有符号秒和纳秒，不经浮点数转换。元数据与文件内容/名字空间共用 SQLite 事务；硬链接共享身份与元数据；替换后的旧打开对象保留其内容和元数据，最终关闭回收时级联清除。写入/截断更新 mtime/ctime 并清除普通文件 setuid/setgid，不清除执行位；空写不更新时间。新建/删除/链接/移动更新父目录时间，case-only rename 也使用事务。chmod/chown/显式时间变更更新 ctime；普通文件 ownership 改变清除 setuid/setgid。

根所有者与权限由创建者显式提供，子项也有带属性的创建 API。旧 convenience create 在 v2 中继承父 uid/gid，并取 file 0600 / directory 0700；不把宿主用户、umask、setgid 继承或调用者授权伪造成已实现。没有自动 atime 更新，访问时间可显式设置。存储执行位不代表系统已可执行。

准确 schema gate 同时识别 v1/v2，仅归一化 SQL 源的 CRLF；marker、config、user_version 必须一致。v2 身份、逐对象覆盖、字段范围在任何普通 open 回收之前验证。Recovery 在单独工作 catalog 检查同样约束，新表行数计入预算，不打开原件 SQLite。恢复导出仍是内容/名称/选定链接语义，报告明确不导出或应用 uid/gid/mode/时间，原始 evidence catalog 保留这些元数据。

C ABI 添加固定大小字段、必须为零的 reserved 和明确 valid mask；不改变旧 PwStat 布局。未知字段、非法 mode/纳秒、空指针在修改前拒绝；失败输出保持原值。这些接口是受信任的存储调用，不认证 OS caller，不实施有效权限。

## 验证与限制

本地 Work Linux 实际运行：9 项新增 core 回归（默认特性 8 项）、新增 native ABI 回归、完整 all-feature/default workspace、clippy/fmt 和真实 C caller。覆盖 v1 原格式继续读写、v2 重开 UUID、u32 最大 uid/gid、负秒/纳秒精确保持、非法字段无变更、链接/替换身份、内容事务失败连同元数据回滚、损坏元数据在 GC 前拒绝、恢复原始字节保持及新增表预算。

`metadata_handover` 复用真实产物交接链 Linux → macOS → Windows → Linux，检查 UUID、uid/gid/mode、时间纳秒与同一硬链接内容。Work 本地按四阶段运行只验证流程，不冒称在本地运行了其他 OS；准确三平台结果以 PR 的源码 SHA 与 CI 为准。

未实现有效权限/ACL、符号链接、xattr/ADS、FSKit 属性接入、持久命名卷标签及升级工具；FSKit load/activate/mount/access 和 WinFsp 安全/Flush 继续关闭。真实挂载、Codex 正常沙盒及越界负向测试、真卷开发工具、物理 exFAT/USB/断电继续 NOT_RUN/BLOCKED。这不是可安装工作盘版本。
