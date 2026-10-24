# M1-F：未知 catalog schema 的回收前拒绝

## 修复的问题

普通 `Volume::open()` 之前只检查 application_id、user_version、格式字符串和数据引用，然后启动孤儿清理及段回收；独立恢复工具虽然拒绝 trigger/view，却也没有核验完整 schema。因此版本号未变但出现未知表、缺失索引、额外列或 trigger 时，两条路径可能作出不同判断。尤其不能让普通打开在未知 trigger 下执行 DELETE/GC。

## 实现

新增共享 catalog schema gate。使用锁定的同一 SQLite 引擎在独立内存数据库加载版本控制内的 `schema.sql`，比较完整 sqlite_schema 的 type/name/tbl_name/SQL；忽略页号和实际数据，不执行输入 schema 提供的 SQL。增加 Rust 字符串复制前的条目数和全部 schema 字段字节准入上限。未知结构返回 UnsupportedFormat，不自动修表、删未知结构或升级原件。当前为严格的实验格式匹配，不保证接受人工重建的等价 SQL 或 ANALYZE 新增表。

普通打开在读取 config 表、配置 WAL 和执行自动回收之前验证；恢复工具在其独立工作 catalog 内使用同一门禁。原件保全仍必须使用 Recovery；普通 open 仍是可写恢复 API，SQLite 自己可能恢复 WAL，不能把被拒绝的 open 宣称为原件字节完全不变的只读诊断。

新增回归在一次性容器中保留仍有数据的孤儿与正常文件，离线加入 trigger/view/新表/额外列/缺失索引；打开必须拒绝且不能清理孤儿、执行 trigger 或删除段。Recovery 也须拒绝。合法 SQLite VACUUM 后应正常重开、读取和回收。三平台具体 PASS 以 PR 对应源码 CI 为准。

这只是 schema 结构边界，不是完整恶意 SQLite 解析器预算、catalog 重建、卷加密或阻止有原始目录权限的程序改写容器。名称版本、UUID、能力位、权限、完整元数据、挂载/同步和物理介质测试继续未完成。

## 交接发现与修复

初始 core run 40106719661 功能/clippy 通过但一处测试格式不符；修正后 core run 40106903898 三平台通过。与此同时 handover run 40106903895 的 macOS 阶段通过，Windows 打开 Mac 产物报 UnsupportedFormat，返回检查因上游失败而未运行。不能把单平台绿色扩展成跨平台成功。

精确 SQL 指纹存在源码 checkout LF/CRLF 差异；两个内存 catalog 分别从 LF/CRLF 的同一 schema 创建即可重现比较差异。修复只将 schema SQL 的 CRLF 规范为 LF，并用 gitattributes 固定新 checkout 的 schema.sql 为 LF；不泛化忽略 SQL 内容、列、索引、trigger 或未知表。新增两种来源换行的门禁回归，最终必须重新完成同一容器的完整四阶段交接。
