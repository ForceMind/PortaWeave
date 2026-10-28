# 验证方法与证据状态

日期：2026-10-24。已有实验性持久化核心和独立诊断/内容导出，但**仍没有可挂载的 PortaWeave 文件系统**。按源版本记录证据；后续文档提交/合并的精确 SHA 与最终 CI 状态在相应 PR 验收评论补记，旧版本通过不自动覆盖新版本。

## 证据分层

1. 普通宿主目录探针：只测试宿主系统 API 和探针自身。
2. 共享核心：读写真实实验容器、目录数据库和数据段，不经过 OS 挂载层。
3. 同一核心容器跨 OS 传递：CI 产物顺序交接，不是同一块 exFAT 硬盘插拔。
4. 真实平台挂载、普通应用与 Codex 工作流：尚未执行。
5. 专用 exFAT 设备、USB 异常断开与物理断电：尚未执行。

核心和恢复测试成功不能关闭 Issue #1，也不代表 Issue #3 全部完成。

## M1-B 第一安全增量（PR #4）

实现范围见 [M1-B 说明](IMPLEMENTATION-M1B.zh-CN.md) 和 ADR 0003。原件与 evidence 不交给 SQLite 恢复；另建工作 catalog。默认严格导出，目标必须全新；真正硬链接与显式独立副本区分。业务成功与随后维护失败分离，维护错误仍警告并锁定写保护。

### 已核验运行

| 源提交 | 运行 | 结果与范围 |
|---|---|---|
| `a0302e113b5bf32a3dcd7b1927a15e11f833fd02` | `40077681605` | Linux 功能测试通过；clippy 因 `&mut u64::MAX` 临时常量可变引用失败，不是全绿 |
| `4f690ca52fec958fefa3183d5fcb7d5b615ab851` | `40077908185` | macOS/Linux 全步骤通过；Windows 恢复测试 4 通过、11 失败，锁文件第二句柄读取触发 error 33 |
| `c0759f3cb4be35d21d36a59fdb8296b6e397a3c8` | `40078171236` | 三平台核心 job 全步骤通过：全部特性/默认构建、故障测试、scratch、clippy、fmt；Linux 存储+恢复套件额外并行回归 10 轮 |
| 同上 | `40078171191` | seed→macOS→Windows→Linux 四段成功；返回后 CLI 导出、字节内容/硬链接及原件全文件 SHA-256 不变检查成功 |
| 同上 | `40078171196` | 三平台普通目录探针基线成功，不是挂载测试 |

上述源的全部特性测试构成：存储集成 28 项（每个平台）；恢复主套件 Unix 16 / Windows 15 项；新增非空锁文件证据集成 1 项；owner 单元 Unix 3 / Windows 2 项。Windows 少的一项恢复测试是 Unix 专用符号链接场景，少的一项 owner 测试是 Unix 描述符继承语义；两者不作为 Windows 对应功能的通过证明。两个 ignored crash helper 由父测试在子进程中显式调用，不是缺失的产品验收豁免。

运行与 PR：
- https://github.com/ForceMind/PortaWeave/actions/runs/40077681605
- https://github.com/ForceMind/PortaWeave/actions/runs/40077908185
- https://github.com/ForceMind/PortaWeave/actions/runs/40078171236
- https://github.com/ForceMind/PortaWeave/actions/runs/40078171191
- https://github.com/ForceMind/PortaWeave/actions/runs/40078171196
- https://github.com/ForceMind/PortaWeave/pull/4

### 失败修复，不以重跑掩盖

临时常量可变引用改为明确的工作 catalog 复制预算；不放宽 clippy。Windows 的独占字节锁拒绝通过同进程第二个句柄读锁区；改为在锁持续有效时通过原持锁句柄复制 session.lock，不提前解锁、不省略该文件、不吞错误。新增只读持锁句柄与非空锁文件证据测试。相关官方语义：https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex

### 已覆盖的新增行为

原件/evidence 全量内容哈希保全；活动 owner 拒绝；源重命名后可继续离线导出；空文件、中文目录、逻辑空洞；硬链接与明确副本；重复/嵌套输出路径拒绝；预算/深度/行数限制；块损坏拒绝；恶意名称不得逃出目标；未命名孤儿保全而不 GC；CLI 正常与错误退出；提交前错误/提交后维护警告；带 WAL 的子进程中断在副本恢复到正确版本。

证据保存和导出不等于完整损坏抢救：不重建损坏 catalog，不猜无名孤儿的旧路径，不承诺 ACL/xattr/时间戳完整迁移。读取可能改变访问时间。要求原始目录和输出命名空间可信，不宣称抵御恶意原始目录并发替换。

## M1-A 历史记录（保留）

| 源提交/运行 | 已核验结果 | 范围 |
|---|---|---|
| `b4b36495cda0be3c524a81efcef79490122165ed` / `40074304433` | Linux/macOS/Windows 成功 | 28 项集成测试、scratch、clippy、fmt |
| `aa82d5d4b29f75dba010796a9acc434cc82b93fb` / `40074413951` | seed→macOS→Windows→Linux 成功 | 同一容器产物、字节、路径、硬链接身份、读租约与回收 |
| 同上 / `40074413948` | Mac/Windows 成功，Linux 27 通过、1 失败 | 关闭后立即重开偶发 Busy，不能用前一次绿色掩盖 |
| 修复流程 `40074665682`，产出 `d2eb97752cb950e9cfa638051ad9f04edea5e07f` | owner 回归、核心、clippy、10 轮并行回归通过 | 显式释放所有权锁，见 ADR 0002；最终验收见 PR #2 |

对应历史链接：
- https://github.com/ForceMind/PortaWeave/actions/runs/40074304433
- https://github.com/ForceMind/PortaWeave/actions/runs/40074413951
- https://github.com/ForceMind/PortaWeave/actions/runs/40074413948
- https://github.com/ForceMind/PortaWeave/actions/runs/40074665682
- https://github.com/ForceMind/PortaWeave/pull/2

锁定 Rust 1.99.1；核心日志报告 bundled SQLite 3.53.2。以 Cargo.lock 为准，Python 探针 SQLite 不是生产核心依赖。

既有覆盖：对象身份、真实核心硬链接、打开后删除、替换后旧句柄、跨块读写、截断后读零、逻辑空洞、配额、旧版本读租约、整段/混合回收、错误保护、格式拒绝、重复所有权、240 步模型操作与周期重开、数据同步后/事务提交前后退出子进程、整理中断恢复。

## 普通宿主探针基线（保留）

初版云端 Linux 普通目录全部 12 项探针及 6 项单元测试通过；[原始证据](evidence/2026-10-24-linux-host-probe.json) 中 mount_identity_verified/product_compatibility_certified=false 不得改成 true。首次三平台探针 run `40070876505` 各运行 6 项单元及 8 项宿主用例；不是核心或驱动测试。

## 可重复命令

Python 探针不安装依赖、不联网、不启动 Codex，只创建指定根下的独立临时子目录，禁止指向原始容器。Rust 命令使用已锁定工具链：

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python -m unittest discover -s tests -v
cargo test --workspace --all-features --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo run --locked -p portaweave-core --bin portaweave-recover -- verify SOURCE NEW_WORKSPACE
```

初轮 Rust 编译/测试在 GitHub 的一次性 OS runner 中实际执行，当时 Work 容器未安装 Rust；2026-10-25 续研已在 Work 配置锁定云端工具链，新增本地实际 cargo 证据见后节。没有操作用户电脑。

## 未完成验证

| 项目 | 状态 |
|---|---|
| FSKit / Windows 真实文件系统挂载 | NOT_RUN |
| WinFsp 硬链接接口缺口 | BLOCKED，内部核心身份模型不能代替 |
| 真实 Codex 正常沙盒及权限负向测试 | NOT_RUN |
| 真卷内 Git/依赖/构建/执行/监听/数据库闭环 | NOT_RUN |
| ACL、符号链接、xattr/ADS、mmap、通知等 OS 适配 | 未实现/NOT_RUN |
| 真实 exFAT 设备往返、USB 拔盘与物理断电 | NOT_RUN |
| 宿主空间预约、应急预留、完整短写/EIO/ENOSPC/同步错误矩阵 | 未完成 |
| 完整元数据导出、损坏 catalog 重建或逐文件抢救 | 未完成 |
| 有界低优先级空闲维护、范围索引和全局内存预算 | 未完成 |

进程退出不是物理断电，CI 产物传递不是移动硬盘插拔。段占用归零不代表 catalog/WAL/整个容器占用归零。每份真实平台报告需记录提交、OS/build、后端/工具版本、权限策略、介质/文件系统、命令与原始结果、失败及未运行原因；不上传秘密或真实用户文件。静默损坏、误回收、权限绕过均阻止发布。


## 2026-10-24 本轮接管：M1-E / M0-J / M1-F

| 增量 | 准确已验证源码与证据 | 范围 |
|---|---|---|
| M1-E / PR #13 | `27933a6fa3a0a1b1e3c59211da63c6ef8d78dc27`；core 40106219071、native 40106219134、handover 40106219073、host 40106219079 均 success | 三平台默认/全部特性、故障/clippy/fmt、范围租约实际 C 调用与四阶段容器交接；合并 `b10ccd5` 的 tree `5d15d3a` 与源码/合并候选一致 |
| M0-J / PR #14 | `7204052b55b46c6b943445233154533e10d20264`；bundle 40107067747、core 40107062261、native 40107062181、handover 40107062178、host 40107062244 均 success | Xcode 26.6 / SDK 26.5，实际 App/ExtensionKit/Mach-O/静态库链接、身份/架构/依赖及 ad-hoc/无开发者签名检查；合并 `c669149` 的 tree `f48f7c9` 一致 |
| M1-F / PR #15 | 最终准确 SHA、全流程状态和合并验收记录在 [PR #15](https://github.com/ForceMind/PortaWeave/pull/15) | schema gate、未知结构保全、合法 VACUUM、LF/CRLF 来源及整合后容器交接；文档存在不等于预先通过 |

保留失败：40106003956（M1-E 一处格式）、40106385275（FSPathURLResource 要求 macOS 26，原 target 15.4）、40106650467（构建成功，otool 第一行被错误当成依赖）、40106719661（M1-F 一处格式）、40106903895（schema 来源 LF/CRLF 导致 Windows 拒绝 Mac 容器，return 阶段被跳过）。修复后的准确版本必须通过完整门禁，不能把 skipped 算通过。

macOS 构建产物位于 GitHub Actions 的实验包与日志 artifact。Xcode 在一次性 runner 对构建输出运行 LaunchServices 注册；没有在用户电脑安装、启用或挂载。ARM 链接器 ad-hoc 签章不等于开发者签名、公证或系统授权。CI JSON 明确 actual_mount=NOT_RUN、development_volume_certified=false。

Windows 增加只读环境清单，记录 OS、runtime DLL/版本及注册驱动；原始报告由 native ABI workflow 保存。缺失 driver/runtime 只记录 BLOCKED，不自动安装。不把存在 DLL 或注册驱动当成真正挂载成功。所有 Windows 文件打开仍拒绝，Flush 未支持，完整 ACL/共享/硬链接/缓存语义仍未完成。

本轮 Work Linux 本地运行九项 Python 回归、C header/caller 语法、shell/Python 语法、plist/entitlement/scheme 与工程引用检查；Rust 与 Xcode 实际构建在 GitHub 隔离 runner。真实 FSKit/WinFsp 挂载、卷内 Codex/开发工具、受限沙盒负向权限、物理 exFAT/USB/断电全部继续 NOT_RUN/BLOCKED，Issue #1/#3 保持开放。

## 2026-10-25 续研：M1-G / M0-K / M1-H

M1-G / PR #16：准确源码 `2192816673603af0925218dd466f30e33c02ce97` 三平台全部 17 项通过，core 40111432540、native 40111432533、handover 40111432523、bundle 40111432580、host 40111432511；合并 `8cbcfc25e3d80f640863f1cecbdb9fb86a40b5c2`，tree `f7aebf50d51cd8ecb8a82f20cb20fd614f4c1188` 与候选/源码一致。Unix 实际后端屏障与非 Unix 明确拒绝，均不等于设备弹出。

M0-K / PR #17：准确源码 `557a57e9e464c3f1ddf72889f88e56e6e1a88400` 全部 17 项通过；native 40113309165（SDK job 120929345802 输出 FSVOLUME_CALLBACK_CONTRACT_OK）、core 40113309102、bundle 40113309073、handover 40113309090、host 40113309063 / 40113305249。合并 `372c3f5a57c6078a68a0ba4d0edbb6290099d561`，tree `cf3b0111bd499600be4b386faa0973b7c94524c7` 一致。保留失败 native 40112544717 / bundle 40112544675（Swift Int32/导入 Int 常量）；评审的 cursor 资源耗尽修复后实际跑 256 次重复扫描、80 目录淘汰和 ESTALE 重启。直接 SDK/core 回调不等于 OS 分发/安装挂载。

Work Linux 使用云端只读准备产物中的 Rust 1.99.1 和锁定 registry cache，核验 ZIP 与内部 SHA-256 后解包；首次 uid 保留解包失败，用 `--no-same-owner` 修正，未提升权限或关闭沙盒。本地完整 default/all-feature workspace、clippy/fmt、真实 C caller 和九项 Python 回归可实际运行。没有在用户机器安装工具链。

M1-H 的新增元数据/ABI/恢复及交接范围见 [实现记录](IMPLEMENTATION-M1H.zh-CN.md)。本地四阶段运行不算跨 OS；准确三平台源码/流程状态由对应 PR 验收记录，不预先声明通过。未实测项保持上表状态。

PR #18 已验收：准确 head `986c8c097f11fe05fbf68791537f38df90f00613` / tree `d96c3d018ba8760b61160a83a7ad2afb535700e5` 全部 17 检查 success。core 40114367973、native 40114367951、bundle 40114367966、handover 40114367950、host 40114367905 / 40114365081。v2 UUID `[157,51,47,184,177,121,67,205,184,43,51,100,225,202,81,239]` 随同一 artifact 经 macOS、Windows、Linux 返回保持一致；job 120933006011 输出 v2 recovery/link/original SHA-256 保全成功。合并 `e8700c9d8e63bec331f396be6a2faa950f51e8c8`，候选树与源码树一致，远端 main 已核验。M0-L 后续的 FSKit 属性接入须以自身 SDK/native/bundle CI 验收，不能继承此版本通过结论。

PR #19 已验收：准确 head `0ab866f517b417cb8b908e83ea5d2d0d79bfc012` / tree `4e6c1605c909c9f78acc847c2f5db70241707e4f` 全部 17 检查 success。native 40114673042 的 macOS job 120933385643 实际输出 FSVOLUME_METADATA_CONTRACT_OK 和 FSVOLUME_CALLBACK_CONTRACT_OK，包含重开后 SDK FSVolume identifier 相等、独立 session tag、v1 拒绝、v2 整数属性/纳秒/有效位、链接及拒绝混合设置无副作用。core 40114673015、bundle 40114673050、handover 40114673034、host 40114673014 / 40114669780 全通过。合并 `3098c4cb24a06d8a4695b82dc57ee444297e63a1`，候选树与源码相同。没有启用 load/mount/access；没有系统挂载认证。

M1-I 大小/元数据混合事务新增 4 项场景，元数据套件累计 all-feature 13 / default 10 项；Work Linux 全 workspace/default/all-feature、clippy/fmt、实际 C caller 及四阶段程序验证已运行。源码对应的三平台与交接结果见 M1-I PR 验收；不把本地程序的 macos/windows 阶段参数当作这些 OS。

PR #20 已验收：准确 head `a32096f24ddd9db176f1684859e1d7a5e0c8103f` / tree `801c040038657946e8d4d342e26ba2f467ad0e8f` 17 检查 success，core 40115026207、native 40115026214、bundle 40115026242、handover 40115026280、host 40115026185 / 40115023053。返回 job 120934994651 实际输出 ATOMIC_ATTRIBUTES_HANDOVER_OK incoming=2 outgoing=None，并验证 v2 内容/链接与原件哈希保全。合并 `1e0fc95fb28d099a1a640b3ecca3d1408e88c6a8`，源码/候选树一致，远端 main 已核验。

M0-M 在 macOS 实际 SDK 合同增加 metadata-only/混合 setattr、成功消费位、一次 generation、写模式拒绝、非法纳秒、未知字段无截断、明确 owner/mode 创建及不支持创建字段不留名字的回归。Work Linux 仅做 diff/文本检查；准确 SDK 运行与 App/Extension 编译结果以本增量 PR 为准。有效权限/系统安装挂载仍未通过。

PR #21 验收补记：准确 head `75a1a19d0e977d16028f216f3589ca0f8f403d5e` / tree `bb8ac4acbdd299bdd466885a0d8aaf4379bb2e9e` 17 项 completed/success，实际 SDK job 120935604428 输出 SETATTR / METADATA / CALLBACK 三类合同成功；合并 `3a5cd787519ecfba468addc5c96f9b776343c8cc` 的主线 14 项全部 success。core 40115720966、native 40115720993、bundle 40115721001、handover 40115721017、host 40115720981。当前新一轮开始已通过连接器重新核验该 SHA、14 检查及无未合并 PR。

## 2026-10-25 续研：M1-J 权限策略

Work 临时目录被回收后，重新克隆准确主线，并恢复既有云端只读准备产物的 Rust 1.99.1/锁定依赖；核验 ZIP SHA-256 与 tar 内部 SHA256SUMS，未操作用户机器或改变沙盒。新增核心权限集成 all-feature 9/default 8 项、纯 mode 矩阵 40,960 断言、native ABI 拒绝/输出/健康回归，以及真实 C caller。完整 default/all-feature workspace、clippy/fmt 和四阶段本地程序运行有实际结果；本地阶段名不构成跨 OS 证据。

metadata_handover 新增 trusted-ID 权限允许/拒绝及无副作用校验；准确三平台 source SHA/CI 以 M1-J PR 验收为准。Swift/WinFsp 的拒绝码映射测试不代表回调已启用认证。原生 OS 身份、ACL、数据句柄权限、真实挂载与 Codex 正常沙盒仍未验收，详见 [M1-J](IMPLEMENTATION-M1J.zh-CN.md)。

PR #22 已验收：准确 head `65251b0414aa71c7b0d0e8c656bc8e881504a8c7` / tree `96efea5d77f7bb4812a108fbc7ec7da66b1c63be` 17 检查 success，core 40173465484、native 40173465507、bundle 40173465512、handover 40173465518、host 40173465496 / 40173462347。返回 job 121109468180 实际输出 POSIX_POLICY_HANDOVER_OK 及 v2 恢复/原件哈希保全成功。合并 `7ac870cd2dd98d0d2f42783a32c99addfca700ff`，源码/候选树一致，远端 main 已核验，无未解决评审。

M1-K 增加 all-feature 9/default 7 项 granted-file/metadata 授权回归、1 项原生 ABI 回归，含 4096 总预算（计入管理句柄）、旧 FD/失效/跨 owner/混用 token、读写拒绝输出不变、属性授权和事务故障。Work Linux 完整 workspace、clippy/fmt、真实 C caller 与四阶段程序已实际运行；修复本地 clippy nested-if 和 C 原型/类型声明顺序错误后再验证。准确三平台 source CI 见本增量 PR；受信任 FD 测试不是 OS 认证、执行或真挂载证据。

PR #23 已验收：准确 head `0077e1660727391416b47830348830acf3ea934e` / tree `71c01f76b9e593a184179d2878173957b17c7a53` 17 检查 success，core 40174370270、native 40174370286、bundle 40174370319、handover 40174370274、host 40174370299 / 40174368052。返回 job 121112127994 实际输出 POSIX_DESCRIPTOR_HANDOVER_OK 及 v2 恢复/原件哈希保全成功。合并 `3b89b92783bb1e7c94d2662a84071ee47e437e2f`，源码/候选树一致，远端 main 已核验，无未解决评审。

M1-L 增加 all-feature 6/default 5 项核心视图权限/生命周期回归、1 项 ABI 回归、实际 C 调用及 C++ header 检查。Work Linux 完整 default/all-feature workspace、clippy/fmt、C caller 与四阶段程序实际执行；只写不能通过空租约读取，非法范围/混用 token 无输出，源 FD 关闭/覆盖后旧范围与最后释放均验证。三平台准确 source/CI 看本增量 PR；范围租约不是 metadata snapshot、OS mmap 或原生认证。所有真实系统挂载/Codex/物理介质门禁仍未通过。

PR #24 验收补记：准确 head `d458f8f0123e517356f1dc1c513a79cbef703261` / tree `b57feef2397d92a9818b3add59532440ea2ef962` 17 检查 success；合并 main `8334bc538396743edf16e204e18c66d64a54b8de` 的 14 项全部 success（core 40175426933、native 40175426952、bundle 40175426942、handover 40175426951、host 40175426945）。返回 job 121114324592 输出 POSIX_LEASE_HANDOVER_OK 和原件保全通过。该主线 Windows 清单 artifact 12591079962 的 SHA-256 `813393faa2ecec2253cee1212772b3f2b25e9df450813067f633bd9cad186542` 已下载核验，runtime/driver 均为空、mount NOT_RUN、无安装/服务修改。

## 2026-10-25 续研：M1-M 原子写请求

从重新核验的 `8334bc538396743edf16e204e18c66d64a54b8de` 主线继续。Work Linux 实际 default/all-feature workspace 全套通过；新增 write_requests 为 all-feature 7/default 5 项，含 384 步模型及提交前后故障；native ABI 累计 9 项通过，其中新增 2 项覆盖错误输出、原始/策略 token 拒绝和 6 线程 96 记录追加。真实 C caller 输出 NATIVE_C_ABI_OK，C++17 公共头检查通过，Python 9 项通过；本地四阶段均输出 WRITE_REQUEST_HANDOVER_OK，仍只是 Linux 流程验证。

首次 clippy 因固定长度 chunks_exact 要求改用 as_chunks 失败；修复测试解码写法后 clippy/fmt 和 native 套件重新通过，没有放宽规则。准确三平台、MSVC、SDK/bundle、artifact 往返结果以本增量 PR 的 source SHA 为准；未在这里预报远端结果。真实挂载、原生身份/ACL/共享/缓存/Flush、Codex 正常沙盒与越界负向、物理 exFAT/USB/断电仍未通过。

PR #25 已验收：准确 head `3ad88cec27e544f003445a4b2de91b2d1c506eaa` / tree `0cae02573dd7427129254e58eeda9b7b720187ec` 全部 17 检查 completed/success。core 40188261205、native 40188261276、bundle 40188261226、handover 40188261230、host 40188261207 / 40188259420。返回 job 121153961408 实际输出 WRITE_REQUEST_HANDOVER_OK，v1/v2 恢复导出和原件 SHA-256 保全成功。无冲突/评审阻断，合并 `fe63c2db52308af77cd0ab9602f86f1f82da697b` 的 tree 与源码相同；主线 push 的结果单独核验。

## 2026-10-25 续研：M0-N WinFsp 授权文件合同

Work Linux full default/all-feature workspace、clippy/fmt、真实 C caller（含 Windows 纯投影 ABI）及 C++17 header 实际通过。新增核心 projection refusal 1 项，write_requests 累计 all-feature 8/default 6；native 新增 FILETIME 单元 1 项和 ABI 2 项，ABI 累计 11。最初 check 提示一个未用 import，已移除，不放宽 lint。

Windows MSVC/真实回调签名及 Dispatcher 自检必须由本增量准确源码 CI 验证；新标记 WINFSP_GRANTED_IO_CONTRACT_OK 仅表示直接回调与核心连通。未在 Work Linux 运行 Windows 驱动或 MSVC，也没有把纯投影 ABI 当成 OS I/O。源码对应的 Windows job、SDK/bundle、完整交接及最终合并见 PR；所有真实挂载/ACL/共享/Flush/Codex 沙盒和物理介质门禁保持未通过。

PR #26 已验收：准确 head `513170fef5bbc16de8d0f1047fdb13bf580a465f` / tree `ae0e59696a8ad47f013fd6d3e6e450f2f638e0fb` 全部 17 项 completed/success。core 40188731498、native 40188731577、bundle 40188731499、handover 40188731493、host 40188731487 / 40188729391。Windows MSVC job 121154954006 实际输出 WINFSP_GRANTED_IO_CONTRACT_OK / CALLBACK_CONTRACT_OK / SERVICE_SELFTEST_OK；macOS SDK job 121154953831 的 metadata/setattr/callback 合同通过。返回 job 121155376274 输出 WRITE_REQUEST_HANDOVER_OK 和 v1/v2 恢复导出/原件哈希保全成功。无冲突/评审阻断，合并 `832023dab77429432b7035a17ab66fec7468fd50` tree 一致；合并主线 14 项全部通过（core 40189098888、native 40189098891、bundle 40189098870、handover 40189098932、host 40189098892）。

同 source PR 的 Windows 环境 artifact 12597655777（执行候选 `f4794cc68945706d209f004fd06fe51a489fea0f`，tree 与准确源码/合并一致）已实际下载，ZIP SHA-256 `5446c54ca28a27d7e9439d6a27ab2f99d4124deadec7d29cc3d09de1d7b50cff` 核验通过。Windows 10.0.26100.0 AMD64，runtimes=[]、drivers=[]、actual_mount=NOT_RUN；安装/服务修改/挂载尝试均 false。因此直接回调成功不提升为真实 mount。

## 2026-10-25 续研：M0-O FSKit 事务内属性回执

Work Linux full default/all-feature workspace、clippy/fmt、真实 C caller/C++17 header 及本地四阶段交接实际通过。新增 attribute_replies 核心 all-feature 4/default 2 项，native ABI 新增 2 项（累计 13），覆盖 v1 元数据缺席、v2/mixed/root、投影拒绝和提交前后故障、完整属性/generation/重开、expected object 和管理/策略 token 隔离。本地交接新增 ATTRIBUTE_REPLY_HANDOVER_OK，仍不冒充其他 OS。

FSKit SDK 合同新增 root/no-op/metadata-only/v1 回执精度与有效位检查。Work Linux 不提供 Swift/FSKit 编译证据；准确 SDK 运行、App/Extension build、Windows 回归及真实跨 OS artifact 返回，以本增量 PR 的 source SHA/CI 为准，不预报 PASS。原生身份/ACL/共享/缓存/Flush、真正挂载/Codex 正常沙盒/越界、物理 exFAT/USB/断电门禁仍未通过。

## 2026-10-25 续研：M0-P 原子创建

重新核验主线已由 #27 推进至 #29 合并 `3c215486c3660259b0ee18b2b69ad678f50b4ee4`，14 项检查全通过、无未合并 PR。保留新 Disk Studio 只读镜像检查器、Cargo/workflow 更新与安全边界，继续 .pwvol 原生主线。

Work Linux full default/all-feature workspace（含 partition-inspector）实际通过；新增 create_replies 为 5/3 项，native ABI 新增 2 项累计 15，覆盖共享 4096 准入、失败输出/名字/父时间不变、提交前后故障、创建后 unlink 与 pin 释放。clippy/fmt、实际 C caller/C++17 header、Python 9 项通过；默认套件初次有故障特性专用 import warning，修正后对应套件与 lint 通过。本地四阶段新增 CREATE_REPLY_HANDOVER_OK，不算其他 OS。

SDK 新预算耗尽合同、完整 App/Extension 构建、MSVC 和真实 artifact 交接必须以本增量准确源码 CI 验证；实现存在不预报 PASS。没有安装/启用驱动、创建系统挂载、操作物理盘或关闭沙盒；所有原生权限/共享/缓存/同步、Codex 真卷和物理介质门禁保留 BLOCKED/NOT_RUN。

PR #30 已验收：准确 head `1c123ced61b17a294f01ee91a44aa0fa40e77ae8` / tree `03d322e531c1ca3278b833eb742faed19d6ba81b` 全部 17 项 completed/success，core 40195824302、native 40195824357、bundle 40195824323、handover 40195824250、host 40195824339 / 40195821384。实际 SDK job 121176312211 输出 FSVOLUME_ATOMIC_CREATE_CONTRACT_OK；返回 job 121176775946 输出 CREATE_REPLY_HANDOVER_OK 与 v2 恢复/原件 SHA-256 保全。无未解决评审/冲突，候选与源码 tree 一致，合并 `a3df83046093b505097d0cb02954c2add3fbded7`；主线 14 项全成功（core 40196119984、native 40196120020、bundle 40196119987、handover 40196120027、host 40196119997）。

## 2026-10-25 续研：M0-Q 初始创建状态

Work Linux full default/all-feature workspace（含 Disk Studio）实际通过，新 create_state 核心 4/2 项、native ABI 新增 1 项累计 16；clippy/fmt、真实 C caller/C++17 header 和本地四阶段 INITIAL_STATE_HANDOVER_OK 通过。初次构建遇到 quota 私有访问和示例缺 import，修正后再检查，未放宽 lint 或测试。

实际 SDK 初始大小/时间/消费位/零读、v1 限制、非法字段与目录 size 拒绝，完整 App/Extension、MSVC、同一 artifact 往返及恢复，以本增量 PR 的准确源码 CI 为准。未安装或启用任何驱动，原生身份/ACL/共享/缓存/完整同步、真实挂载/Codex 正常沙盒及物理介质继续未通过。

PR #31 最终源码 `529e1bb4188c52338e934b5183f127496a04ee41` / tree `3069c956240e6cff0e15d3f312f046963e3d2c4e` 全部 17 项通过，合并 `4df50732072fed346f458d66112bfd0f7b16f0a0`。2026-10-26 再次通过 GitHub 连接读取该 main 的 14 项全部 completed/success，无未合并 PR。此次核验没有改变既有挂载/安全限制。

## 2026-10-26 续研：M1-N / M0-R 文件属性授权与 Windows 基本时间

Work 云端 Linux 实际 full default/all-feature workspace（包含 Disk Studio）、clippy `-D warnings`、fmt、真实 C caller、C++17 公共头及布局检查、Python 9 项通过。新增核心 descriptor_metadata all-feature 5/default 4 项、native ABI 4 项（原 ABI 16 项另行保留，总计 20 项）和 FILETIME 单元 1 项。覆盖当前 owner/组/setid、只读数据 grant、换主拒绝、活 unlinked inode、投影拒绝、提交前后故障、输出哨兵、所有时间/属性拒绝、token 与重开。初次测试引用了不存在的 C 函数旧名称，改为现有 pw_node_create_with_metadata 后通过；没有放宽 lint。

真实 C caller 输出 NATIVE_C_ABI_OK。一次附加检查误把 C 测试源按 C++ 的 missing-field-initializers 规则编译而失败；公共头随后用独立 C++17 translation unit、同样的 `-Wall -Wextra -Werror` 与结构布局断言验证通过，C 测试继续作为 C11 编译，没有抑制警告或改写既有测试初始化。Work 本地四阶段 descriptor metadata artifact 例程通过，阶段名不计作其他 OS 证据。

Windows 实际 MSVC 回调/Dispatcher、macOS SDK/App/Extension 回归与真实跨 OS artifact 返回必须核验本增量准确源码 CI。标记 WINFSP_BASIC_INFO_CONTRACT_OK 只证明直接回调合同，DESCRIPTOR_METADATA_HANDOVER_OK 只证明各进程重新授权的核心交接。安全 Open/权限/Flush 继续关闭；没有 runtime/驱动/签名/实际挂载的项目继续 BLOCKED/NOT_RUN，Codex 正常沙盒/越界和物理 exFAT/USB/断电不提升为通过。对应 PR 记录准确 source/tree、Actions、失败和合并核验。

PR #32 已验收：准确 head `ff74f2d03bb17f3f139d04aafcab781c1d156a9b` / tree `78d97780eac6132805d4fb63d3846a647810c207` 全部 17 项 completed/success。core 40220203996、native 40220203883、bundle 40220203831、handover 40220203880、host 40220203892 / 40220182438；Windows MSVC job 121249928170 实际输出 WINFSP_BASIC_INFO_CONTRACT_OK 和既有 I/O/服务标记；SDK job 121249927928 保持所有合同通过，return job 121250455799 输出 DESCRIPTOR_METADATA_HANDOVER_OK 与返回 v1/v2 原件 SHA-256 保全。候选 `4d65811bb64b968d542616a20732aab07958bc2e` 和实际合并 `f2dedd02632bb43d8fd923980e296c5782382a45` 树一致，合并前无冲突/未解决评审；主线 14 项全部 success（core 40220655217、native 40220655235、bundle 40220655216、handover 40220655314、host 40220655296）。

## 2026-10-26 续研：M0-S 名字操作身份校验

Work Linux full default/all-feature workspace（含 Disk Studio）、clippy/fmt、真实 C caller/C++17 公共头及 Python 9 项通过。新增核心 checked_namespace 6/4 项（all-feature/default）、ABI 3 项，总计 native ABI 23 项。核心验证 v1/v2 身份不匹配不改名字/内容/元数据/健康、case/Unicode/alias、目录/环路、前后提交故障及旧打开 inode；C 两个真实线程经 Barrier 受控替换后，使用旧身份删除/rename 返回 STALE，保护替换文件。原有未 checked 接口与名字规则保持，新增语义不冒充原生授权。

C caller 输出 NATIVE_C_ABI_OK，本地四阶段例程输出 CHECKED_NAMESPACE_HANDOVER_OK。真实 macOS SDK 新合同、App/Extension 完整构建、MSVC 回归和同一 artifact 的实际跨 OS 交接需由此增量准确源码 CI 验证，精确记录在对应 PR；不从 Linux 预报平台通过，也不以 SDK/返回容器证明真实挂载。原生 caller/ACL/共享/缓存/同步/安全卸载、Codex 沙盒/越界与物理介质门禁继续 BLOCKED/NOT_RUN。

PR #33 已验收：准确 head `68152a9f0f6982b10132eb25bb59a70c50d516fc` / tree `0f299bb1c466ce20ee47df2ace0ba7f97a23e6ce` 全部 17 项 completed/success；core 40221060853、native 40221060870、bundle 40221060822、handover 40221060815、host 40221060862 / 40221041132。真实 SDK job 121252531379 输出 FSVOLUME_CHECKED_NAMESPACE_CONTRACT_OK，MSVC job 121252531356 保持全部 C/WinFsp 合同通过；return job 121253044406 输出 CHECKED_NAMESPACE_HANDOVER_OK 与 v1/v2 原件 SHA-256 保全。无冲突/未解决评审，候选 `ff7f3a77ef472299d2b5779d59ad7310324f964f` 和实际合并 `c7292690beedea190c2d5ad842382697d3cd247b` 树一致，远端 main 已核验。合并主线的 push CI 单独记录在 PR；不把 SDK 成功提升为系统挂载。

## 2026-10-26 补正：原始 Rust 打开入口的共享句柄上限

实际复现：强化已有 create_replies 的 4096 额度测试后，未修复代码仍允许 `Volume::open_file` 额外打开，断言返回 ResourceLimit 失败（记录 pin-admission-before.log；属于预期的修复前失败）。修复将管理 open 路由到既有 core reserve/publish；另强化混合 policy/admin 满额用例，以及释放一个后重新准入、不复用 ID 和目录不占普通文件 pin。

修复后 Work Linux full default/all-feature workspace（各 30 个 test/doc-test suite，含 Disk Studio）、clippy `-D warnings` 和 fmt 实际通过，两个既有预算回归均保留；没有把强化断言计算为新增独立测试。无格式/schema/ABI/依赖/工作流变更，也未改变管理接口权限或 native mount gates。精确 source/tree、三平台/native/SDK/bundle/artifact 与最终 main 核验见对应 PR。

## 2026-10-26 续研：M0-T 目录快照/ReadDirectory

通过 GitHub 连接重新读取 main `3198ac57df8a5b54762adc05868b68c0a0a411ee` / tree `d3ee3c6687b82a5ffa74ff24d1d0882dc8900ded`，14 项 completed/success、无未合并 PR，#1/#3/#28 保持开放。Work 工作区由仓库恢复，离线工具链 ZIP/两个内层 tar 的既有 SHA-256 再核验后使用；没有操作用户本机。

Work Linux full default/all-feature workspace 各 32 suites、clippy `-D warnings`、fmt、实际 C11 caller/C++17 公共头和 Python 9 项通过；新增核心 directory_snapshots 4/3 项、ABI windows_directory 8 项（累计 31）、native 累计条目预算单元 1 项。真实 C caller 输出 NATIVE_C_ABI_OK；本地四阶段目录元数据例程输出 DIRECTORY_SNAPSHOT_HANDOVER_OK，阶段名不计作其他 OS 已运行。

覆盖全部祖先及 R+X/v1/预算拒绝、原名/父目录/不可读子项属性、无 inode/data pin、保护下只读观察、UTF-16 surrogate/255-unit/对齐/零 padding/完整分页及 EOF、投影拒绝无 cursor、错误输出哨兵、跨 owner/管理 token 混用、64 个真实混合 cursor 和关闭后的准入。4 个真实读线程与 close 竞争，只允许完整快照或不改输出的 invalid。累计 100000-entry 逻辑预算以 unit 中降低至 3 的真实卷/两类 cursor 验证，不冒称 100000 个真实名字的性能压力、精确堆界限或完整解析器预算。首次 clippy 的 is_multiple_of/as_chunks 两个 decoder lint 已修正，未降低 lint。

固定 WinFsp 2.1 头、fsop.c 和 dirctl.c 已重读核对 104-byte/8-byte/EOF/ordinal marker。新 MSVC SDK 类型解析/ReadDirectory 回调、共享 volume 参数和 self-test，macOS SDK 64-cursor 压力下自有 LRU/外部 snapshot 保留及完整 App/Extension、真实跨 OS artifact 回传，必须按本增量 PR 的准确 source/tree/日志验收，当前文档不预报结果。Marker 重放是同一快照，目录 Open/当前 GFI/刷新及 ACL/共享/Flush 仍未完成；真正挂载/Codex 正常沙盒/越界与物理 exFAT/USB/断电保持 BLOCKED/NOT_RUN。

PR #35 已验收：source `2ad7a8c007512465d7b3ef99107d841cd91e76ba` / tree `2e6ef7e23d2b5b531ef6d43abd918cdfea2955ec`，17 项 completed/success；实际 MSVC job121494703714 输出 WINFSP_DIRECTORY_CONTRACT_OK，SDK job121494703375 输出 FSVOLUME_CURSOR_PRESSURE_CONTRACT_OK，App/Extension 完整构建及真实 macOS/Windows/return 目录元数据交接通过。候选 `14f421facf80cd65091bde594f1fb9e92c85c4d2` 与合并 `2dca20f5630c73e54fdbab9d2bac7adf4e543187` 树一致，主线 14 项全部 success（core40300438415、native40300438267、bundle40300438225、handover40300438447、host40300438335）。最终 return job121497630332 保全 v1/v2 恢复原件 SHA-256。Windows 清单 artifact12639927908 实际下载/核验 SHA-256 `4fac3af915b8b0dd79209f684a9910229e2665daa102eab4cb9a3eb6e2fe54c0`，source_commit 为该 main，runtime/driver 为空，实际挂载 NOT_RUN，安装/服务更改/挂载尝试 false。2026-10-27 再通过 GitHub 连接核验此 main 的 14 项全成功、无开放 PR。

## 2026-10-27 续研：M0-U 当前目录属性与显式重建

从上述已核验 main 继续。当前目录属性每次按当前可信 R+X/全部祖先检查，只用 snapshot 捕获的 ID；显式替换先准备完整新列表/属性和新 token，抵扣自己的旧 slot/条目后才发布。删除返回 NotFound，不提供目录 inode pin；旧 token 在成功替换后失效，旧 cookie 必须丢弃。ReadDirectory 的 NULL Marker 保持原快照重放，原生门禁继续关闭。

Work Linux full all-feature/default workspace（各 32 suites，包含 Disk Studio）、clippy `-D warnings`、fmt、真实 C11 caller、独立 C++17 公共头检查及 Python 9 项实际通过。新增 core 2 项（directory_snapshots 现 all-feature6/default5）及 ABI4项（windows_directory12/native integration累计35），强化原有保护下只读观察和3-entry共享预算 unit，不虚增单元测试数量。覆盖当前/旧属性分离、祖先撤权、目录移动及原名替换、删后无旧 inode/重开正常、转换与权限拒绝输出、满64混合cursor替换、抵扣后超限保留旧快照、4真实读线程与替换竞争及typed token隔离。

首次新增 ABI 两处 NULL credentials 的测试预期误写 DENIED，既有解析为 INVALID；按既有 C ABI 修正断言/头注释后通过，没有修改凭据解析或放宽授权。WinFsp 回调层自己在 NULL caller 时 ACCESS_DENIED。真实 C caller 输出 NATIVE_C_ABI_OK；本地四阶段输出 DIRECTORY_CURRENT_INFO_HANDOVER_OK，均运行在 Linux，不冒充另两个 OS。

准确源码的 MSVC 回调新合同、Swift/SDK 满额重建、完整 App/Extension 与真实跨 OS artifact 返回，须读取对应 PR 的 CI 日志验收；此文件不预报平台结果。完整活目录需另做格式/旧 reader/恢复决策，因为当前 v1/v2 拒绝孤儿目录，不能暗中放宽 schema/verify、伪造旧 fstat 或迁移唯一容器。真正挂载/Codex正常沙盒/越界/物理exFAT/USB/断电仍 BLOCKED/NOT_RUN，没有本机/驱动/签名/安全要求改动。


## 2026-10-27 续研：M1-O 显式 v3 目录 inode 生命周期

GitHub 连接重新核验 main `a578785591976e9d71658dc5b93bf1c234c9db7f` / tree `ada462873a1bee474dbde118de7a03e164f839fc`，14 项 completed/success，无未合并 PR，#1/#3/#28 开放。从该版本继续，旧 v1/v2 schema 未改、无迁移。

Work Linux full default/all-feature workspace、clippy `-D warnings`、fmt 实际通过。新增 directory_lifetimes core all-feature10/default8 项；ignored 子进程 helper 由父测试在新夹具中分别执行两次真正 exit(86)，验证 rmdir commit 前后恢复与重开回收。新 native8 项，integration累计43/unit3；强化既有 core/ABI 4096 混合预算，不计作额外独立用例。覆盖目录撤权/移动/最后 unlink/替换/多个 pin、当前 owner、纯投影回滚/失败输出、非空/有块孤儿拒绝、旧格式不放宽、foreign/stale/typed token、64 个真实混合 cursor、4 个真实 info 线程与最后 close 竞争。真实 C11 caller 输出 NATIVE_V3_DIRECTORY_ABI_OK/NATIVE_C_ABI_OK，独立 C++17 公共头编译/链接/调用通过。

首轮 C caller 夹具 uid=100 与既有 caller uid=501 不匹配，正确 DENIED；修正创建属性匹配 caller 后通过，未放宽权限检查。固定 a578 旧 native cdylib 在独立 target 从实际旧源码构建，Python ctypes 两次 pw_volume_open 新 v3 返回 UNSUPPORTED、输出99及原件所有文件 SHA-256 不变，输出 ACTUAL_V2_READER_REJECTS_V3_OK。这是 Linux 实测；其他 OS 仍须准确 CI。

本地 directory_handover 四阶段全部运行在 Linux，验证同一 UUID/硬链接/阶段内容、目录 grant 和放弃 owner 后一个空孤儿。每站在普通打开前 Recovery::prepare/strict export/全原件 SHA-256，之后 ordinary open 才回收，最终无孤儿；这是 abandoned-owner 夹具，不能称为硬件断电、安全卸载或跨 OS 实测。

新增 Windows MSVC v3 目录回调合同、macOS SDK 外部可信 pin 后真实 getAttributes/最后 close ENOENT、完整 App/Extension、固定旧 reader 三平台拒绝，以及独立 v3 artifact 的 seed→macOS→Windows→Linux，必须按本增量准确 source CI 验收并记录在对应 PR。WinFsp Open/security/Flush 和 FSKit access/probe/load/activate/mount 未打开。实际已安装驱动的挂载、签名/系统授权、SID/ACL/共享/缓存/完整同步、真实卷开发工具/Codex 正常沙盒及越界拒绝、物理 exFAT/USB/断电仍 BLOCKED/NOT_RUN。


## 2026-10-27 续研：M0-V FSKit 目录对象引用

GitHub连接重新核验 main `0103619da1cee08ffc3eb3970296c4b06fb463aa` / tree `efafd2d7bdde73580ceb6b715c491ad7ccd2c5f3`，14项completed/success，无未合并PR，#1/#3/#28开放。从该版本继续，不改schema/format/capabilities或原生安全门。

Work Linux full default/all-feature workspace、clippy -D warnings、fmt、Python9、实际C11 caller与独立C++17公共头/布局/类型/链接调用通过。新增 core directory_create_pins all-feature4/default2，ignored helper由父测试在新夹具中实际执行两次exit86；新 native directory_item_pins 4项（integration累计47/unit3），含4真实线程同名创建竞争。旧 core/native 4096预算加入typed admin目录和显式atomic目录创建拒绝/恢复，未虚增测试数。验证旧格式拒绝、默认目录不占pin、预投影拒绝无名字/父元数据/pin、前后commit故障/维护保护、引用独立与foreign/stale/typed隔离。

实际 C caller 输出 NATIVE_DIRECTORY_ITEM_REFERENCE_ABI_OK/NATIVE_V3_DIRECTORY_ABI_OK/NATIVE_C_ABI_OK。本地 v3 四阶段每站输出 DIRECTORY_CREATE_PIN_HANDOVER_OK 并保留恢复/原件SHA256/UUID/硬链接回归；四阶段全部在Linux。固定实际 a578旧二进制仍两次UNSUPPORTED、输出99和源全文件SHA256不变。没有用本地阶段名冒充另外两个OS。

新实际SDK目录item生命周期/删除替换/当前属性/close与reclaim分离、4096额度下create与lookup拒绝/字段消费/父时间不变/释放恢复，必须由本增量准确CI执行；保留M1-O外部grant与SDK独立pin合同。枚举空目录的pack closure只是算法证据，不是packer系统分发。完整unsigned App/Extension、MSVC/Dispatcher、三平台core/native/旧reader与同一v1/v2/v3 artifact往返须读日志，不能从类型检查推断挂载。具体source/tree/Actions与最终main以对应PR为准。

真实Windows挂载缺runtime/driver、macOS签名/系统授权、原生caller/ACL/共享/缓存/完整同步、符号链接/xattr/监听/mmap、真实卷开发工具/Codex正常沙盒及越界限制、物理exFAT/USB/断电仍BLOCKED/NOT_RUN，#1/#3不关闭。未操作用户本机、安装驱动、修改服务或放宽沙盒。


## 2026-10-28 续研：M0-W FSKit 捕获式目录枚举

GitHub连接重读main `433b0ce832c12894c82b66e6146ffd02455a25be` / tree `015cba5f52ed9ee9379d30beb7af74d5d54831c6`，主线14项全部completed/success、5工作流success，无开放PR；此前M0-V记录的主线单条check状态滞后现已更新成功。#1/#3/#28仍开放。清理后的Work目录从仓库恢复，固定Rust1.99.1与registry的既有ZIP/两个tar SHA256重新核验，无本机操作。

Work Linux full default/all-feature workspace、clippy -D warnings、fmt、Python9、实际C11调用与独立C++17头/类型/链接检查通过。新core object_directory_snapshots all-feature3/default2，验证各格式一起捕获/无pin、当前父/空孤儿、预算拒绝与保护下只读；新native fskit_directory6（integration累计53/unit3），覆盖旧元数据在覆盖/unlink/最后GC后仍可读、显式新观察、父/root/dots、错误/EOF/小缓冲输出、typed/foreign/stale与非法cookie、64真实混合cursor抵扣、4真实读线程与替换竞争。既有降低到3-entry的预算unit加入第三表/2dots/增长拒绝/旧副本，不虚增unit数、不宣称100000实际名字/峰值内存压力。

实际C caller在v1/v2/v3分别输出NATIVE_CAPTURED_DIRECTORY_ABI_OK，保持所有旧ABI标记；本地v3四阶段每站增加OBJECT_DIRECTORY_SNAPSHOT_HANDOVER_OK，内容/恢复/原件SHA256/UUID/硬链接与目录pin回归保留。这些阶段都在Linux。另行构建固定 a578785591976e9d71658dc5b93bf1c234c9db7f 的真实旧reader，对本轮v3完成容器两次拒绝UNSUPPORTED，失败输出及原件所有文件SHA256保持不变；未从旧版本推断新源码通过。

SDK新增对已捕获child先覆盖/删名/close+reclaim后继续分页的合同，确保返回旧名字/属性且不阻止GC；新initial枚举换verifier、非零verifier稳定重放、失败重启保留旧数据、dots/root/parent/模式与invalidDirectoryCookie、pack拒绝重试、256新枚举/80目录LRU和满64混合cursor自身重建都需准确macOS CI。真实SDK对象/packing closure不等于FSDirectoryEntryPacker内核分发。完整App/Extension、MSVC/Dispatcher、三平台core/native/旧reader、实际v1/v2/v3 artifact返回须以本轮准确PR source/tree/日志验收，不预报平台成功。

所有native caller/security/Open/Flush与FSKit access/probe/load/activate/mount门禁不变。runtime/driver、签名/授权隔离环境真挂载、OS身份/ACL/共享/缓存通知/完整同步、符号链接/xattr/mmap/执行、真卷开发工具与Codex正常沙盒/越界、物理exFAT/USB/断电仍BLOCKED/NOT_RUN，#1/#3保持开放。


## 2026-10-29 M1-P 协作范围锁续研

GitHub重读main `0e1338591a04b970985d3aa866528331cb3f1df0` / tree `04ccaab734b616711b0636de4e113930b2bc23d4`，14/14检查及5工作流成功、无开放PR，#1/#3/#28开放；Work干净。新增inode/handle有界共享独占范围grant、typed policy C入口与关闭释放，详见M1-P/ADR0018/AGENTS51。初轮native测试fixture误用open access1/3而被既有ABI拒绝，修正为4/6后3项通过，未改生产校验；初轮日志保留。最终全量/真实C和跨OS CI按准确源码结果补记PR。所有真挂载、系统锁/共享、身份/ACL/缓存/同步/Codex/物理介质门禁不变，不操作用户本机或驱动。

本轮Work最终完整回归：all-feature244通过/0失败、default204通过/0失败；原有4/1 ignored helper按既有父测试触发规则保留。clippy -D warnings、fmt、Python9、真实C11及独立C++17头/签名/链接调用通过。C输出NATIVE_ADVISORY_RANGE_LOCK_ABI_OK；本地v2四阶段逐站ADVISORY_RANGE_LOCK_HANDOVER_OK且UUID一致，均是Linux本地阶段，不冒充其他OS。完整source/main平台CI与确切SHA/环境清单以对应PR最终验收记录为准。
