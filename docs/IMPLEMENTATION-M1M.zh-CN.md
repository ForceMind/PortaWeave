# M1-M：原子追加、受 EOF 限制的写入与事务内回执

原生写回调不能先查询 EOF，再以该偏移发起第二个写请求；其他请求可能在中间修改文件。新增 `PosixFile::write_request`，用 `WritePosition::Offset` 或 `EndOfFile` 在同一独占 owner 借用内解析位置。`constrained=true` 只覆盖当前 EOF 内的前缀，EOF 及之后返回零；追加加 constrained 也返回零。16 MiB 准入先于裁剪，绝对偏移与实际写入末端须在有符号 64 位范围内。没有伪造空间预留或改变普通 write 的覆盖语义。

`FileWriteResult` 返回实际写入数量、已解析的绝对偏移以及文件身份/大小/link count/generation/元数据。信息在**同一 SQLite 业务事务内、commit 前**取回，随后作为已完成结果保留，不在成功提交后再查询而把已成功写入误报失败。`PosixFile::truncate_request` 采用同样方式返回新文件信息；原 truncate、混合属性和普通 write 共用既有实现。实际 commit 失败仍可能结果不明，后续维护失败继续单独告警并写保护。

`info` 是当前 owner 的一次串行观察，不是持久化元数据快照、OS 缓存失效或 fsync 承诺。零长/完全裁剪写不改变内容、时间、setid 或 generation；与既有空写相同，操作后的维护仍可能发生。数据权限仍取原 READ/WRITE grant，后续 chmod/chown/unlink 不撤销旧授权，纯读 grant 连空写/截断也拒绝。原始管理接口不接收策略 token，v1 不升级。

C ABI 增加 `PwFileInfo`、`PwWriteResult` 和 `pw_posix_file_info/write_request/truncate_info`，均只收策略 token。APPEND 忽略 offset；CONSTRAINED 采用当前 EOF。未知 flags、空输出、过长数据先拒绝；零长允许 NULL data。失败不改输出，指针有效/对齐/不别名契约不变。C ABI 的串行 owner 锁覆盖位置选择、写入、回执构建及输出，不给多线程留下查询/写入窗口。

新增核心 all-feature 7/default 5 项，含 384 步模型、共享句柄、旧范围租约、硬链接与最后 unlink、配额已满时的零写、i64 EOF/溢出、拒绝与失效授权、提交前回滚、提交后成功回执与独立维护故障。新增 ABI 2 项，含 6 线程共 96 个定长记录并发追加，核验每个偏移、完整内容与无覆盖；真实 C caller/C++ 头文件及 v2 artifact 交接也调用新增 API。

Work Linux 与准确源码三平台 CI 的执行结果由 VALIDATION/对应 PR 记录；本地阶段名不是 macOS/Windows 执行证据。Windows WinFsp 文件信息/Write/SetFileSize 回调仍待接入；原生身份/ACL/共享访问、缓存、Flush、真实挂载/Codex 沙盒及物理 exFAT/断电继续 BLOCKED/NOT_RUN。见 ADR 0008。
