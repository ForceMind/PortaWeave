# M0-AA · Windows 硬链接验收与后端证据

## 驱动边界

| 路径 | 已核对的证据 | 处置 |
|---|---|---|
| WinFsp 2.1 `ddca7bd5` | 内核 `FileLinkInformation` 返回 `STATUS_NOT_SUPPORTED`。卷能力位未接通，SDK 要求卷参数及文件信息的 `HardLinks` 为零。 | 固定版本 FS-07 保持 BLOCKED。用户态 Dispatcher 无法从这条分发收到创建请求。 |
| WinFsp 上游 `4425d480` | 同一分发分支仍返回 `STATUS_NOT_SUPPORTED`。 | 继续通用回调研发。若沿用 WinFsp，需上游可维护的内核、协议和用户态扩展。 |
| Dokany `49ae5ed3` | 用户态 `DispatchSetInformation` 没有链接创建分支，默认保留 `STATUS_INVALID_PARAMETER`。驱动的无事件线程回退分支可报告 `FILE_SUPPORTS_HARD_LINKS`。 | 当前源码没有可接的链接创建回调，能力位不足以关闭 FS-07。 |
| CBFS Connect 2026 原生 Windows 组件 | 厂商文档提供 `UseHardLinks`、`CreateHardLink`、`EnumerateHardLinks` 及句柄 token 接口。 | 保留为备选。隔离环境驱动、SDK 许可及实际请求/身份/语义均待验证。 |

源码与厂商资料

- [WinFsp 2.1 内核分发](https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/src/sys/fileinfo.c#L2023) · [卷能力位](https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/src/sys/volinfo.c#L95) · [SDK 字段](https://github.com/winfsp/winfsp/blob/ddca7bd5481857a65ba552f643b8776fd070836f/inc/winfsp/fsctl.h#L205)
- [WinFsp 上游内核分发](https://github.com/winfsp/winfsp/blob/4425d4807aea4626affb151e38c1cc774ccd5e75/src/sys/fileinfo.c#L2024) · [官方兼容性](https://winfsp.dev/doc/NTFS-Compatibility/)
- [Dokany 用户态分发](https://github.com/dokan-dev/dokany/blob/49ae5ed3628806878408995916d772b60241e083/dokan/setfile.c#L197) · [驱动回退能力位](https://github.com/dokan-dev/dokany/blob/49ae5ed3628806878408995916d772b60241e083/sys/volume.c#L134)
- [CBFS Connect 2026 原生组件参考](https://cdn.callback.com/help/CCM/cs/CBFS.htm) · [厂商许可与驱动说明](https://www.callback.com/cbfsconnect)

## 可重复的 FS-07 探针

在 Windows 上指向可丢弃的测试目录，先运行一次宿主基线。在获授权的候选后端真正挂载后，用同样的命令指向该挂载内的测试目录，输出报告存到卷外的新路径。

```powershell
python tools/workspace_probe.py --root $env:TEMP --label windows-host --cases hardlink --report host-hardlink.json
python tools/workspace_probe.py --root 'R:\scratch' --label candidate-mount --cases hardlink --report mounted-hardlink.json
```

探针在私有夹具中用 `CreateHardLinkW` 创建跨目录别名，查询 `FILE_SUPPORTS_HARD_LINKS`、卷序列号与 128 位文件 ID、`FileStandardInfo.NumberOfLinks`。要求链接数依次为 1、2、1，并验证保留源读句柄时禁止通过别名写打开，释放后可重试，且从别名写入能从源句柄读到。删除源名后需拒绝通过原名打开，别名重开后保持同一身份与内容。挂载身份由外部验收记录确定，报告自身保持 `mount_identity_verified=false`。宿主 CI 仍用现有 `hardlink` 条目，非 Windows 沿用普通 API 版本。

Windows 合同见 [CreateHardLinkW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createhardlinkw)、[FILE_STANDARD_INFO](https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-file_standard_info) 和 [GetVolumeInformationByHandleW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getvolumeinformationbyhandlew)。

下一轮后端验证需要记录准确 SDK/驱动/OS、真实挂载身份和安装授权。先核对原生链接请求如何交付源身份及目标父目录，授权校验与命名事务在同一 owner 内完成，再验证跨别名内核文件对象、缓存、锁和 ACL，以及最后 unlink 时已打开句柄的存活。CBFS 备选先验证其真实回调与这些生命周期，再评估许可和发版。当前尚无候选挂载、跨进程锁、真实调用者权限、卷内 Codex 或实物 exFAT 证据。
