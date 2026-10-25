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
