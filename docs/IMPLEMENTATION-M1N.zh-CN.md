# M1-N：打开文件的当前属性授权与事务内回执

日期：2026-10-26。继续 M1-K/M 与既有元数据事务，不改 v1/v2 格式、schema 或依赖。默认创建仍为 v1，元数据策略只用于显式 v2。

`PosixSession::set_file_metadata_request` / `set_file_metadata_with_reply` 在仍有效的策略文件上执行当前 owner/root、uid/gid 与 setid 规则。既有只读数据 grant 不妨碍当前所有者设置属性；旧 WRITE grant 不能在 chown 后绕过新所有者权限。此处的凭据仍是受信任输入，不是 OS 调用者认证、Windows WRITE_ATTRIBUTES/ACL 或文件执行证明。

已经打开的 inode 不需要重新查找当前路径或祖先。权限、名字、父目录或最后一个链接改变后，仍可由当前属性权限主体操作这个活 inode；文件最终 close 后，失效 token 不能重新获得属性权限。数据 grant 与不可变范围租约的原有生命周期保持。空属性更新只观察活文件，不改变 generation/ctime；与既有空更新相同，后续维护仍可能发生。

所有请求字段先校验/授权，再复用同一核心元数据事务：属性、generation 及 `FileInfo` 在提交前形成。纯平台投影可以拒绝并回滚，不能在其中执行外部副作用。成功提交后的维护失败返回完整业务回执，并独立记录健康告警及写保护；真正的 SQLite commit 失败仍可能结果不明。

新增通用 C `pw_posix_file_set_metadata_info` 只接受 policy token，先校验输出、凭据、字段及类型，失败输出不变。Windows M0-R 在同一核心入口内执行可失败的 FILETIME 投影。管理接口不接受新策略 token，策略接口也不接受管理/外来/关闭 token。没有增加持久授权、IPC 或原生访问入口。

新增核心测试 all-feature 5/default 4 项：只读 owner/换主拒绝、无路径活 inode、组和 setid、无效/外来/关闭 grant、投影拒绝、提交前回滚、提交后维护故障与保护下 close、重开保留。C ABI 增加通用字段/输出/token 检查，真实 C caller 调用新接口；v2 artifact 每个系统阶段重新建立本地 grant，输出 `DESCRIPTOR_METADATA_HANDOVER_OK`。准确执行结果见 VALIDATION 和对应 PR，Work Linux 的阶段名不等于其他 OS 已运行。

原生身份/SID/ACL/继承/共享、符号链接/xattr/ADS、锁/监听/mmap/执行、完整同步/缓存/安全卸载、真实挂载/Codex 正常沙盒及物理 exFAT/USB/断电仍未验收。FSKit access/probe/load/activate/mount 与 WinFsp security/Open/Flush 保持关闭。
