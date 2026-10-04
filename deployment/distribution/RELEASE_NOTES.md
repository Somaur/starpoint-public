# Starpoint Somaur — Android 预发行版

基于 x380kkm/starpoint-public 的 Android 分支，包含截至 2026-10-04 的本地修复。

- 修复教程完成状态、合击和自动战斗解锁、十连票券、活动入口和轮播图。
- 修复战斗结算、任务奖励和部分活动／商店数据；保留原客户端的同种装备规则。
- 随包提供《弹射小世界》36 话英文和史黛拉讲座 13 话繁体中文；新安装无需手动导入漫画。
- 个人服务仅在本机运行。发行包不包含发布者的存档、设备信息或调试日志。

## 安装

1. 下载本 Release 的全部 `.partNNN` 文件、`starpoint-mobile-release.json` 和 `restore_release.py`，放在同一目录。
2. 使用 Python 3.11 或更新版执行 `python restore_release.py`。脚本校验每片及完整 APK 的 SHA-256 后生成安装包。
3. 把合并后的 APK 传到 Android 手机安装，首次启动等待 CDN 解包，再从伴随应用安装／打开游戏。

也可只下载清单和恢复脚本，执行：

```text
python restore_release.py --release-url https://github.com/Somaur/starpoint-public/releases/download/本次标签
```

分片不是单独的安装包。电脑合并时需要约 24 GB 可用空间；手机需要容纳安装包及约 12 GB 解包资源，建议预留至少 25 GB。当前仅支持 Android ARM64。

更新前请导出自己的存档。此分发使用 Somaur 的独立签名；从其他分发方安装的应用不能直接覆盖安装，须先备份再迁移。本地调试版使用同一签名，但新包的资源清单增加漫画，首次更新会重新准备 CDN。

## 来源与构建范围

服务修复源码、数据生成脚本和发布流程在本仓库 `android` 分支；准确提交号写入下载清单和 APK 元数据。原版客户端、上游 Java 启动器和 CDN 作为固定校验值的外部输入，构建流程重新编译 Rust/JNI 服务、应用已核验的 SWF 补丁、重新签名并生成完整资源分片。

游戏资源的权利归原权利人；上游代码许可和署名保留于仓库。此项目与原发行商及上游维护者独立。
