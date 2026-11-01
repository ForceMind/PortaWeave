# M1-Q：整理复制的独立有界准入

日期：2026-11-02。延续既有SQLite单owner、不可变段及M1-C的空间检查，不改变v1/v2/v3、schema、公开ABI或默认创建行为。

原compact_one将全部候选段ID复制进Vec，并按选中段的全部当前块分配payload；它依赖写入侧大小限制，未给整理侧独立的复制上限。现候选ID通过SQLite row iterator逐项检查，找到第一个未被旧读者pin住的候选后关闭statement。仍按原ID顺序及有效块≤50%条件选择。

每次整理最多读取/复制256个64KiB活块，即16MiB数据加24字节段头。较大有效段可分多次owner操作完成；每批新段仍必须完整写入并同步、空间准入成功，然后在同一SQLite事务内校验并切换本批引用。没有改成原地覆盖，旧段仅在所有当前块引用消失且没有旧读者需要它时才能删除。中间批次不改变逻辑文件ID、大小、generation或元数据。业务成功与后续维护失败仍分开，故障只读保护和重开清理路径不变。

这只约束Rust整理候选ID的物化及单批payload/索引数量，**不是精确堆内存上限、数据库查询时间预算或全维护有界调度**：SQLite GROUP BY/排序/缓存、pinned集合、孤儿对象与dead_segments的全量列表和循环仍有独立成本。每次变更仍最多整理一个候选的一批；本轮没有空闲worker/定时器、卸载取消、全局空间预约、WAL/vacuum调度或应急预留。不能把16MiB复制限制称为全部GC已完成。

测试在单owner内部通过实际段writer构造514块、257活块的有效大段，未使用外部catalog写入器、未放宽公开MAX_IO。当前公开写入通常生成更小段，该fixture验证整理不依赖这一偶然前提。默认测试跨v1/v2/v3：旧lease时不整理，release后只搬256块且原段保留，下一次owner操作搬最后一块才删除原段，内容/metadata/身份不变。all-feature另测BeforeCompactionCommit失败保持全部旧引用、业务成功而maintenance失败/保护、正常重开丢弃未引用新段并分批续办。不是新的物理断电或原生挂载测试。

Work本地无Rust工具链，源码经审查但新增Rust测试尚未本地执行；准确source三平台core/default/all-feature/clippy/fmt、原生ABI/SDK/App与跨OS交接须由对应PR CI核验。Python19项仍只属于测试器，不证明Rust新代码通过。所有原生权限/Open/Flush/access/load/mount门禁保留；WinFsp硬链接、驱动/签名/授权、真实卷Git/SQLite/Codex和物理exFAT继续BLOCKED/NOT_RUN。后续仍需为全量清理和空闲续办实现独立预算及生命周期接入。
