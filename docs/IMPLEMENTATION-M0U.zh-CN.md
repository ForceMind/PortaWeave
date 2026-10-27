# M0-U：当前目录属性与显式原子重建

日期：2026-10-27。重新通过 GitHub 连接读取 main `2dca20f5630c73e54fdbab9d2bac7adf4e543187` / tree `2e6ef7e23d2b5b531ef6d43abd918cdfea2955ec`，14 项检查成功、无未合并 PR，#1/#3/#28 保持开放。继续 M0-T，不更改格式、schema、依赖或工作流。见 [ADR 0014](adr/0014-current-directory-info-and-atomic-replacement.md)。

## 当前属性与旧快照

`PosixSession::directory_info` 重新检查所选目录 READ+SEARCH 和全部祖先 search，再从当前 catalog 读取 FileInfo。新增 C `pw_windows_directory_info` 从 typed snapshot 取已捕获的对象 ID，按本次显式可信凭据检查并投影当前 Windows 时间/逻辑大小。它不遍历子项，所以子文件存在不能表示的 FILETIME 不会妨碍目录自身的有效属性；目录自身的转换失败明确拒绝，不返回缓存值。

旧授权快照保留的是已经取得的名字/属性数据，继续可读；新的当前属性查询不是该快照的未来路径授权。chmod、改属主或移动到不可搜索的祖先后，新查询按当前规则拒绝；旧路径被同名新目录替换不会改变已选 ID。对象被删除后查询返回 NOT_FOUND，原始快照仍可重放直到 close/replacement。v1 仍不支持凭空生成元数据。

实际 WinFsp GetFileInfo 回调的目录分支仅接受非 NULL 的、生命周期有效的实验凭据；没有 caller 时 ACCESS_DENIED，混合文件/目录 context 为 INVALID_PARAMETER。成功按 DIRECTORY、零目录大小/逻辑分配、稳定 ID/时间投影，HardLinks/EaSize 仍为 0。只有成功才改完整输出；没有 POSIX mode 到 Windows ACL 的伪映射。普通文件原有 grant 分支保持。实验 READ+SEARCH 不是 Windows READ_ATTRIBUTES/SID/ACL 授权。

## 显式重建与额度

`pw_windows_directory_refresh` 在一次 native owner mutex 请求中准备同一 ID 的新授权快照，完成 COUNT、全部 UTF-16/时间/大小转换及新 token 预留，再替换 table 中的旧 token。失败不改旧 token/快照/两个输出，也不驱逐其他 cursor 或文件 pin；成功返回新 token 和与新列表一起捕获的目录属性，旧 token 失效。token 仍为进程内、typed、volume-scoped，阻止 volume close。

替换不增加已发布 cursor 数，累计条目预算仅抵扣自己的旧条目，可在共享 64 cursors/100000 retained entries 满额时替换自己。旧副本和准备中的新副本会暂时同时存在，因此不是精确 heap、峰值内存或 OOM 保护。新快照若超过抵扣后的预算，旧副本照常保留；没有静默清空目录或部分发布。

成功重建后调用方必须丢弃旧 ordinal cookie 并从新 token/offset=0 开始。旧 token 与旧 cookie 不能续读；若调用者故意把旧 ordinal 配给新 token，API 无法辨别其来源。WinFsp ReadDirectory 的 NULL Marker 不自动重建，仍是同一快照重放；这里没有对并发 C context token 做未同步交换，也没有宣称已实现内核缓存/通知。实际 C 合同按顺序显式切换 synthetic context，SDK Swift 合同在满额下替换 external snapshot，并验证其余 LRU/外部状态仍可用。

## 尚缺的活目录语义

本轮代码复核确认：现有 v1/v2 rmdir/目录替换事务立即删除该目录 inode；普通 verify 与原件保全 Recovery 都拒绝非根目录的零 namespace link。若简单让 snapshot pin 住 catalog inode，崩溃/交接后会成为现有 reader 拒绝的容器。完整持久孤儿目录、原格式 reader/backward compatibility 和恢复规则必须单独决策，不能暗中放宽校验或迁移唯一容器。

因此当前属性接口是 **linked-directory observation**，不是删除后 fstat、活目录 open grant、继承授权或原生目录生命周期认证。rmdir 不因旧 snapshot 被永久禁止，删除后也不拿旧属性伪造当前 inode。完整目录 Open/unlink 后句柄、原生身份/ACL/共享/缓存/同步/安全卸载仍须实现。所有原生 security/Open/access/load/activate/mount/Flush 门禁继续关闭。

## 验证

在现有 directory_snapshots 增加 2 项 core 测试（累计 all-feature 6/default 5），并强化保护下当前属性观察；覆盖当前 R+X/全部祖先、类型、稳定 ID、旧名替换、删除后拒绝、旧快照不变/无 pin 和重开校验。ABI windows_directory 新增 4 项（累计该文件 12 项/native integration 35 项），覆盖当前属性/子项分离、权限/投影拒绝输出哨兵、原子成功、old token 失效、真实 64 mixed cursors 满额替换、不驱逐其他 cursor、4 个真实读线程与替换竞争。已有 entry-budget unit 降至 3 的真实卷用例强化抵扣边界及超限拒绝，不虚增单元测试数量，不冒称 100000 个名字压力测试。

首次新增 ABI 测试把 NULL credentials 预期写成 DENIED；既有共用凭据解析返回 INVALID，两处断言与公共头说明已修正，保留缺失凭据拒绝及输出不变，没有放宽实现。WinFsp callback 自己的 NULL caller 明确 ACCESS_DENIED。

Work Linux 的 full default/all-feature workspace、clippy/fmt、真实 C11/C++17/Python、四阶段交接结果见 VALIDATION 和对应 PR；准确源码的 Windows MSVC 新标记 `WINFSP_DIRECTORY_REFRESH_CONTRACT_OK`、实际 Swift/SDK 标记 `FSVOLUME_DIRECTORY_REPLACEMENT_CONTRACT_OK`、完整 App/Extension 与跨 OS `DIRECTORY_CURRENT_INFO_HANDOVER_OK` 必须由相应 CI 运行验收，不从 Linux 预报。

真实挂载、Codex 正常沙盒/禁止越界（含 backing 目录）、Git/依赖/编译/IDE/SQLite 真卷、物理 exFAT/USB/断电仍 BLOCKED/NOT_RUN。没有操作用户 Mac/PC、安装驱动、启用扩展、降低沙盒或操作真实重要文件。
