import { computed, onMounted, onUnmounted, reactive, ref } from "vue";
import axios from "axios";
import { t } from "@/i18n";
import type {
  HostScanResult,
  HostState,
  HostVisualState,
  NetworkInfo,
  PortScanDetail,
  ScanProgressHostResult,
  ScanProgressUpdate,
  ScanTask,
} from "@/types/scanner";

/**
 * @description 管理主机扫描配置、任务生命周期、SSE 进度和结果状态。
 */
export function useHostScanner() {
  const networkInfo = ref<NetworkInfo | null>(null);
  const isFetchingEnv = ref(true);
  const cidr = ref("192.168.1.0/24");
  const scanType = ref<"tcp" | "icmp">("tcp");
  const scanTypeOptions = computed(() => [
    { value: "tcp", label: t.value.tcpModeLabel },
    { value: "icmp", label: t.value.icmpModeLabel },
  ]);
  const ports = ref("22,80,443,3389,8080");
  const timeout = ref(1);
  const maxConcurrency = ref(32);

  const currentTaskId = ref<string | null>(null);
  const scanProgress = ref(0);
  const scannedHosts = ref(0);
  const totalHosts = ref(0);
  const currentHost = ref("");
  const isScanning = ref(false);
  const isCanceling = ref(false);
  const scanDuration = ref(0);
  const currentResults = ref<HostScanResult[]>([]);
  const selectedTask = ref<ScanTask | null>(null);
  const tasksList = ref<ScanTask[]>([]);
  const activeTab = ref("realtime");
  const ipList = ref<string[]>([]);
  const ipStates = reactive<Record<string, HostState>>({});
  const configError = ref("");

  const scanTabs = computed(() => [
    { name: "realtime", label: t.value.tabRealtime },
    {
      name: "history",
      label: t.value.historyReportsCount(tasksList.value.length),
    },
  ]);

  let timerInterval: number | null = null;
  let sse: EventSource | null = null;
  let scanStartedAt = 0;
  const apiBase = "api";
  const completedTaskId = ref<string | null>(null);
  const completedDuration = ref(0);
  const hostVisualStates = new Set<HostVisualState>([
    "pending",
    "scanning",
    "alive-no-port",
    "alive-with-port",
    "offline",
  ]);

  function isRunningTask(task: ScanTask) {
    return task.status === "pending" || task.status === "scanning";
  }

  function isTerminalTaskStatus(status: string) {
    return (
      status === "completed" || status === "failed" || status === "canceled"
    );
  }

  function getRequestErrorMessage(error: unknown): string | null {
    if (axios.isAxiosError(error)) {
      const responseData = error.response?.data;
      if (typeof responseData === "string") return responseData;
      if (
        responseData &&
        typeof responseData === "object" &&
        "message" in responseData &&
        typeof responseData.message === "string"
      ) {
        return responseData.message;
      }
    }
    return error instanceof Error ? error.message : null;
  }

  /**
   * @description 将后端进度事件中的主机状态规整为前端可展示状态。
   */
  function normalizeHostStatus(status: unknown): HostVisualState | null {
    if (typeof status !== "string") return null;
    return hostVisualStates.has(status as HostVisualState)
      ? (status as HostVisualState)
      : null;
  }

  /**
   * @description 将受支持的 IPv4 CIDR 转换为网格 IP 列表。
   */
  function parseCidr(cidrValue: string): string[] {
    const [ip, maskValue] = cidrValue.trim().split("/");
    if (!ip || !maskValue) return [];

    const mask = Number.parseInt(maskValue, 10);
    if (!Number.isInteger(mask) || mask < 24 || mask > 32) return [];

    const octets = ip.split(".").map(Number);
    if (
      octets.length !== 4 ||
      octets.some((item) => !Number.isInteger(item) || item < 0 || item > 255)
    ) {
      return [];
    }

    const [first = 0, second = 0, third = 0, fourth = 0] = octets;
    const ipNumber = (first << 24) + (second << 16) + (third << 8) + fourth;
    const hostCount = 1 << (32 - mask);

    return Array.from({ length: hostCount }, (_, index) => {
      const current = ipNumber + index;
      return [
        (current >>> 24) & 255,
        (current >>> 16) & 255,
        (current >>> 8) & 255,
        current & 255,
      ].join(".");
    });
  }

  /**
   * @description 根据当前 CIDR 重置主机网格为空闲状态。
   */
  function initIpGrid() {
    const list = parseCidr(cidr.value);
    ipList.value = list;
    Object.keys(ipStates).forEach((ip) => delete ipStates[ip]);
    list.forEach((ip) => {
      ipStates[ip] = { status: "pending" };
    });
  }

  /**
   * @description 获取容器网络环境并推导默认扫描网段。
   */
  async function fetchNetworkEnv() {
    try {
      isFetchingEnv.value = true;
      const { data } = await axios.get<NetworkInfo>(
        `${apiBase}/runtime/network`,
      );
      networkInfo.value = data;
      const parts = data.containerIp.split(".");
      if (data.containerIp !== "127.0.0.1" && parts.length === 4) {
        cidr.value = `${parts[0]}.${parts[1]}.${parts[2]}.0/24`;
        initIpGrid();
      }
    } catch (error) {
      console.error(t.value.fetchEnvFailed, error);
    } finally {
      isFetchingEnv.value = false;
    }
  }

  /**
   * @description 刷新历史扫描任务列表。
   */
  async function fetchTasksList(): Promise<ScanTask[]> {
    try {
      const { data } = await axios.get<ScanTask[]>(`${apiBase}/tasks`);
      tasksList.value = data;
      return data;
    } catch (error) {
      console.error(t.value.fetchTasksFailed, error);
      return [];
    }
  }

  /**
   * @description 读取扫描任务详情并恢复主机状态网格。
   */
  async function fetchTaskDetail(
    taskId: string,
    options: { markUnknownOffline?: boolean } = {},
  ): Promise<ScanTask | null> {
    try {
      const markUnknownOffline = options.markUnknownOffline ?? true;
      const { data } = await axios.get<{
        task: ScanTask;
        results: HostScanResult[];
      }>(`${apiBase}/tasks/${taskId}`);
      selectedTask.value = data.task;
      currentResults.value = data.results.map((result) => {
        let parsedPorts: PortScanDetail[] = [];
        try {
          parsedPorts = JSON.parse(result.ports);
        } catch {
          parsedPorts = [];
        }
        return { ...result, parsedPorts, expanded: false };
      });

      const hosts = parseCidr(data.task.cidr);
      ipList.value = hosts;
      Object.keys(ipStates).forEach((ip) => delete ipStates[ip]);
      hosts.forEach((ip) => {
        ipStates[ip] = { status: markUnknownOffline ? "offline" : "pending" };
      });
      currentResults.value.forEach((result) => {
        if (!ipStates[result.host]) return;
        const openPorts =
          result.parsedPorts
            ?.filter((port) => port.status === "open")
            .map((port) => port.port) ?? [];
        ipStates[result.host] = {
          status: openPorts.length > 0 ? "alive-with-port" : "alive-no-port",
          ports: openPorts,
        };
      });
      return data.task;
    } catch (error) {
      console.error(t.value.fetchTaskDetailFailed, error);
      return null;
    }
  }

  function startScanTimer(startedAt: number) {
    scanStartedAt = startedAt;
    if (timerInterval !== null) window.clearInterval(timerInterval);
    timerInterval = window.setInterval(() => {
      scanDuration.value = Number(
        ((performance.now() - scanStartedAt) / 1000).toFixed(1),
      );
    }, 100);
  }

  function startRestoredScanTimer(task: ScanTask) {
    const startedAt = parseDateTime(task.created_at);
    const elapsedMs = startedAt
      ? Math.max(0, Date.now() - startedAt.getTime())
      : 0;
    const performanceStartedAt = performance.now() - elapsedMs;
    scanDuration.value = Number((elapsedMs / 1000).toFixed(1));
    startScanTimer(performanceStartedAt);
  }

  function applyRunningTaskSnapshot(task: ScanTask) {
    cidr.value = task.cidr;
    scanType.value = task.scan_type === "icmp" ? "icmp" : "tcp";
    ports.value = task.ports;
    timeout.value = task.timeout;
    currentTaskId.value = task.id;
    selectedTask.value = task;
    scanProgress.value = task.progress;
    scannedHosts.value = task.scanned_hosts;
    totalHosts.value = task.total_hosts;
    currentHost.value =
      task.scanned_hosts > 0 ? t.value.executing : t.value.readyToScan;
    isScanning.value = true;
    isCanceling.value = false;
    configError.value = "";
    completedTaskId.value = null;
    completedDuration.value = 0;
    activeTab.value = "realtime";
    startRestoredScanTimer(task);
    connectSse(task.id);
  }

  async function restoreRunningTask(tasks: ScanTask[]) {
    const runningTask = tasks.find(isRunningTask);
    if (!runningTask) return;

    const task = await fetchTaskDetail(runningTask.id, {
      markUnknownOffline: false,
    });
    if (!task) return;
    if (!isRunningTask(task)) {
      await completeScanFlow(task.id);
      return;
    }

    applyRunningTaskSnapshot(task);
  }

  /**
   * @description 关闭进度通道并同步任务最终状态。
   */
  async function completeScanFlow(taskId: string) {
    sse?.close();
    sse = null;
    if (timerInterval !== null) {
      window.clearInterval(timerInterval);
      timerInterval = null;
    }
    if (scanStartedAt > 0) {
      completedTaskId.value = taskId;
      completedDuration.value = Math.max(
        0.1,
        Number(((performance.now() - scanStartedAt) / 1000).toFixed(1)),
      );
    }
    await fetchTaskDetail(taskId);
    await fetchTasksList();
    isScanning.value = false;
    isCanceling.value = false;
    currentTaskId.value = null;
  }

  /**
   * @description SSE 异常关闭时同步后端最终状态，避免把临时断流误判为扫描结束。
   */
  async function syncFinalTaskState(taskId: string) {
    const task = await fetchTaskDetail(taskId, { markUnknownOffline: false });
    if (!task) return;
    if (isTerminalTaskStatus(task.status)) {
      await completeScanFlow(taskId);
      return;
    }
    if (currentTaskId.value === taskId && isScanning.value) {
      window.setTimeout(() => {
        if (currentTaskId.value === taskId && isScanning.value) {
          void syncFinalTaskState(taskId);
        }
      }, 1200);
    }
  }

  /**
   * @description 按主机地址合并 SSE 资产结果，保证事件回放不会生成重复报告。
   */
  function upsertProgressHostResult(
    taskId: string,
    result: ScanProgressHostResult,
  ) {
    const index = currentResults.value.findIndex(
      (item) => item.host === result.host,
    );
    const existing = index >= 0 ? currentResults.value[index] : undefined;
    const parsedPorts = result.ports.map((port) => ({ ...port }));
    const hostResult: HostScanResult = {
      id: existing?.id ?? 0,
      task_id: taskId,
      host: result.host,
      status: result.status,
      ports: JSON.stringify(parsedPorts),
      detail: result.detail,
      parsedPorts,
      expanded: existing?.expanded ?? false,
    };

    if (index >= 0) {
      currentResults.value[index] = hostResult;
    } else {
      currentResults.value.push(hostResult);
    }
  }

  /**
   * @description 连接任务 SSE 进度流并实时更新主机状态与资产报告。
   */
  function connectSse(taskId: string) {
    sse?.close();
    sse = new EventSource(`${apiBase}/tasks/${taskId}/progress`);
    sse.onmessage = (event) => {
      try {
        const update = JSON.parse(event.data) as ScanProgressUpdate;
        scanProgress.value = update.progress;
        scannedHosts.value = update.scanned_hosts;
        totalHosts.value = update.total_hosts;
        currentHost.value = update.current_host;
        if (update.current_host && update.current_host !== "finished") {
          const state = ipStates[update.current_host];
          const hostStatus = normalizeHostStatus(update.host_status);
          if (state && hostStatus) {
            ipStates[update.current_host] = {
              status: hostStatus,
              ports: update.open_ports,
            };
          }
        }
        if (update.host_result) {
          upsertProgressHostResult(update.task_id, update.host_result);
        }
        if (isTerminalTaskStatus(update.status)) {
          void completeScanFlow(taskId);
        }
      } catch (error) {
        console.error(t.value.parseSseError, error);
      }
    };
    sse.onerror = () => {
      void syncFinalTaskState(taskId);
    };
  }

  /**
   * @description 校验配置并创建扫描任务。
   */
  async function startScan() {
    if (isScanning.value) return;
    const hosts = parseCidr(cidr.value);
    if (hosts.length === 0) {
      configError.value = t.value.invalidCidrError;
      return;
    }

    configError.value = "";
    isScanning.value = true;
    isCanceling.value = false;
    scanProgress.value = 0;
    scannedHosts.value = 0;
    totalHosts.value = hosts.length;
    currentHost.value = t.value.readyToScan;
    currentResults.value = [];
    selectedTask.value = null;
    scanDuration.value = 0;
    completedTaskId.value = null;
    completedDuration.value = 0;
    activeTab.value = "realtime";
    initIpGrid();

    startScanTimer(performance.now());

    try {
      const { data } = await axios.post<{ taskId: string }>(`${apiBase}/scan`, {
        cidr: cidr.value,
        scanType: scanType.value,
        ports: ports.value,
        timeout: timeout.value,
        maxConcurrency: maxConcurrency.value,
      });
      currentTaskId.value = data.taskId;
      connectSse(data.taskId);
    } catch (error) {
      isScanning.value = false;
      if (timerInterval !== null) {
        window.clearInterval(timerInterval);
        timerInterval = null;
      }
      scanStartedAt = 0;
      const message = getRequestErrorMessage(error);
      configError.value =
        typeof message === "string" ? message : t.value.startScanFailed;
    }
  }

  /**
   * @description 请求后端停止当前扫描任务，并等待 SSE 或任务同步确认最终状态。
   */
  async function cancelScan() {
    const taskId = currentTaskId.value;
    if (!taskId || !isScanning.value || isCanceling.value) return;

    isCanceling.value = true;
    configError.value = "";
    try {
      await axios.post(`${apiBase}/tasks/${taskId}/cancel`);
      window.setTimeout(() => {
        if (currentTaskId.value === taskId && isScanning.value) {
          void syncFinalTaskState(taskId);
        }
      }, 1200);
    } catch (error) {
      isCanceling.value = false;
      const message = getRequestErrorMessage(error);
      configError.value =
        typeof message === "string" ? message : t.value.cancelScanFailed;
    }
  }

  /**
   * @description 删除历史任务及其扫描结果。
   */
  async function deleteTask(taskId: string) {
    try {
      await axios.delete(`${apiBase}/tasks/${taskId}`);
      if (selectedTask.value?.id === taskId) {
        sse?.close();
        sse = null;
        if (timerInterval !== null) {
          window.clearInterval(timerInterval);
          timerInterval = null;
        }
        isScanning.value = false;
        isCanceling.value = false;
        currentTaskId.value = null;
        selectedTask.value = null;
        currentResults.value = [];
        initIpGrid();
      }
      await fetchTasksList();
    } catch (err) {
      console.error(err);
      throw new Error("删除任务失败");
    }
  }

  /**
   * @description 从按 IP 索引的实时状态计算存活主机、开放端口和端口分布。
   */
  const stats = computed(() => {
    let totalAlive = 0;
    let openPortCount = 0;
    const portCounts: Record<number, number> = {};
    Object.values(ipStates).forEach((state) => {
      const isAlive =
        state.status === "alive-no-port" || state.status === "alive-with-port";
      if (!isAlive) return;
      totalAlive += 1;
      if (state.status !== "alive-with-port") return;

      const uniqueOpenPorts = new Set(state.ports ?? []);
      openPortCount += uniqueOpenPorts.size;
      uniqueOpenPorts.forEach((port) => {
        portCounts[port] = (portCounts[port] ?? 0) + 1;
      });
    });
    return {
      totalAlive,
      openPortCount,
      sortedPorts: Object.entries(portCounts)
        .map(([port, count]) => ({ port: Number(port), count }))
        .sort(
          (left, right) => right.count - left.count || left.port - right.port,
        )
        .slice(0, 5),
    };
  });

  /**
   * @description 解析后端时间字符串并兼容微秒精度。
   */
  function parseDateTime(value: string | null | undefined): Date | null {
    if (!value) return null;
    const direct = new Date(value);
    if (!Number.isNaN(direct.getTime())) return direct;
    const match = value.match(
      /^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d+))?(Z|[+-]\d{2}:?\d{2})?$/,
    );
    if (!match?.[1]) return null;
    const milliseconds = (match[2] ?? "000").slice(0, 3).padEnd(3, "0");
    const parsed = new Date(`${match[1]}.${milliseconds}${match[3] ?? ""}`);
    return Number.isNaN(parsed.getTime()) ? null : parsed;
  }

  const totalTime = computed(() => {
    const task = selectedTask.value;
    if (!task) return 0;
    if (task.id === completedTaskId.value && completedDuration.value > 0) {
      return completedDuration.value;
    }
    if (task.id === currentTaskId.value && scanDuration.value > 0)
      return scanDuration.value;
    const startedAt = parseDateTime(task.created_at);
    const completedAt = parseDateTime(task.completed_at);
    if (startedAt && completedAt) {
      return Math.max(
        0.1,
        Number(
          ((completedAt.getTime() - startedAt.getTime()) / 1000).toFixed(1),
        ),
      );
    }
    return task.timeout;
  });

  /**
   * @description 展开指定主机结果并滚动到对应报告。
   */
  function highlightHost(ip: string) {
    const result = currentResults.value.find((item) => item.host === ip);
    if (!result) return;
    result.expanded = !result.expanded;
    document
      .getElementById(`host-${ip}`)
      ?.scrollIntoView({ behavior: "smooth", block: "center" });
  }

  async function handleHistoryItemClick(taskId: string) {
    await fetchTaskDetail(taskId);
    activeTab.value = "realtime";
  }

  onMounted(() => {
    void (async () => {
      await fetchNetworkEnv();
      const tasks = await fetchTasksList();
      initIpGrid();
      await restoreRunningTask(tasks);
    })();
  });

  onUnmounted(() => {
    sse?.close();
    if (timerInterval !== null) window.clearInterval(timerInterval);
  });

  return {
    networkInfo,
    isFetchingEnv,
    cidr,
    scanType,
    scanTypeOptions,
    ports,
    timeout,
    maxConcurrency,
    scanProgress,
    scannedHosts,
    totalHosts,
    currentHost,
    isScanning,
    isCanceling,
    scanDuration,
    currentResults,
    selectedTask,
    tasksList,
    activeTab,
    scanTabs,
    ipList,
    ipStates,
    configError,
    stats,
    totalTime,
    startScan,
    cancelScan,
    deleteTask,
    highlightHost,
    handleHistoryItemClick,
  };
}
