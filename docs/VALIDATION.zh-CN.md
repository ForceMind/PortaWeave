# 验证方法与证据状态

日期：2026-10-24。已实现实验性持久化核心，但**仍没有可挂载的 PortaWeave 文件系统**。以下分版本记录证据，后续最终提交状态与精确 run ID 在 PR #2 的验收评论中补记；旧版本通过不自动覆盖新版本。

## 证据分层

1. 普通宿主目录探针：只测试宿主系统 API 和探针自身。
2. 共享核心：读写真实的实验容器、目录数据库和数据段，不经过 OS 挂载层。
3. 核心容器产物跨 OS 传递：验证同一个样本可顺序修改/重新打开，介质是 CI 产物传输而非同一块 exFAT 硬盘。
4. 真实平台挂载、应用与 Codex 工作流：尚未执行。
5. 专用 exFAT 设备和物理断电：尚未执行。

只有第 4/5 层对应测试通过，才允许作相应产品声明。核心成功不能关闭 Issue #1。

## 本轮核心记录

| 源提交/运行 | 已核验结果 | 范围 |
|---|---|---|
| `b4b36495cda0be3c524a81efcef79490122165ed` / run `40074304433` | Linux/macOS/Windows 三个 job 全部成功：集成测试、scratch 示例、clippy、fmt | 28 项集成测试；ignored helper 由父测试单独启动，并不是豁免某项功能 |
| `aa82d5d4b29f75dba010796a9acc434cc82b93fb` / run `40074413951` | seed→macOS→Windows→Linux 返回校验四个 job 成功 | 同一容器产物、文件字节、大小写路径、硬链接身份、读取租约与回收 |
| 同上 / run `40074413948` | macOS/Windows 成功；Linux 27 通过、1 失败 | 发现关闭后重开偶发 Busy；不能只引用上一个绿色版本掩盖该失败 |
| 修复流程 run `40074665682`，产出 `d2eb97752cb950e9cfa638051ad9f04edea5e07f` | 新锁回归、核心测试、clippy，以及 10 轮并行集成回归全部成功 | 显式锁释放，见 ADR 0002；后续仍须对最终提交复跑三平台 |

对应记录：
- https://github.com/ForceMind/PortaWeave/actions/runs/40074304433
- https://github.com/ForceMind/PortaWeave/actions/runs/40074413951
- https://github.com/ForceMind/PortaWeave/actions/runs/40074413948
- https://github.com/ForceMind/PortaWeave/actions/runs/40074665682
- https://github.com/ForceMind/PortaWeave/pull/2

锁定工具链 Rust 1.99.1；首次核心日志实际报告 bundled SQLite 3.53.2。依赖以 Cargo.lock 为准，不把 Python 探针内置的 SQLite 当成生产核心依赖。

### 关键已覆盖行为

目录/对象身份、真实核心硬链接、打开后删除、替换后旧句柄、跨块写入、截断后读零、逻辑空洞、配额、读取旧版本租约、整段和混合段回收、校验失败保护、格式拒绝、重复所有权、240 步模型操作与周期重开、数据同步后/事务提交前后直接退出子进程、整理提交前失败恢复。

锁修复增加 1 个全平台 owner 排他单元测试和 1 个 Unix 描述符副本单元测试；三平台集成集合仍是 28 项。CI 的 Linux 额外循环 10 次整个集成集合以保留并行回归压力。

进程退出测试不等于拔盘或断电；SQLite 与 OS 已确认写入在真实设备上是否持久化仍待验证。数据段长度为零不代表目录数据库和整个容器物理占用为零。

## 原有宿主探针基线（保留）

初版在云端 Linux 普通目录执行全部 12 项探针及 6 项探针单元测试，通过。原始机器可读证据保留于 [Linux host probe](evidence/2026-10-24-linux-host-probe.json)。其中 `mount_identity_verified=false`、`product_compatibility_certified=false` 不得删除或改为 true。

初次三平台宿主探针 CI：run `40070876505`，各运行 6 项单元测试和 8 项选定宿主用例。它不是核心测试，更不是驱动挂载测试。

探针需要 Python 3.10+，Git/Node 用例需对应工具；不安装依赖、不联网、不启动 Codex。只在指定路径中新建独立临时子目录，禁止直接指向原始 `.pwvol` 或其内部。

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python -m unittest discover -s tests -v
cargo test --workspace --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
```

## 未完成的验证

| 项目 | 状态 |
|---|---|
| FSKit / Windows 真实文件系统挂载 | NOT_RUN |
| WinFsp 挂载接口硬链接缺口 | BLOCKED，核心身份模型不能替代接口解决 |
| 真实 Codex 正常沙盒、权限负向测试 | NOT_RUN |
| 真实卷内 Git/包管理器/构建/执行/IDE/数据库工作流 | NOT_RUN |
| ACL、符号链接、xattr/ADS、mmap、通知等 OS 适配 | 未实现/NOT_RUN |
| 真实 exFAT 设备跨系统交接、拔盘和断电 | NOT_RUN |
| 只读恢复工具、完整导出、全面空间不足及短写故障矩阵 | 未完成 |

所有后续报告必须保留提交、OS/build、驱动版本、工具/代理版本、权限策略、挂载信息、设备/宿主文件系统、命令与原始结果、失败复现及未运行原因；不上传认证信息或用户真实文件。任何静默损坏、回收误删或权限绕过阻断发布。
