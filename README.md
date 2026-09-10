# VUST 主机扫描

VUST 主机扫描套件镜像，用于 IPv4 主机发现和常见 TCP 端口探测。

VUST 是一个可扩展的分布式 Linux 主机运维平台，用于统一管理一台或多台 Linux 主机，并支持通过套件按需扩展平台能力。

## 镜像

```text
vustcc/host-scanner:0.1.0-alpha.1
```

## 运行示例

```bash
docker network create vust-suite-network
docker run --rm \
  --name vust-host-scanner \
  --network vust-suite-network \
  --cap-add NET_RAW \
  -v vust-host-scanner-data:/data \
  vustcc/host-scanner:0.1.0-alpha.1
```

## 本地构建

```bash
./build-image.sh 0.1.0-alpha.1
```

本仓库只维护套件源码和 Docker 镜像。`.vsp` 套件交付包由 `vust-suites` 仓库统一维护和发布。

在 VUST 内运行时，后端通过 Suite Runtime SDK 使用 `operation-logs.write` 能力上报扫描提交、终态和删除事件；运行时令牌仅挂载到后端容器。
