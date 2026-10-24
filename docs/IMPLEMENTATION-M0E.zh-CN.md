# M0-E：Native C ABI 统一桥接层（实验）

日期：2026-10-24，基础提交 main f3ca60a46ea38d85361d0d2c6199da77022aa651。

## 为什么做
macOS FSKit 是 Swift 原生扩展，需要通过 C ABI 与共享 Rust Core 通信；Windows WinFsp 的原生适配器同样需要稳定的会话、文件句柄和元数据接口。之前只有 Rust Volume API，还没有能让原生宿主调用的、跨语言固定数据布局的入口。

本增量引入 crates/native-bridge 和 include/portaweave_native.h，作为 **M0 原生接入前置接口**，不改变容器格式。提供 ABI 版本、创建与打开容器、关闭独占所有者、对象状态、目录/文件创建、lookup、打开文件、按范围读写和截断、原子 rename、删除、硬链接与枚举目录，以及写保护/维护状态。

## 生命周期与边界

所有 volume/file/cursor token 为同一进程内唯一的单调不重复 u64，关闭后不能再次使用。函数返回稳定的实验状态码；错误时不会把未实现操作伪装成成功。关闭仍有文件或目录游标的卷返回 BUSY。每个调用持有进程内互斥锁，当前全局串行仅为工程最小接入模型，不是最终并发性能方案。容器拥有 Rust Core 的 OS 独占锁，第二个打开者必须失败。

目录枚举在 begin 时先查询条目总数，再按最多 100,000 项限制读取和构造快照。已有文件名规则限制名称最多 255 字节；这仍是条目数限制，不是完整内存字节预算。读取/写入单次遵循 Core 的 MAX_IO 上限；ffi 数据指针必须满足 C 头文件规定的可访问性、长度和生命周期，非法任意非空指针无法被安全识别，故不允许未经审计的任意不可信进程直接调用。

C ABI 路径仅支持 UTF-8，不提供自动转换原生 Windows 宽字符或 macOS security scope。macOS FSKit 所需的 app extension、FSPathURLResource 授权、文件类型注册和 entitlement 尚未实现。Windows WinFsp 实际回调、驱动安装、签名、ACL、共享模式、通知、可执行映射、硬链接接口缺口尚未实现。不能声称真实系统挂载或 Codex 已兼容。

API 返回健康状态 0=正常、1=回收待空间、2=维护失败，并单独给出写保护标志；调用者不能因为业务写成功就忽略维护失败的警示。FFI 关闭仅释放核心 owner，不等于 VFS 安全卸载，未来内核 flush/fsync、Windows FlushFileBuffers、系统缓存回收、忙碌处理必须正确实现和验证。未实现功能应返回明确不支持，不通过无条件成功来消除错误。

## 验证
新 crate 属于 workspace，原三平台 rust-core CI 将在 Linux/macOS/Windows 构建、运行 Rust 调用 C ABI 的集成测试，并执行 clippy + fmt。包括 UTF-8 中文路径、持久化往返、创建/修改/截断、真实硬链接身份、目录枚举、错误与不覆盖、未关闭句柄阻止释放，以及第二 owner 忙碌检查。CI 使用各系统原生宿主目录，不是实际 exFAT 硬盘、FSKit/WinFsp 挂载测试。

通过状态以对应 PR 的准确提交和 Actions 为准；此文档本身不预宣称通过。

## 之后的硬性工作
1. macOS FSKit 真实签名/授权 app extension，以原生 FSVolume operations 调用 ABI，验证其 create/read/write/rename/unlink/sync，不能用普通目录伪装挂载。
2. Windows WinFsp 原生文件系统回调、盘符可见性、Windows sharing/delete、ACL、notification、mmap/executable 与 WinFsp 不支持 hardlink 的阻断处置。
3. Codex 本地、Git、IDE、pnpm/npm、uv/Cargo 的真实卷内工作与沙盒负向测试，依赖与编译结果保持在挂载卷内。
4. 优先补原生扩展属性、执行位、可扩展目录枚举、进程间并发与安全卸载。未通过全部门禁不能发布稳定版。

参考：
- https://developer.apple.com/documentation/FSKit/building-a-passthrough-file-system
- https://winfsp.dev/doc/NTFS-Compatibility/
- https://docs.rs/winfsp/latest/winfsp/
