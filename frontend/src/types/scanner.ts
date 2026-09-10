export interface NetworkInfo {
  hostname: string;
  containerIp: string;
  defaultRoute: string;
  dnsServers: string[];
  capNetRaw: boolean;
  networkMode: string;
}

export interface ScanTask {
  id: string;
  cidr: string;
  scan_type: string;
  ports: string;
  timeout: number;
  status: string;
  progress: number;
  total_hosts: number;
  scanned_hosts: number;
  alive_hosts: number;
  created_at: string;
  completed_at: string | null;
}

export interface PortScanDetail {
  port: number;
  status: "open" | "closed";
  banner: string | null;
}

export interface HostScanResult {
  id: number;
  task_id: string;
  host: string;
  status: string;
  ports: string;
  detail: string;
  parsedPorts?: PortScanDetail[];
  expanded?: boolean;
}

export interface ScanProgressHostResult {
  host: string;
  status: string;
  ports: PortScanDetail[];
  detail: string;
}

export type HostVisualState =
  | "pending"
  | "scanning"
  | "alive-no-port"
  | "alive-with-port"
  | "offline";

export interface HostState {
  status: HostVisualState;
  ports?: number[];
}

export interface ScanProgressUpdate {
  task_id: string;
  progress: number;
  scanned_hosts: number;
  total_hosts: number;
  current_host: string;
  status: string;
  host_status?: HostVisualState;
  open_ports: number[];
  host_result: ScanProgressHostResult | null;
}
