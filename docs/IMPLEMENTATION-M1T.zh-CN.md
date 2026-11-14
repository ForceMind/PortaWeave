# M1-T：同步时保留 SQLite 的跨进程锁

日期：2026-11-14。实验性存储核心修复，不是挂载或物理断电验收。

## 问题与修复

原 `Volume::synchronize_storage` 在 SQLite 连接仍存活时，通过普通 `File`
额外打开、同步并关闭 `catalog.sqlite`。POSIX 会在关闭同一 inode 的任意
描述符时释放该进程在文件上的记录锁，SQLite 无法得知这个外部关闭。
因此，存储同步可能让仍正常工作的连接失去 catalog 的共享锁。

现由原 SQLite 连接的 FULL 检查点负责 catalog/WAL 同步，保留已有
`synchronous=FULL`、`fullfsync=ON` 和 `checkpoint_fullfsync=ON`。数据段和
segments 目录先同步，再做无打开操作的 catalog/WAL 类型检查、FULL 检查点、
格式文件与容器目录同步。已完成的检查点可以复用同步状态，不承诺每次重新
发出一次设备刷新。详见 [ADR 0021](adr/0021-preserve-sqlite-locks-during-storage-sync.md)。

没有改持久格式、原生 ABI、依赖版本或 `core` 禁止 unsafe 的边界。
只有 Unix 集成测试显式使用锁文件中已有的固定 `libc` 版本。

## 回归方法

在全新、可丢弃的 v1/v2/v3 容器里，父进程持有真实 `Volume`；独立 exec
子进程通过 `fcntl(F_SETLK)` 尝试获取 SQLite shared byte range 的写锁。
仅 `EACCES/EAGAIN` 算锁冲突，其他错误均失败。子进程使用自己打开的
描述符，检查不同 PID；非阻塞尝试、超时和清理避免遗留进程。

验证同步前、同步后、重复已完成检查点后，锁都保持；关闭 owner 后子进程
能够获取锁，作为正向对照。文件内容和旧读取租约也需保持。故障注入版本
另验证 catalog 同步边界失败后写保护、可读内容和锁的生命周期。

修复前失败记录保留在
[`evidence/2026-11-14-catalog-lock-before.log`](evidence/2026-11-14-catalog-lock-before.log)。
本地和三平台 CI 的准确源码、计数及结果以本轮 PR 验收记录为准。

## 保留的边界

- BUSY 不变成成功，也不触发写保护；真实 SQL/I/O/结构错误继续上报并保护。
- 文件句柄与读取租约不被强制关闭，维护不冒充回滚。
- Windows 的该同步入口仍为 UNSUPPORTED；没有启用 WinFsp Flush。
- 子进程验证的是宿主 SQLite 锁，不能关闭真挂载、调用者身份/ACL、系统缓存、
  安全卸载、卷内 Codex/Git 或物理 exFAT/USB 验收。
