# PortaWeave（随卷）

[English](README.md)

> 一个容器，两端工作；按需增长，自动回收。

把 exFAT 移动硬盘上的目录容器挂载为 macOS/Windows 的**正常读写开发工作盘**。不仅保存文档，还要让 Codex 本地会话、Git、IDE、依赖安装、编译、测试和程序运行直接在卷内工作。不以只读预览、专用导入导出接口或暗中复制到系统盘替代。

## 当前状态（2026-10-24）

**已有完整规格与初版兼容性探针；尚无可用文件系统。** Rust 存储引擎、双端挂载、自动回收和桌面客户端都尚未实现。普通宿主目录上的探针结果不是 PortaWeave 卷的兼容性结果。

目标架构为 Rust + SQLite 权威元数据 + 追加式分段数据。FSKit、WinFsp 是待验收的候选；WinFsp 官方列出的硬链接缺失已登记为选型阻断项。不能为了保持选型而降低工作盘要求，也不能复制一个文件后假称创建了硬链接。

默认自动回收，尊重回收站、打开句柄、映射和有效引用。一个容器只有一个 owner，两台电脑安全卸载后顺序交接。两端需要客户端和相应系统组件；不承诺免安装或随意拔盘安全。

## 文档

- [完整产品与存储规格](docs/SPEC.zh-CN.md)
- [开发工作盘兼容性契约](docs/COMPATIBILITY.zh-CN.md)
- [研发路线与发布门禁](docs/ROADMAP.zh-CN.md)
- [验证方法与证据状态](docs/VALIDATION.zh-CN.md)
- [候选后端决策](docs/adr/0001-development-volume-backend-gate.md)
- [官方资料](docs/SOURCES.md)、[工程规则](AGENTS.md)、[工作接管指令](WORK_HANDOFF.zh-CN.md)

## 当前能运行的内容

仅有测试工具，不是客户端。Python 3.10+；Git/Node 用例需要对应已安装工具。对专用测试目录执行：

```sh
python tools/workspace_probe.py --root /path/to/scratch --label host-baseline --report /path/to/report.json
python -m unittest discover -s tests -v
```

只在指定目录下创建独立临时子目录；不格式化、不安装驱动、不访问网络、不启动付费 Codex 任务、不修改权限配置。缺失工具和未执行用例不能当成通过。真机挂载、真实 Codex 沙盒与故障测试仍须完成。

项目顶层许可证尚未由所有者确定，第三方许可独立保留。项目名称不代表已完成商标核验。
