# M1-K：打开文件能力与当前属性修改授权

在 M1-J 受信任有效 ID 策略上新增不透明 `PosixFile`，只由通过祖先搜索和当前 mode 检查的 open 产生。显式 Read/Write/ReadWrite 授权不能通过读句柄写入/截断，也不能用纯写句柄读取。FD 数据权限保持到关闭，不因后续 chmod/chown、祖先不可搜索、rename/unlink 自动变化；新 open 仍按当前状态检查。关闭使副本一并失效，其他 Volume 或重开 owner 不能使用旧 token。

属性修改独立检查当前 owner/root，不能从“可写 FD”推导 chmod/chown 权限。非特权 owner 不得改变 uid，显式 gid 必须属于其有效/附加组；设置 mode 中的 setgid 在不属于目标组时清除。普通文件任意显式 ownership 请求都清除 setuid/setgid，包括相同 ID。显式时间修改仍限 owner/root，没有引入 UTIME_NOW/OMIT 或自动 atime。大小+属性同时要求 WRITE 与当前属性授权，再进入 M1-I 单事务，保持一次 generation。

core 与原生接口限制每 owner 4096 个策略 open，计入已有管理句柄；C ABI 使用独立表并与旧表共用总限额。互相混用 token、失效或跨会话 token 拒绝；两表有打开项时 volume_close 都 BUSY。句柄及视图的生命周期仍须显式处理，不能用强制释放伪造安全卸载。写保护时仍可读/关闭有效 grant。

新增 9 项集成回归（默认特性 7 项）、1 项 ABI 回归和真实 C caller：读写 grant 拒绝无副作用、chmod/chown/祖先/名字变化后旧 FD、同卷副本/其他卷/重开 owner 失效、当前属性授权、组与 setid 约束、混合属性一次提交、4096 总预算（含管理句柄）、提交前回滚、提交后维护失败与保护时关闭。交接程序新增受限读 FD、拒绝写及非 owner 属性修改的检查。

Work Linux 已实际运行默认/全部特性 workspace、clippy/fmt、真实 C caller、四阶段交接程序；本地首次 clippy 提示 nested if、C 编译提示新原型先于类型声明，修正后再验证。准确三平台、MSVC、Swift/SDK 和完整 bundle 结果见本增量 PR，不预先声明通过；没有更改依赖、格式或迁移。

凭据仍由受信任适配器传入，不是 OS 身份来源。旧核心/C API 仍为管理接口，不能对不可信调用者开放。原生 caller/SID/ACL、内核权限委托、范围租约授权、真实挂载/Codex 沙盒/物理介质仍未完成，FSKit access/probe/load/activate/mount 与 WinFsp security/Flush 继续关闭。见 [ADR 0007](adr/0007-granted-files-and-metadata-policy.md)。
