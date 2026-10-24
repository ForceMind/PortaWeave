# 验证方法与证据状态

日期：2026-10-24。**没有可用的 PortaWeave 文件系统实现。** 这些测试不能被宣传为真实容器、驱动或 Codex 兼容性已通过。

## 本次实际执行

环境：云端 Linux x86_64 容器，Python 3.13.5，Python 内置 SQLite 3.46.1；使用 `/mnt/data` 中的普通宿主测试目录。没有操作用户电脑、安装驱动或格式化磁盘。

| 项目 | 实际结果 | 能证明什么 |
|---|---|---|
| `python -m unittest discover -s tests -v` | 6/6 通过 | 探针核心流程、清理边界、复制冒充硬链接检测、缺工具不算通过、拒绝原始容器路径、报告不覆盖 |
| `workspace_probe.py` 默认全部用例 | 12/12 PASS | 当前 Linux 宿主上的 API 冒烟基线 |
| Rust 引擎/自动 GC | NOT_RUN | 尚未实现 |
| macOS FSKit 真实挂载 | NOT_RUN | 当前执行没有 macOS 测试环境 |
| Windows 真实挂载 | NOT_RUN | 当前执行没有 Windows 测试环境 |
| WinFsp 硬链接门禁 | BLOCKED | 官方文档限制待复现、选型与实现解决，不是本轮实测结果 |
| 真实 Codex/正常沙盒/权限负向验证 | NOT_RUN | 未启动代理任务，未改变权限或认证配置 |
| npm/pnpm/pip/uv/Cargo 完整工作流 | NOT_RUN | 只运行离线 Git、Node 和 venv 夹具，不等同完整依赖/构建验收 |
| 真 exFAT/设备跨系统交接/异常断电 | NOT_RUN | 没有授权测试介质，不以宿主目录结果代替 |

机器可读证据：[Linux host probe](evidence/2026-10-24-linux-host-probe.json)。其中 `mount_identity_verified=false`、`product_compatibility_certified=false` 是有意保留的事实，不是需要为了显示绿色而删除的字段。

探针所带 SQLite 3.46.1 只操作它自己创建的临时夹具。它不是产品依赖，不能代替 SPEC 要求的经过补丁核验的生产 SQLite 构建。

## 探针使用与边界

需要 Python 3.10+，Git/Node 测试需要对应已安装工具；探针不安装依赖，不访问网络，也不启动 Codex。它只在指定根目录下新建独立临时子目录，并清理这个子目录。禁止指向原始 `.pwvol` 容器或其内部；将来应指向挂载后的卷内专用测试目录。

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/new-report.json
python tools/workspace_probe.py --root /path/to/scratch --cases io,mmap,replace,hardlink,sqlite
```

输出文件必须不存在，避免覆盖旧报告或用户文件。退出码 0 表示所选择的适用冒烟用例通过，1 表示失败，2 表示缺前提/未执行或运行配置/报告错误。未选用例在报告中列出；NOT_APPLICABLE 只用于平台专属场景，不能拿来豁免缺失功能。

用例覆盖随机写/截断与读零、mmap、基础替换、真实硬链接、相对符号链接、Unicode 和大小写改名、跨进程独占创建、SQLite WAL 写者锁、Git worktree/gc/fsck、Node 事件和模块执行、复制式 venv 解释器及 POSIX 执行位。

没有覆盖故障原子性、Windows 原生共享标志/ACL、完整原生加载器、GC、物理掉电或实际 Codex。这些留在 COMPATIBILITY 的完整门禁中。`--label` 是自填标签，不能证明挂载类型；必须另附 OS 原生挂载信息、卷 ID、实际 backing 和驱动版本。

## CI 的证据等级

`.github/workflows/probe-baseline.yml` 配置 Linux/macOS/Windows 的**普通宿主目录**回归，验证探针可移植性；不是 FSKit/WinFsp 驱动 CI，不会安装或挂载我们的文件系统。是否实际运行及结果以 GitHub Actions 为准，本文件不预宣称成功。

## 后续每份真机报告必须包含

提交 SHA、格式版本、OS/build、驱动/后端版本、工具/代理版本、权限策略、挂载信息、设备/宿主文件系统、用例命令与原始结果、重复次数、失败复现、未运行原因。删除认证信息和秘密，不上传用户真实项目内容。

先在同机原生目录做对照，再在 passthrough 原型，最后在真实容器做同组操作。前两层通过不能替代第三层。Codex 正向访问与越界拒绝都必须测；Windows 原生与 WSL 分开；每个后端升级应重跑相关门禁。任何静默损坏、回收误删或权限绕过阻断发布。
