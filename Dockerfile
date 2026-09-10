FROM debian:trixie-slim

LABEL vust.owner="suite"

# 安装 ping (ICMP 探测必须) 与 curl (健康检查必须)
RUN apt-get update \
    && apt-get install -y --no-install-recommends iputils-ping curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY target/release/vust-host-scanner .

EXPOSE 8080

CMD ["./vust-host-scanner", "8080"]
