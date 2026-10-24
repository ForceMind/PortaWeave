# M1-B 第一增量：独立诊断/导出与维护错误分离

日期：2026-10-24。承接 `b12c069cd6d33b683a16a42598950f1338fbc42f`。本轮不是完整 M1-B、系统挂载或稳定版；Issue #1/#3 仍需继续。

## 独立恢复路径

`Recovery::prepare(source, new_workspace, limits)` 不调用 `Volume::open()`。先以只读文件描述符获得源容器独占锁，拒绝活动卷；将原始目录中支持的普通文件和 segments 内文件复制到 `evidence/`，包括 SQLite/WAL/SHM、未引用段及未知普通文件，逐文件记录 SHA-256 与长度。符号链接、设备或额外嵌套目录会拒绝，不跟随它们逃出源目录。

完成快照后释放源锁：诊断不依赖原件继续存在，也不阻止用户随后正常打开原件。SQLite 只打开 `catalog-work/` 下的第二份数据库与 WAL；保全的 evidence 不交给 SQLite 恢复或 checkpoint。工作视图设置 query_only；发生恢复写入、重建 WAL 索引及关闭检查点也只影响工作副本。

不宣称硬件级只读：文件读取可能改变宿主 atime。源目录、工作目录和导出目录都要求处于可信、非恶意并发改写的命名空间；本轮不提供抵御原始文件 TOCTOU 攻击的沙盒边界。

## 校验与安全导出

校验格式/application_id、SQLite 完整性/外键、对象类型和长度、名称比较键、目录连通性/深度/循环、块范围与 SHA-256。无名文件对象保留并计数，不执行自动 GC；正常目录导出不猜测其旧文件名。损坏引用默认使严格校验/导出失败，未实现逐文件容错抢救或重建损坏 catalog。

导出必须是全新目录，其下 `files/` 保存文件，`report.txt` 记录范围和差异。默认创建真正的宿主硬链接，失败不静默复制；显式 `--copy-hardlinks`/`ExportLinks::CopyAliases` 才允许独立副本。空洞读取为零；限额计算完整逻辑长度，防止巨大空洞绕过导出预算。目标已存在或落在源容器/诊断工作区内都拒绝。

默认限制：证据快照 64 GiB、导出逻辑数据 64 GiB、catalog 总行数 100 万、目录深度 256。Rust API 可以显式设置限制；CLI 不隐式提高它们。复制工作 catalog 也使用独立的快照上限预算，但该上限不是全部工作区物理空间上限，SQLite 恢复还可能需要额外空间。源介质损坏、空间不足、权限错误均保留已有证据，不自动删掉失败成果。

只有 `COMPLETE` 存在且 `INCOMPLETE` 不存在才是导出完成；快照相应检查 `SNAPSHOT_COMPLETE` 和 `SNAPSHOT_INCOMPLETE`。这里是内容导出，不是元数据完整备份：ACL、xattr、ADS、时间戳、符号链接等尚未在核心实现。需要人工另存完整容器备份，不能拿内容导出替代尚未实现的完整语义迁移。

## 成功提交与维护失败分离

原实现把业务 action 和 maintain 通过 `and_then` 相连，导致成功写入之后 GC 失败也返回一般错误，调用方可能重复提交。本轮改为：

- 业务 action 失败：返回实际错误，必要时进入写保护。底层 commit 报错仍可能结果不明，不承诺全部回滚。
- 业务 action 已成功：返回业务结果，包括新建文件 ID、写入长度或目录操作成功。后续维护错误写入 `MaintenanceStatus::Failed`，Stats 同步提供该状态，并锁定写保护。
- 只读查询和关闭句柄仍可进行；写保护不由一次状态读取解除。正常重新打开会先校验、再恢复维护；如果仍失败，必须继续显示状态，不能声称已修复。

`AfterCommit` 故障点统一位于成功 action 后，用来验证此边界；进程退出仍直接终止，不伪造 rollback。操作系统适配器未来必须同时显示维护警告和卷保护状态，但不能把已成功的文件系统操作改报失败。

## CLI

需要克隆仓库并使用锁定的 Rust 工具链构建；不是已签名的安装包。命令中的路径为一次性测试数据，工作区和输出必须不存在：

```sh
cargo run --locked -p portaweave-core --bin portaweave-recover -- verify SOURCE NEW_WORKSPACE
cargo run --locked -p portaweave-core --bin portaweave-recover -- export SOURCE NEW_WORKSPACE NEW_OUTPUT
cargo run --locked -p portaweave-core --bin portaweave-recover -- export SOURCE NEW_WORKSPACE NEW_OUTPUT --copy-hardlinks
```

## 验证与边界

新增回归覆盖：原件/evidence 的全量内容哈希不变；空文件、中文目录、空洞、真硬链接/显式复制；重复/嵌套目标拒绝；活动所有者拒绝；源重命名后继续导出；预算与深度限制；损坏校验、路径穿越、无名孤儿保全；CLI；提交前错误/提交后警告；子进程异常终止后从原始 WAL 副本恢复正确版本。

首轮源 `a0302e1` 在 Linux 上通过 28 项既有存储测试、16 项新恢复测试和 2 项 owner 测试，但静态检查因 `&mut u64::MAX` 的常量临时值写法失败；保留 run 40077681605 记录，修正为实际局部复制预算后重新验证，不能将首轮描述为全部通过。最终结果以 PR #4 精确提交的 Actions 和 VALIDATION 为准。

尚无实际 exFAT 设备、物理断电、FSKit/Windows 挂载、真实 Codex 沙盒、权限/xattr 或完整 ENOSPC/EIO 故障矩阵。自动维护仍为操作触发的串行实现，没有新增空闲线程。

## 参考

SQLite 官方说明 WAL 读者、WAL/SHM、只读打开与恢复限制：
https://www.sqlite.org/wal.html
https://www.sqlite.org/uri.html

本方案不使用 `immutable=1` 打开活动或可变原件，也不丢弃 WAL 以换取“只读打开成功”。这些官方说明不能替代实际故障与数据保全测试。
