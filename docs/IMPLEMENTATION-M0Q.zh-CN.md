# M0-Q：创建时的逻辑大小与初始时间同事务提交

在 M0-P 的预先准入/原子 pin 上，新增 `NodeCreateOptions` / `pw_fskit_node_create_with_state`。普通文件可在创建时具有初始逻辑大小，v2 可显式设置 birth/access/modify time；名字、大小、owner/mode、时间、父目录时间和预验证回执在同一 SQLite 创建事务内提交。不是创建后再调用 truncate/set_metadata，初始 generation 仍为 0。ctime 由核心维护。

## 语义与限制

初始大小是稀疏零区间：读取返回零，未写入内容不产生数据段；按包含旧打开对象的逻辑占用检查配额和 i64 地址边界。它不是物理预分配、宿主容量预约或回收完成承诺。已有写入/截断行为不变；创建本身保留明确请求的 mode，后续内容修改仍清除普通文件 setid。此处是可信管理 primitive，存 mode 不代表已验证执行。

v1 支持其既有 schema 可表达的初始 size，但不接受初始时间/明确 owner/mode，不创建元数据表或迁移格式。v2 时间保持有符号秒与纳秒，非法纳秒在变更前拒绝；目录可有 v2 时间，但任何显式 size（包括 0）明确 unsupported。未知 valid 位、reserved 非零、空 state/output 指针不修改名字/输出。原旧创建 ABI 转发全空 state，维持原行为。

FSKit laboratory createItem 增加这些支持字段；成功后才消费 size/birth/access/modify time，未支持字段继续拒绝。修改前进行时间转换、cache/item 准备、共享句柄额度和 token 准入；失败不发布 FSItem 或 pin，成功后无二次 open/query。原生 caller/umask/ACL/权限委托尚未接通，所有 access/probe/load/activate/mount 保持关闭。

## 验证

新增核心 create_state 4/2 项（all-feature/default）：v1/v2、稀疏跨块零读、无数据段/初始 generation=0、精确时间/owner/mode、重开、非法/配额/目录大小、投影拒绝、BeforeCommit 回滚、AfterCommit 完整已提交状态及可关闭 pin。native ABI 新增 1 项（累计 16），覆盖 flags/保留位/空指针、v1 时间拒绝、逻辑配额、目录 size 拒绝、输出不变、时间及初始零读。真实 C caller 使用新 state API，交接各进程检查初始大小/时间/零读并打印 INITIAL_STATE_HANDOVER_OK。

实际 SDK 合同增加 v2 混合初始 owner/mode/size/三类时间与消费位、初始零读，v1 仅 size、目录 size/非法时间/未知字段的拒绝。原先用于拒绝创建时间的用例改为拒绝仍未支持的 changeTime，不删除负向检查。SDK/完整 bundle/MSVC 的准确通过状态以本增量 PR/source SHA 为准，Work Linux 不预报。

Work Linux full default/all-feature workspace（含 Disk Studio）、clippy/fmt、C caller/C++17 header 和本地四阶段实际通过。初次 check 发现复用 quota 的模块私有范围和示例缺少 import，已修正为 crate 内共享并补 import，未跳过检查。没有修改持久 schema、依赖或工作流。没有真实挂载、driver/签名启用、Codex 沙盒、物理盘/exFAT/USB/断电测试；相关门禁继续 BLOCKED/NOT_RUN。
