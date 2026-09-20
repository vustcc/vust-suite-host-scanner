# Changelog

## [Unreleased]

### Added

- 接入 Suite Runtime 主题桥接，支持跟随 VUST 主控实时同步主题与液态玻璃开关。

### Changed

- 扫描配置、实时状态、主机结果和任务历史面板全面适配公共 Liquid Glass 材质，并统一标准图标、表单和操作控件的视觉表现。

## [0.1.0-alpha.1] - 2026-09-10

### Added

- 创建 VUST 主机扫描套件全新仓库。
- 支持 `/24` 至 `/32` IPv4 CIDR、ICMP 存活探测、TCP 端口探测与 Banner 服务识别。
- 提供实时扫描状态、任务历史、开放及关闭端口和服务指纹报告。
- 支持取消运行中的扫描，并在服务重启后正确终结中断任务。
- 提供基于 VUST UI、VDL Token 与 Suite Runtime SDK 的 Vue 3 界面和语义操作日志。
- 提供 `vustcc/host-scanner:0.1.0-alpha.1` Docker 镜像构建与手动发布流程。
