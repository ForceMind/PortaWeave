# M0-AB · FSKit 硬链接检查源文件引用

在基线 `e1ddb7d56aa61a8a7f54fd6864343c57ce8031aa` 上，用实际 Linux C ABI 在私有 v1 夹具中创建并打开文件，删去唯一名字，再调用 FSKit 实验回调此前所用的 `pw_hard_link`。调用返回 0，新名字指向原对象 2。该入口是管理操作，能访问仍被持有的无名文件。此次复现覆盖管理 ABI，直接 SDK 路径的结果以对应 macOS CI 为准。

FSKit `createLink` 现在传入缓存项的管理型文件引用和预期对象 ID。`pw_fskit_file_link` 在当前会话查找正确类型的引用，核心在同一串行 owner 请求内检查对象匹配、源文件仍至少有一个名字、目标为有名字的目录且新名字未占用。随后复用原 SQLite 插入和时间更新事务。最后 unlink 或替换移除最后一个名字后，回调返回 ESTALE，已有读写句柄仍有效至关闭。重复目的名返回 EEXIST，错误类型或过期的原生 token 不能访问另一对象。只有新实验回调采用这些约束，旧管理接口保留既有语义。

新增 ABI 回归验证真实数据读取、剩余别名仍允许链接、最后 unlink 后的拒绝与无副作用。直接 SDK 回归分别检查最后 unlink 和替换移除旧目标，原对象的数据寿命独立验证。Apple 的接口要求指定要链接的既有项和目标目录。 [FSVolume createLink](https://developer.apple.com/documentation/fskit/fsvolume/operations/createlink%28to%3Anamed%3Aindirectory%3Areplyhandler%3A%29)

该引用是进程内的身份和数据寿命机制。实际原生调用者授权、内核分发和真实卷验收仍待验证。
