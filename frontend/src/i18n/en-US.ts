import type { zhCN } from "./zh-CN";

export const enUS: typeof zhCN = {
  deleteTaskFailed: "Delete task failed",
  hostIcmpOnlyNotice:
    "This host currently only responded to ICMP Ping discovery; no TCP service banners were found.",
  title: "Host Scanner",
  subtitle: "Network asset discovery, port probing & service banner grabbing",
  network: "Network",
  available: "Available",
  limited: "Limited",
  containerIp: "Container IP",
  fetchingEnv: "Reading container network environment...",
  scanConfig: "Scan Configuration",
  targetCidr: "Target CIDR (IPv4)",
  cidrPlaceholder: "e.g., 192.168.1.0/24",
  scanMode: "Scan Mode",
  timeoutSec: "Timeout (seconds)",
  portsLabel: "Probed TCP Ports (comma-separated)",
  portsPlaceholder: "e.g., 22,80,443,3389,8080",
  concurrencyLimit: "Max Concurrency",
  scanning: "Scanning...",
  startScan: "Start Scan",
  cancelScan: "Cancel Scan",
  cancelingScan: "Canceling...",
  scanOverview: "Scan Overview",
  executing: "Task running...",
  progress: (scanned: number | string, total: number | string) =>
    `Progress: ${scanned} / ${total}`,
  currentIp: (ip: string) => `Current IP: ${ip}`,
  aliveHosts: "Alive Hosts",
  openPorts: "Open Ports",
  elapsedTime: "Elapsed Time",
  totalTime: "Total Time",
  popularPorts: "Top Open Ports Distribution",
  hostsCount: (count: number) => `${count} Host${count === 1 ? "" : "s"}`,
  assetReport: "Asset Discovery Report",
  itemsCount: (count: number) => `${count} Item${count === 1 ? "" : "s"}`,
  realtimeReportDesc:
    "After configuring target network and starting the scan, alive hosts and service banners will keep updating here.",
  openPortsCount: (count: number) =>
    `${count} port${count === 1 ? "" : "s"} open`,
  closedPortsCount: (count: number) =>
    `${count} port${count === 1 ? "" : "s"} closed`,
  portDetailTitle: "TCP Port Probe Details",
  thPort: "Port",
  thStatus: "Status",
  thBanner: "Service Banner / Detail",
  portStatusOpen: "OPEN",
  portStatusClosed: "CLOSED",
  establishedNoBanner: "Connection established (No active banner pushed)",
  closedPortNoBanner:
    "Port closed (TCP RST received; the target path responded)",
  historyReports: "Historical Scan Reports",
  historyReportsCount: (count: number) => `Historical Scan Reports (${count})`,
  historyReportDesc:
    "After scan completes, task records and asset reports will be saved here.",
  scanMethod: (method: string) => `Method: ${method}`,
  scanPorts: (ports: string) => `Ports: ${ports}`,
  scanTime: (time: string) => `Scan Time: ${time}`,
  foundAliveHosts: (count: number | string) =>
    `Found <strong>${count}</strong> alive host${count === 1 ? "" : "s"}`,
  delete: "Delete",
  confirmDelete: "Confirm Delete",
  confirmDeleteMessage:
    "Are you sure you want to delete this scan task and its report? This action cannot be undone.",
  cancel: "Cancel",

  // HostGridPanel.vue
  legendIdle: "Idle",
  legendScanning: "Scanning",
  legendAliveNoPort: "Alive",
  legendAliveWithPort: "Ports Open",
  legendOffline: "No Response",
  unresponsive: "Unresponsive or currently unreachable",
  segmentHostStatus: "Network Host Status",
  legendTitle: "Host Status Legend",
  portListHover: (ports: string) => `Ports: ${ports}`,

  // useHostScanner.ts
  tcpModeLabel: "TCP Port Scan & Service Identification",
  icmpModeLabel: "ICMP Ping Active Host Discovery",
  tabRealtime: "Real-time Scan & Host Status",
  fetchEnvFailed: "Failed to read network environment:",
  fetchTasksFailed: "Failed to fetch task list:",
  fetchTaskDetailFailed: "Failed to pull task details:",
  parseSseError: "Failed to parse SSE data:",
  invalidCidrError:
    "Please enter a valid IPv4 CIDR, currently supporting /24 to /32.",
  readyToScan: "Ready to Scan",
  startScanFailed: "Failed to start scan task",
  cancelScanFailed: "Failed to cancel scan task",
};
