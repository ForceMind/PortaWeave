# M1-J：受信任凭据的核心目录权限策略

在既有 v2 元数据上增加 `Volume::posix(Credentials)` 同步 facade，实际约束其 lookup/stat/metadata/access、受限列目录、创建、删除与改名。原 `Volume` 和旧 C API 仍是管理接口；本增量不认证 OS 调用者，不把传入 uid/gid 当作可信身份的来源，不改变既有格式或默认 v1 创建。

## 已实现与拒绝行为

有效 uid、主 gid 与最多 128 个附加组显式提供，复制后规范化，不从宿主/daemon 推断，也没有默认 root。owner/group/other 只选择一组权限，owner 被拒绝时不能回退到 other。显式 uid 0 的权限模型仍要求普通文件存在某个执行位才能申请执行；目录搜索/读写可使用该特权。执行位计算不代表原生程序已从卷加载。

已知目录 ID 也必须检查到逻辑 ROOT 的全部祖先搜索权限，根目录本身同样检查；单次最多 256 目录，不解释宿主路径或符号链接。lookup 只需搜索，列目录还需读；创建/删除/改名需要相关父目录写与搜索，删除不以被删文件可写代替父目录授权。粘滞目录只有项 owner、目录 owner 或本模型的 root 可删除/改名，替换时检查两端，不采用可写目标例外。

创建使用 caller uid、显式 umask；父目录 setgid 时继承父 gid，子目录再继承 setgid。普通文件只接受 0777 权限位，mkdir 另接受 sticky；创建请求的 setuid/setgid 等未支持位拒绝。原管理 create 仍保留其旧默认，不静默改变已有调用语义。拒绝不会创建名字、改变时间/链接/数据、运行维护或锁定写保护。

`PosixSession` 独占借用串行 `Volume`，检查至业务操作之间不允许另一合法核心请求插入。最终变更仍使用现有 SQLite 事务，成功后的维护失败仍单独告警并锁写，不能改报业务回滚。新增 `PermissionDenied` / `PW_DENIED`，Swift 映射 EACCES，WinFsp 契约映射 ACCESS_DENIED，分别测试，区别于 EROFS/MEDIA_WRITE_PROTECTED。

## 验证

新增 9 项核心集成回归（默认特性 8 项）、1 项纯 mode 单元矩阵共 40,960 个断言、1 项 ABI 负向回归和真实 C caller 场景。覆盖权限类优先级、附加组/预算、root 执行、祖先搜索不被已知 ID 绕过、列举与搜索分离、umask/setgid/sticky 创建、父目录规则、两端粘滞替换拒绝不改变 inode、旧打开项保持、深度预算、非法输入及提交后维护告警。

v2 artifact 交接程序新增权限策略检查：有效 ID/附加组读允许、写与创建拒绝、拒绝后父属性/名字不变。Work Linux 本地四阶段只验证程序；实际 Linux→macOS→Windows→Linux 与 SDK/C/完整 bundle 的准确结果由对应 PR 的 source SHA 及 CI 记录，不预先声明通过。

## 仍未完成

OS caller 身份绑定、Windows SID/ACL、原生路径/符号链接解析、chown/chmod 授权、数据句柄的访问能力、内核缓存与真实挂载。当前 FSKit/WinFsp 回调不使用来自未认证对象的凭据，也不因此开放；load/access/mount/Flush 门槛保持关闭。测试传入的合成身份不是系统账户测试，不是 Codex 沙盒负向验收。完整边界见 [ADR 0006](adr/0006-trusted-posix-policy.md)。
