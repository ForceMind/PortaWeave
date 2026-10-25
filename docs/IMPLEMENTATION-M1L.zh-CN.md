# M1-L：受限读能力的不可变范围租约

M1-K 的不透明文件新增 `acquire_range_view`，只有已授予 READ 的有效文件才能申请，包括空范围。只写 grant 在复制索引或占用租约预算前拒绝。`PosixReadView` 仅保存私有核心视图 token，复用已有范围索引、原 EOF、校验和与旧段保留，不另建存储或改变格式。

租约使用绝对偏移，不能读取其范围之外；授权后持有已准入的不可变内容，原 FD 关闭、chmod、覆盖/截断/unlink 和前台 GC 都不会改变旧数据，直到显式释放。属性不是租约快照。失效、副本关闭、跨 owner 或同进程重开 owner 后的 token 拒绝。所有能力都仅在当前进程/owner 生命周期有效，不写入容器或在跨系统交接中序列化。

策略与管理租约共用核心的活动数/复制索引预算，但 C token 分表，不能通过管理 API 使用策略 view 或反之。关闭源文件后，策略 view 仍使 volume_close BUSY；写保护下可以读取和释放。预算/范围拒绝不锁写，不泄露输出或留下活动 view。预算不是精确堆字节或物理段空间上限；这也不是 OS mmap。

新增核心 all-feature 6/default 5 项、原生 ABI 1 项和真实 C caller：只写/空范围拒绝、原数据跨修改与最后句柄关闭保留、原 EOF/绝对边界/溢出、两类租约共享活动/索引限额、失效/其他 owner、保护时释放、token 混用/失败输出、仍有 view 时 BUSY，以及最后释放后可回收。v2 交接程序在每个 OS 阶段新建授权/租约，关闭源 FD 后覆盖文件，验证旧范围仍返回原字节，恢复当前内容后再释放。

Work Linux 完整 default/all-feature workspace、clippy/fmt、C caller、C++ header 和本地四阶段程序实际运行。跨 OS、MSVC、SDK/bundle 的准确源 SHA/结果看对应 PR，未完成时不预报通过。原生 OS 身份、ACL/内核权限委托、真实挂载/可执行 mmap/Codex 沙盒和物理 exFAT/USB/断电仍未验收，所有 native 门槛保持关闭。
