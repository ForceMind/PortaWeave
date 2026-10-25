# PortaWeave Disk Studio（磁盘工作台）— 产品与工程规格 v0.1

日期：2026-10-25。状态：**扩展方向已确定；仅 M0 只读分区镜像检查器进入研发**，不代表 DiskGenius 全功能已实现。跟踪：Issue #28。

## 架构：绝不把“文件卷”当作“磁盘分区”

PortaWeave 原有 .pwvol 是运行于 exFAT 上的动态文件系统容器，Rust + SQLite catalog + 追加式数据段；它不是块设备。Windows WinFsp 和 macOS FSKit 文件系统适配器不自动提供可由磁盘工具重新分区的 LBA 原始磁盘。因此要求将 Disk Studio 单独做成三条能力线，不破坏原有格式、自动回收、FSKit/WinFsp 及 Codex/Git 开发盘验收。

**A. 文件卷逻辑子卷：**同一 .pwvol 内规划多个独立命名、配额、挂载位置的工作卷。可以创建、删除、扩容、缩容配额和管理占用，但这不是 GPT/MBR，不能误叫系统可重分区的磁盘。

**B. 真实虚拟块磁盘：**未来独立 .pwdisk 动态镜像，有 512e/4Kn LBA 读写、写时复制、持久化映射、空间回收、主/备 GPT、MBR、镜像备份和离线分区编辑。需要专门的块设备/磁盘镜像对接才能成为操作系统可以再分区的磁盘；现有 FSKit/WinFsp 文件挂载不能直接代替这个层次。分区内若格式化 NTFS/APFS/ext4，仍必须面对跨系统读写驱动不对称问题，GPT 支持不等于客体文件系统通用。

**C. 真实物理磁盘：**采用独立受限的系统能力。Windows 优先 Storage Management API，不以过时的 Virtual Disk Service COM 作为首选；macOS 用 Disk Arbitration 做设备发现、挂载/卸载协调和相应平台受权命令。任何会写磁盘的动作都需要可核验设备标识、显式批准、可靠异盘备份、独占/离线状态、故障恢复计划、特权 helper 最小授权与审计，不能从普通文件卷权限顺带获得真实磁盘写入权。系统盘、EFI、加密/RAID、唯一数据副本默认拒绝高风险修改。

## 与 DiskGenius 的能力对照

| 类别 | 最终希望覆盖的能力 | 分期 | 现在状态 |
|---|---|---|---|
| 分区浏览 | GPT/MBR、主备表、校验、分区类型、GUID、名称、起止 LBA、交叠 | M0 | 普通 RAW/IMG 镜像只读解析原型 |
| 子卷管理 | 逻辑子卷新建/删除/更名/配额、多卷挂载 | M1 | 未实现 |
| 分区表变更 | 创建/删除/移动、GPT/MBR 转换、隐藏/激活、修改 GUID/类型 | M3 | 未实现 |
| 文件系统 | FAT32、exFAT、NTFS、APFS、ext4 格式化和文件访问 | M4 | 必须逐格式、逐 OS 验证 |
| 无损调整 | resize、extend、shrink、split、merge | M4 | 依赖 FS-aware 引擎，不能只改分区 LBA |
| 克隆镜像 | 整盘/分区备份、复制、增量镜像、VHDX/VMDK/VDI/qcow2 | M2/M4 | 未实现 |
| 数据恢复 | 丢失分区扫描、RAW 文件恢复、数据导出、分区表修复 | M5 | 只读扫描优先 |
| 实体磁盘工具 | SMART、坏扇区扫描、擦除、扇区编辑 | M5/M6 | 未实现，危险操作需要分级授权 |
| 高级 | 动态盘、RAID、BitLocker、Storage Spaces、系统迁移/引导修复 | 长期 | 独立研究，不保证全部可跨平台 |

DiskGenius 官方将这些分为分区管理、磁盘操作、虚拟磁盘/镜像、扇区工具、备份和数据恢复；这里参考功能分类，不复用其源码或受保护 UI。

## 本轮 M0 只读检查器

模块：crates/partition-inspector；命令：pw-partitions inspect IMAGE [--sector-size 512|4096]。

- 只读打开用户指定的**普通 RAW/IMG 文件**，不支持真实盘符、Unix block/character 设备、目录或符号链接；不能分区/格式化/克隆/修复，也不访问现有 .pwvol 容器。
- MBR：签名、最多四个主分区、类型/活动标记、范围与重叠。扩展分区仅识别并警告：EBR 逻辑链尚未扫描；拒绝混合 protective MBR/GPT 的歧义布局。
- GPT：保护性 MBR；primary 和 backup header、CRC32、entry-array CRC32、分区/磁盘 GUID、UTF-16 分区名、可用范围、重叠及溢出。双份有效且一致才是 Complete；只有一份有效则 Degraded 且**仅展示，不做自动修复**；双份都坏返回错误。
- 硬性输入预算：每份 GPT 分区数组最多 4 MiB；扇区 512/4096 B 显式指定；所有长度与 LBA 检查。错误不能被转译为“空磁盘”。
- 无第三方运行依赖，CI 上测试合成镜像、4Kn、MBR 重叠、hybrid 拒绝、主备 GPT 分别损坏、原件哈希/内容不变等。当前不处理 VHDX 等封装文件，不遍历 EBR，不浏览分区文件系统。

## 安全写入门禁（M2 以后才允许）

1. 操作先生成 Preview/Plan，展示受影响扇区、前后 GPT/MBR、分区内文件系统、4KiB 对齐、数据量、备份/回滚要求，默认不执行。
2. 虚拟镜像只对**新建的工作副本**写入，写完校验两份 GPT/文件系统再安全切换；写入的旧唯一副本永不提前回收。
3. 真实物理盘独立授权和受限 helper；禁止后台自动执行分区/格式化/恢复/覆盖克隆。操作前备份到另一物理设备、锁定/卸载、明确目标物理设备身份，拒绝系统分区无授权修改。
4. 调整分区大小必须同时调整客体文件系统元数据，不能只移动 GPT 起止 LBA。任何突然断电、控制器错误、坏扇区和外部写入都可能破坏数据，不能承诺绝对无损。
5. 实测主/备 GPT 恢复顺序、扇区短写/撕裂、空间不足、故障注入和专用媒体断电，记录实际结果，不使用 CI 普通文件测试冒充可靠性证明。
6. 不影响现有 .pwvol 单卷工作盘发布门禁，包括 Codex、Git/worktree、npm/pnpm、uv/Python、Cargo、IDE、mmap、锁/通知/权限的真挂载测试。

## UI

左侧为“虚拟工作卷 / 虚拟磁盘镜像 / 物理磁盘”分组；顶部显示分区布局条；中间显示分区和文件系统详情；右侧显示备份状态、只读/可编辑权限与健康信息。变更操作进入可预览、可审计的队列；真实设备写入以明显危险确认与独立授权为前提。第一版不制造虚假的“修复已完成”提示。

## 里程碑与参考资料

P0 只读 RAW/IMG 分区镜像探测；P1 逻辑子卷；P2 动态块设备 .pwdisk；P3 镜像分区操作；P4 文件系统感知调整/克隆；P5 只读恢复/设备健康；P6 经审计的物理磁盘操作；P7 高级系统迁移/复杂阵列。原有工作盘主线不因新模块停滞。

资料：
- DiskGenius 官方功能：https://www.diskgenius.com/manual/ 和 https://www.diskgenius.com/partition-management.php
- DiskGenius resize 风险：https://www.diskgenius.com/manual/resize-partition.php
- UEFI 2.11 GPT 规范：https://uefi.org/specs/UEFI/2.11/05_GUID_Partition_Table_Format.html
- Microsoft Storage Management API：https://learn.microsoft.com/en-us/windows/win32/vds/virtual-disk-service-portal
- Apple Disk Utility 分区与 APFS：https://support.apple.com/en-gb/guide/disk-utility/dskutl14027/mac
- Apple Disk Arbitration：https://developer.apple.com/library/archive/documentation/DriversKernelHardware/Conceptual/DiskArbitrationProgGuide/Introduction/Introduction.html
- GNU libparted：https://www.gnu.org/software/parted/api/

外部资料不证明我们已经实现或测试了这些能力。没有复制 DiskGenius 软件内容或第三方私有源码。
