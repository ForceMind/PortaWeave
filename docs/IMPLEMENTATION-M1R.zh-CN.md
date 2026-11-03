# M1-R：有界候选清理与现有 owner 维护续办

2026-11-03，接续已核验main `b6498bc1add38fad54649486ed01fcb24fb3b6f4`（14/14检查成功、无开放PR）。云目录被平台清理后，从GitHub恢复187文件并逐一核对Git blob SHA；本地仍无Rust/Swift/MSVC。架构决定见ADR0019。

原orphan/dead查询不再收集整个结果集。每步最多128孤儿候选、两阶段各128废弃段候选、128整理候选，整理复制仍最多256块/16MiB。跳过的活句柄或旧租约同样消耗候选预算，不能为找足128个可删除项而无限扫描Rust行。查询返回数和复制量有上限；SQLite扫描/排序/缓存、对象级cascade、同步等待和全局ABI锁仍非硬时延/内存预算。

三个volatile keyset sweep固定本轮最高ID，新分配的更高ID等下一轮；扫描完成后回绕，避免旧pin释放后被持续增长的数据饿死。半整理源继续分批；HostSpace延期不消耗所选整理位置，恢复空间可直接重试。没有持久化第二套队列，SQLite仍唯一事务权威。普通open验证后仅推进一批，大积压不会宣称已完全排空。老v1/v2/v3格式与默认创建、孤儿目录规则、读者保护不变。

新增Rust `maintenance_step` 和C `pw_volume_maintenance_step/PwMaintenanceReport`，在同一live owner里推进，不新开数据库、不关闭活能力。显式step失败会返回错误，空间延期不投毒，真实维护故障写保护；之前已完成的清理可能已提交，不承诺全回滚。业务操作后的维护仍保留业务成功与独立health。失败C输出不变，NULL输出在维护前拒绝；report是实际完成数量，零/Healthy不等于无积压。BUSY、old leases及管理/策略token规则保持。

新增4个默认core回归：三格式300孤儿/300真实废弃段重开只处理128/256后靠step续办；持续新增高ID时已释放低ID获得重查；保护时拒绝且health不被清空；130个旧段的真实lease跨多页清理保持旧数据并在释放后排空。2个feature回归：显式空间延期/同源重试，显式整理提交前故障作为错误而非业务成功。内部fixture由单owner连接/真实段writer构造，不使用第二catalog writer，不冒充公开业务API模型或物理断电。

新增native回归覆盖三格式、旧lease/Busy、无效owner/已关闭owner/NULL输出和输出哨兵；实际C11 caller在三格式打印 `NATIVE_MAINTENANCE_STEP_ABI_OK` 并检查24-byte wire/计数/拒绝。本地Python19项、严格C11语法和C++17公共头语法通过；无本地Rust执行或新ABI实际链接成功声明。准确source平台测试、C/SDK/WinFsp、完整App与跨OS交接及独立合并main CI须依据对应PR最终记录。

这不是自动空闲timer/worker。下一步需owner事件循环的间隔、前台优先、退出/卸载中断与平台生命周期接入，以及SQLite/WAL维护/空间预约。所有native caller/ACL/Open/Flush/access/load/mount、WinFsp硬链接后端和实际卷Git/SQLite/依赖/Codex正常沙盒/物理exFAT门禁仍BLOCKED/NOT_RUN，无本机、驱动、服务、系统配置或沙盒变更。
