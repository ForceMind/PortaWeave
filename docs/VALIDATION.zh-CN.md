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

本轮 Rust 编译/测试在 GitHub 的一次性 OS runner 中实际执行；聊天容器未安装 Rust，不将源码检查说成本地 cargo 测试。没有操作用户电脑。

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
