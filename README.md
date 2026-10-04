# Starpoint Somaur

《世界弹射物语》Android 本地版的独立维护与分发 Fork。基于 [x380kkm/starpoint-public](https://github.com/x380kkm/starpoint-public/tree/android)，包含 2026-10-04 前的本地调试修复。

- **给朋友安装：** [下载发行版](https://github.com/Somaur/starpoint-public/releases)；完整安装说明见 [发行说明](deployment/distribution/RELEASE_NOTES.md)。
- **开发和发布：** [构建与签名说明](deployment/distribution/README.md)、[Actions](https://github.com/Somaur/starpoint-public/actions)。
- **源码分支：** `android`。`main`、`ios` 保留上游内容，本次分发只覆盖已调试的 Android ARM64 版本。

修复覆盖教程／合击／自动战斗解锁、票券与抽卡资源、活动展示、战斗及任务结算、商店和本地管理页面。装备遵循当前官方客户端规则。漫画随资源包提供 36 话英文《弹射小世界》和 13 话繁体中文史黛拉讲座。

服务源码和修复数据保存在 Git；大型客户端及 CDN 通过 Release 分片分发。每次发布提供每片及完整 APK 的 SHA-256、恢复脚本和对应源码提交，朋友无需搭建服务器。

```text
git clone --branch android https://github.com/Somaur/starpoint-public.git
```

上游完整开发文档见 [README.upstream.md](README.upstream.md)。保留 [LICENSE](LICENSE) 与上游署名；游戏客户端、图片、声音等外部资源的权利属于原权利人。本 Fork 与原游戏发行商、上游维护者保持独立。
