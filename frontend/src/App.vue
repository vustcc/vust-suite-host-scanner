<script setup lang="ts">
import { ref } from "vue";
import {
  VustAlert,
  VustButton,
  VustEmpty,
  VustIcon,
  VustSelect,
  VustTabs,
  VustTag,
  VustModal,
} from "@vustcc/vue";
import HostGridPanel from "@/components/HostGridPanel.vue";
import { useHostScanner } from "@/composables/useHostScanner";
import { t } from "@/i18n";
import type { HostScanResult, PortScanDetail } from "@/types/scanner";

const {
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
} = useHostScanner();

const activeWorkspacePanel = ref<"hosts" | "report">("hosts");

function toggleWorkspacePanel(panel: "hosts" | "report") {
  activeWorkspacePanel.value = panel;
}

const deleteTaskId = ref<string | null>(null);
const deleteModalVisible = ref(false);

function showDeleteConfirm(taskId: string, event: Event) {
  event.stopPropagation();
  deleteTaskId.value = taskId;
  deleteModalVisible.value = true;
}

async function handleConfirmDelete() {
  if (!deleteTaskId.value) return;
  try {
    await deleteTask(deleteTaskId.value);
  } catch (error: unknown) {
    window.alert(
      error instanceof Error ? error.message : t.value.deleteTaskFailed,
    );
  } finally {
    deleteModalVisible.value = false;
    deleteTaskId.value = null;
  }
}

function handleCancelDelete() {
  deleteModalVisible.value = false;
  deleteTaskId.value = null;
}

function portCount(host: HostScanResult, status: PortScanDetail["status"]) {
  return host.parsedPorts?.filter((port) => port.status === status).length ?? 0;
}

function portStatusLabel(status: PortScanDetail["status"]) {
  return status === "open" ? t.value.portStatusOpen : t.value.portStatusClosed;
}

function portBannerText(port: PortScanDetail) {
  if (port.banner) return port.banner;
  return port.status === "open"
    ? t.value.establishedNoBanner
    : t.value.closedPortNoBanner;
}
</script>

<template>
  <div class="host-scanner" data-page="host-scanner">
    <div class="app-header" data-ui="toolbar">
      <div class="header-logo">
        <div class="header-mark" :class="{ 'is-active': isScanning }">
          <VustIcon name="host-scanning" :size="28" />
        </div>
        <div class="logo-title">
          <h1>{{ t.title }}</h1>
          <p>{{ t.subtitle }}</p>
        </div>
      </div>

      <div class="env-badges" v-if="networkInfo">
        <VustTag type="default">
          <span class="badge-label">{{ t.network }}</span>
          <span class="badge-val">{{ networkInfo.networkMode }}</span>
        </VustTag>
        <VustTag :type="networkInfo.capNetRaw ? 'success' : 'danger'">
          <span class="badge-label">ICMP</span>
          <span class="badge-val">{{
            networkInfo.capNetRaw ? t.available : t.limited
          }}</span>
        </VustTag>
        <VustTag type="primary">
          <span class="badge-label">{{ t.containerIp }}</span>
          <span class="badge-val">{{ networkInfo.containerIp }}</span>
        </VustTag>
      </div>
      <div v-else-if="isFetchingEnv" class="env-loading">
        {{ t.fetchingEnv }}
      </div>
    </div>

    <div class="app-body" data-slot="content">
      <div class="body-left" data-ui="scan-control">
        <!-- 配置面板 -->
        <div class="panel config-panel">
          <div class="panel-header">
            <VustIcon name="settings" :size="16" />
            <span>{{ t.scanConfig }}</span>
          </div>
          <div class="panel-body">
            <div class="form-group">
              <label>{{ t.targetCidr }}</label>
              <input
                v-model="cidr"
                type="text"
                :placeholder="t.cidrPlaceholder"
                :disabled="isScanning"
              />
            </div>

            <div class="form-row">
              <div class="form-group flex-1">
                <label>{{ t.scanMode }}</label>
                <VustSelect
                  v-model="scanType"
                  :options="scanTypeOptions"
                  :disabled="isScanning"
                />
              </div>

              <div class="form-group flex-1">
                <label>{{ t.timeoutSec }}</label>
                <input
                  v-model.number="timeout"
                  type="number"
                  step="0.1"
                  min="0.1"
                  max="5.0"
                  :disabled="isScanning"
                />
              </div>
            </div>

            <div class="form-group" v-if="scanType === 'tcp'">
              <label>{{ t.portsLabel }}</label>
              <input
                v-model="ports"
                type="text"
                :placeholder="t.portsPlaceholder"
                :disabled="isScanning"
              />
            </div>

            <div class="form-group">
              <label>{{ t.concurrencyLimit }}</label>
              <input
                v-model.number="maxConcurrency"
                type="number"
                min="1"
                max="256"
                :disabled="isScanning"
              />
            </div>

            <VustAlert v-if="configError" type="error" show-icon>
              {{ configError }}
            </VustAlert>

            <div class="action-buttons">
              <VustButton
                class="scan-action"
                type="primary"
                size="large"
                :loading="isScanning"
                :disabled="isScanning"
                @click="startScan"
              >
                <VustIcon v-if="!isScanning" name="play" :size="16" />
                {{ isScanning ? t.scanning : t.startScan }}
              </VustButton>
              <VustButton
                v-if="isScanning"
                class="scan-action"
                type="danger"
                size="large"
                :loading="isCanceling"
                :disabled="isCanceling"
                @click="cancelScan"
              >
                <VustIcon v-if="!isCanceling" name="stop" :size="16" />
                {{ isCanceling ? t.cancelingScan : t.cancelScan }}
              </VustButton>
            </div>
          </div>
        </div>

        <!-- 实时扫描看板/指标 -->
        <div
          class="panel stats-panel"
          v-if="isScanning || stats.totalAlive > 0"
        >
          <div class="panel-header">
            <VustIcon name="chart" :size="16" />
            <span>{{ t.scanOverview }}</span>
          </div>
          <div class="panel-body">
            <!-- 实时进度条 -->
            <div class="progress-bar-container" v-if="isScanning">
              <div class="progress-bar-header">
                <span>{{ t.executing }}</span>
                <span>{{ scanProgress }}%</span>
              </div>
              <div class="progress-bar-track">
                <div
                  class="progress-bar-fill"
                  :style="{ width: scanProgress + '%' }"
                ></div>
              </div>
              <div class="progress-meta">
                <span>{{ t.progress(scannedHosts, totalHosts) }}</span>
                <span>{{ t.currentIp(currentHost) }}</span>
              </div>
            </div>

            <!-- 数据快照指标 -->
            <div class="stats-grid">
              <div class="stat-card">
                <span class="stat-label">{{ t.aliveHosts }}</span>
                <span class="stat-val text-success">{{
                  stats.totalAlive
                }}</span>
              </div>
              <div class="stat-card">
                <span class="stat-label">{{ t.openPorts }}</span>
                <span class="stat-val text-info">{{
                  stats.openPortCount
                }}</span>
              </div>
              <div class="stat-card" v-if="isScanning">
                <span class="stat-label">{{ t.elapsedTime }}</span>
                <span class="stat-val font-mono">{{ scanDuration }}s</span>
              </div>
              <div class="stat-card" v-else-if="selectedTask">
                <span class="stat-label">{{ t.totalTime }}</span>
                <span class="stat-val font-mono">{{ totalTime }}s</span>
              </div>
            </div>

            <!-- 端口分布 -->
            <div
              class="ports-chart-container"
              v-if="stats.sortedPorts.length > 0"
            >
              <label>{{ t.popularPorts }}</label>
              <div class="port-bar-list">
                <div
                  v-for="item in stats.sortedPorts"
                  :key="item.port"
                  class="port-bar-item"
                >
                  <span class="port-num font-mono">PORT {{ item.port }}</span>
                  <div class="port-track">
                    <div
                      class="port-fill"
                      :style="{
                        width: (item.count / stats.totalAlive) * 100 + '%',
                      }"
                    ></div>
                  </div>
                  <span class="port-count font-mono">{{
                    t.hostsCount(item.count)
                  }}</span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="body-right" data-ui="scan-workspace">
        <VustTabs v-model="activeTab" :tabs="scanTabs" />

        <div class="tab-content" v-if="activeTab === 'realtime'">
          <HostGridPanel
            :ip-list="ipList"
            :ip-states="ipStates"
            :results="currentResults"
            :interactive="isScanning || Boolean(selectedTask)"
            :collapsed="activeWorkspacePanel !== 'hosts'"
            @select="highlightHost"
            @toggle="toggleWorkspacePanel('hosts')"
          />

          <!-- 报告结果指纹列表 -->
          <div
            class="panel results-panel"
            :class="{ collapsed: activeWorkspacePanel !== 'report' }"
          >
            <button
              type="button"
              class="panel-header panel-toggle"
              :aria-expanded="activeWorkspacePanel === 'report'"
              @click="toggleWorkspacePanel('report')"
            >
              <VustIcon class="panel-chevron" name="chevron-down" :size="16" />
              <VustIcon name="server" :size="16" />
              <span>{{ t.assetReport }}</span>
              <span class="panel-count">{{
                t.itemsCount(currentResults.length)
              }}</span>
            </button>
            <div
              v-if="activeWorkspacePanel === 'report'"
              class="panel-body report-scroll"
            >
              <VustEmpty
                v-if="currentResults.length === 0"
                icon="network"
                :description="t.realtimeReportDesc"
              />

              <div class="results-list" v-else>
                <div
                  v-for="host in currentResults"
                  :key="host.host"
                  :id="'host-' + host.host"
                  class="host-card"
                  :class="{ expanded: host.expanded }"
                  @click="host.expanded = !host.expanded"
                >
                  <div class="card-summary">
                    <div class="summary-left">
                      <span class="icon-chevron"></span>
                      <span class="host-ip font-mono">{{ host.host }}</span>
                      <span
                        class="badge-ports"
                        v-if="portCount(host, 'open') > 0"
                      >
                        {{ t.openPortsCount(portCount(host, "open")) }}
                      </span>
                      <span
                        class="badge-ports closed"
                        v-if="portCount(host, 'closed') > 0"
                      >
                        {{ t.closedPortsCount(portCount(host, "closed")) }}
                      </span>
                    </div>
                    <div class="summary-right font-mono">
                      {{ host.detail }}
                    </div>
                  </div>

                  <!-- 展开显示详情 -->
                  <div class="card-detail" v-if="host.expanded">
                    <div class="detail-section">
                      <div class="section-title">{{ t.portDetailTitle }}</div>
                      <div
                        class="ports-table-wrapper"
                        v-if="host.parsedPorts && host.parsedPorts.length > 0"
                      >
                        <table class="ports-table">
                          <thead>
                            <tr>
                              <th>{{ t.thPort }}</th>
                              <th>{{ t.thStatus }}</th>
                              <th>{{ t.thBanner }}</th>
                            </tr>
                          </thead>
                          <tbody>
                            <tr v-for="p in host.parsedPorts" :key="p.port">
                              <td class="font-mono text-primary">
                                PORT {{ p.port }}
                              </td>
                              <td>
                                <span class="badge-status" :class="p.status">
                                  {{ portStatusLabel(p.status) }}
                                </span>
                              </td>
                              <td>
                                <code class="banner-code font-mono">{{
                                  portBannerText(p)
                                }}</code>
                              </td>
                            </tr>
                          </tbody>
                        </table>
                      </div>
                      <div class="no-ports-notice" v-else>
                        {{ t.hostIcmpOnlyNotice }}
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>

        <!-- 历史报告 TAB -->
        <div class="tab-content" v-else>
          <div class="panel history-panel">
            <div class="panel-header">
              <VustIcon name="log" :size="16" />
              <span>{{ t.historyReports }}</span>
            </div>
            <div class="panel-body report-scroll">
              <VustEmpty
                v-if="tasksList.length === 0"
                icon="log"
                :description="t.historyReportDesc"
              />

              <div class="history-list" v-else>
                <div
                  v-for="task in tasksList"
                  :key="task.id"
                  class="history-item"
                  @click="handleHistoryItemClick(task.id)"
                >
                  <div class="history-left">
                    <div class="history-cidr font-mono">{{ task.cidr }}</div>
                    <div class="history-meta">
                      <span>{{
                        t.scanMethod(
                          task.scan_type === "tcp"
                            ? t.tcpModeLabel
                            : t.icmpModeLabel,
                        )
                      }}</span>
                      <span>{{ t.scanPorts(task.ports) }}</span>
                      <span>{{
                        t.scanTime(new Date(task.created_at).toLocaleString())
                      }}</span>
                    </div>
                  </div>

                  <div class="history-right">
                    <span
                      class="history-stat"
                      v-html="t.foundAliveHosts(task.alive_hosts)"
                    ></span>
                    <VustButton
                      type="danger"
                      size="small"
                      @click="showDeleteConfirm(task.id, $event)"
                    >
                      <VustIcon name="trash" :size="14" />
                      {{ t.delete }}
                    </VustButton>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
    <!-- 删除确认弹窗 -->
    <VustModal
      :visible="deleteModalVisible"
      :title="t.confirmDelete"
      :message="t.confirmDeleteMessage"
      :confirm-text="t.confirmDelete"
      :cancel-text="t.cancel"
      type="danger"
      @confirm="handleConfirmDelete"
      @cancel="handleCancelDelete"
    />
  </div>
</template>

<style>
@import "@vustcc/tokens";
@import "@vustcc/vue/style.css";

/* CSS Reset and Global Integration */
html,
body,
#app {
  width: 100%;
  height: 100%;
  min-height: 0;
  margin: 0;
  overflow: hidden;
}

.host-scanner {
  width: 100%;
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background-color: var(--vdl-bg-canvas);
  color: var(--vdl-text-primary);
  font-family: var(--vdl-font-family);
  overflow: hidden;
  box-sizing: border-box;
}

.host-scanner * {
  box-sizing: border-box;
}

/* 顶部栏 */
.app-header {
  height: 60px;
  min-height: 60px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 var(--vdl-space-5);
  background-color: var(--vdl-bg-base);
  border-bottom: 1px solid var(--vdl-border-subtle);
}

.header-logo {
  display: flex;
  align-items: center;
  gap: var(--vdl-space-3);
}

.header-mark {
  width: 40px;
  height: 40px;
  display: grid;
  place-items: center;
  flex: 0 0 auto;
  border: 1px solid var(--vdl-border-brand);
  border-radius: var(--vdl-radius-md);
  background: var(--vdl-bg-active);
  color: var(--vdl-primary);
}

.header-mark.is-active {
  color: var(--vdl-info);
}

.header-mark.is-active .vl-icon {
  animation: header-scan-pulse 1s ease-in-out infinite alternate;
}

@keyframes header-scan-pulse {
  from {
    opacity: 0.55;
    transform: scale(0.9);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.logo-title h1 {
  margin: 0;
  font-size: 16px;
  font-weight: 700;
}

.logo-title p {
  margin: 2px 0 0 0;
  font-size: 11px;
  color: var(--vdl-text-muted);
}

.env-badges {
  display: flex;
  align-items: center;
  gap: var(--vdl-space-2);
}

.env-badges .vl-tag {
  display: flex;
  align-items: center;
  gap: 4px;
}

.badge-label {
  font-weight: 600;
  opacity: 0.75;
}

.badge-val {
  font-weight: 700;
  font-family: var(--vdl-font-mono);
}

.env-loading {
  color: var(--vdl-text-muted);
  font-size: var(--vdl-font-caption);
}

.text-success {
  color: var(--vdl-success);
}
.text-danger {
  color: var(--vdl-danger);
}
.text-info {
  color: var(--vdl-info);
}

/* 主体区域 */
.app-body {
  flex: 1;
  min-height: 0;
  display: flex;
  overflow: hidden;
}

.body-left {
  width: 380px;
  min-width: 380px;
  min-height: 0;
  border-right: 1px solid var(--vdl-border-subtle);
  display: flex;
  flex-direction: column;
  gap: var(--vdl-space-4);
  padding: var(--vdl-space-4);
  overflow: hidden;
}

.body-right {
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: var(--vdl-space-4);
  overflow: hidden;
  gap: var(--vdl-space-4);
}

/* 面板通用组件 */
.panel {
  min-height: 0;
  background-color: var(--vdl-bg-card);
  border: 1px solid var(--vdl-border-default);
  border-radius: var(--vdl-radius-md);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  box-shadow: var(--vdl-shadow-panel);
}

.panel-header {
  min-height: 40px;
  display: flex;
  align-items: center;
  gap: var(--vdl-space-2);
  padding: var(--vdl-space-3) var(--vdl-space-4);
  border-bottom: 1px solid var(--vdl-border-subtle);
  font-weight: 700;
  font-size: 13px;
  color: var(--vdl-text-primary);
  background-color: var(--vdl-bg-panel);
}

.panel-toggle {
  width: 100%;
  border-top: 0;
  border-left: 0;
  border-right: 0;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.panel-toggle > :not(.panel-chevron):first-of-type {
  color: var(--vdl-primary);
}

.panel-chevron {
  color: var(--vdl-text-muted);
  transition: transform 0.18s ease;
}

.panel-toggle[aria-expanded="false"] .panel-chevron {
  transform: rotate(-90deg);
}

.panel-count {
  margin-left: auto;
  padding: 2px 6px;
  border-radius: var(--vdl-radius-sm);
  color: var(--vdl-text-muted);
  background: var(--vdl-bg-muted);
  font-family: var(--vdl-font-mono);
  font-size: var(--vdl-font-caption);
  font-weight: 500;
}

.panel-header > :not(.panel-chevron):first-of-type {
  color: var(--vdl-primary);
}

.panel-body {
  min-height: 0;
  padding: var(--vdl-space-4);
}

.config-panel,
.stats-panel {
  flex: 1 1 0;
  min-height: 0;
}

.config-panel .panel-body,
.stats-panel .panel-body {
  flex: 1;
  overflow: auto;
  overscroll-behavior: contain;
}

/* 配置表单 */
.form-group {
  margin-bottom: var(--vdl-space-3);
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-group label {
  font-size: 11px;
  font-weight: 600;
  color: var(--vdl-text-secondary);
}

.form-group input,
.form-group select {
  padding: 8px 10px;
  border: 1px solid var(--vdl-border-default);
  border-radius: var(--vdl-radius-sm);
  background-color: var(--vdl-bg-input);
  color: var(--vdl-text-primary);
  font-size: 13px;
  outline: none;
}

.form-group .vl-select {
  font-size: 11px;
}

.form-group .vl-select-trigger {
  height: 28px;
  padding: 0 8px;
}

.form-group .vl-select .vl-select-label {
  font-size: 11px !important;
}

.vl-select-options .vl-select-option {
  font-size: 11px !important;
  padding: 4px var(--vdl-space-3) !important;
}

.form-group input:focus,
.form-group select:focus {
  border-color: var(--vdl-primary);
  box-shadow: var(--vdl-focus-ring);
}

.form-row {
  display: flex;
  gap: var(--vdl-space-3);
}

.flex-1 {
  flex: 1;
}

.action-buttons {
  margin-top: var(--vdl-space-4);
  display: flex;
  flex-direction: column;
  gap: var(--vdl-space-2);
}

.scan-action {
  width: 100%;
}

.scan-action .vl-button-content {
  display: inline-flex;
  align-items: center;
  gap: var(--vdl-space-2);
}

/* 进度条 */
.progress-bar-container {
  margin-bottom: var(--vdl-space-4);
  background: var(--vdl-bg-muted);
  padding: var(--vdl-space-3);
  border-radius: var(--vdl-radius-sm);
}

.progress-bar-header {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  margin-bottom: 6px;
  font-weight: 600;
}

.progress-bar-track {
  height: 6px;
  background: rgba(0, 0, 0, 0.06);
  border-radius: 99px;
  overflow: hidden;
}

.progress-bar-fill {
  height: 100%;
  background: var(--vdl-primary);
  border-radius: 99px;
  transition: width 0.2s ease;
}

.progress-meta {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--vdl-text-muted);
  margin-top: 6px;
}

/* 指标看板 */
.stats-grid {
  display: grid;
  grid-template-columns: repeat(2, 1fr);
  gap: var(--vdl-space-3);
}

.stat-card {
  padding: var(--vdl-space-3);
  background-color: var(--vdl-bg-muted);
  border: 1px solid var(--vdl-border-subtle);
  border-radius: var(--vdl-radius-sm);
  display: flex;
  flex-direction: column;
}

.stat-label {
  font-size: 11px;
  color: var(--vdl-text-muted);
}

.stat-val {
  font-size: 22px;
  font-weight: 750;
  margin-top: 4px;
}

/* 热门端口占比 */
.ports-chart-container {
  margin-top: var(--vdl-space-4);
  border-top: 1px solid var(--vdl-border-subtle);
  padding-top: var(--vdl-space-4);
}

.ports-chart-container label {
  font-size: 11px;
  font-weight: 600;
  color: var(--vdl-text-secondary);
  display: block;
  margin-bottom: var(--vdl-space-3);
}

.port-bar-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.port-bar-item {
  display: flex;
  align-items: center;
  font-size: 11px;
}

.port-num {
  width: 70px;
  font-weight: 700;
}

.port-track {
  flex: 1;
  height: 8px;
  background: rgba(0, 0, 0, 0.06);
  border-radius: 4px;
  overflow: hidden;
  margin: 0 var(--vdl-space-3);
}

.port-fill {
  height: 100%;
  background: var(--vdl-accent);
  border-radius: 4px;
}

.port-count {
  width: 40px;
  text-align: right;
  color: var(--vdl-text-muted);
}

.tab-content {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: var(--vdl-space-4);
  overflow: hidden;
}

/* 报告结果列表 */
.results-panel {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.results-panel.collapsed {
  flex: 0 0 auto;
}

.report-scroll {
  flex: 1;
  min-height: 0;
  overflow: auto;
  overscroll-behavior: contain;
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px;
  text-align: center;
  color: var(--vdl-text-muted);
}

.empty-state p {
  font-size: 12px;
  margin-top: var(--vdl-space-3);
  max-width: 320px;
}

.results-list {
  display: flex;
  flex-direction: column;
  gap: var(--vdl-space-2);
}

.host-card {
  border: 1px solid var(--vdl-border-default);
  border-radius: var(--vdl-radius-sm);
  background-color: var(--vdl-bg-card);
  cursor: pointer;
  overflow: hidden;
  transition: all 0.2s;
}

.host-card:nth-child(odd) {
  background-color: var(--vdl-bg-card);
}

.host-card:nth-child(even) {
  background-color: var(--vdl-bg-muted);
}

.host-card.expanded {
  background-color: var(--vdl-bg-base);
}

.host-card:hover {
  border-color: var(--vdl-primary);
  background-color: var(--vdl-bg-active);
}

.card-summary {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: var(--vdl-space-4);
  padding: var(--vdl-space-3) var(--vdl-space-4);
  font-size: 13px;
}

.summary-left {
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 12px;
}

.icon-chevron {
  width: 0;
  height: 0;
  border-left: 5px solid transparent;
  border-right: 5px solid transparent;
  border-top: 6px solid var(--vdl-text-muted);
  transition: transform 0.2s;
}

.host-card.expanded .icon-chevron {
  transform: rotate(180deg);
}

.host-ip {
  font-weight: 700;
}

.badge-ports {
  padding: 2px 6px;
  background-color: var(--vdl-success-soft);
  color: var(--vdl-success);
  border-radius: 99px;
  font-size: 10px;
  font-weight: 700;
}

.badge-ports.closed {
  background-color: var(--vdl-bg-muted);
  color: var(--vdl-text-secondary);
}

.summary-right {
  flex: 1;
  min-width: 0;
  color: var(--vdl-text-muted);
  font-size: 11px;
  line-height: 1.5;
  text-align: right;
  white-space: normal;
  word-break: break-word;
}

.card-detail {
  border-top: 1px solid var(--vdl-border-subtle);
  background-color: var(--vdl-bg-card);
  padding: var(--vdl-space-4);
}

.section-title {
  font-size: 11px;
  font-weight: 700;
  color: var(--vdl-text-muted);
  margin-bottom: var(--vdl-space-3);
  text-transform: uppercase;
}

.ports-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.ports-table th,
.ports-table td {
  padding: var(--vdl-space-2) var(--vdl-space-3);
  text-align: left;
  border-bottom: 1px solid var(--vdl-border-subtle);
}

.ports-table th {
  color: var(--vdl-text-muted);
  font-weight: 600;
  background-color: var(--vdl-bg-panel);
}

.badge-status {
  padding: 2px 6px;
  border-radius: var(--vdl-radius-xs);
  font-size: 10px;
  font-weight: 700;
}

.badge-status.open {
  background-color: var(--vdl-success-soft);
  color: var(--vdl-success);
}

.badge-status.closed {
  background-color: var(--vdl-bg-muted);
  color: var(--vdl-text-secondary);
}

.banner-code {
  background-color: var(--vdl-bg-muted);
  padding: 4px 8px;
  border-radius: var(--vdl-radius-xs);
  display: block;
  max-width: 100%;
  overflow-x: auto;
  font-size: 11px;
  border: 1px solid var(--vdl-border-subtle);
  white-space: pre-wrap;
  word-break: break-all;
}

.no-ports-notice {
  font-size: 12px;
  color: var(--vdl-text-muted);
}

/* 历史列表 */
.history-panel {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.history-list {
  display: flex;
  flex-direction: column;
  gap: var(--vdl-space-3);
}

.history-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: var(--vdl-space-4);
  border: 1px solid var(--vdl-border-default);
  border-radius: var(--vdl-radius-md);
  background-color: var(--vdl-bg-card);
  cursor: pointer;
  transition: all 0.2s;
}

.history-item:hover {
  border-color: var(--vdl-primary);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.03);
}

.history-cidr {
  font-size: 15px;
  font-weight: 700;
}

.history-meta {
  display: flex;
  gap: 12px;
  font-size: 11px;
  color: var(--vdl-text-muted);
  margin-top: 4px;
}

.history-right {
  display: flex;
  align-items: center;
  gap: var(--vdl-space-4);
}

.history-right .vl-button-content {
  display: inline-flex;
  align-items: center;
  gap: var(--vdl-space-1);
}

.history-right .vl-icon {
  display: block;
  flex: 0 0 auto;
}

.history-stat {
  font-size: 12px;
  color: var(--vdl-text-secondary);
}

.history-stat strong {
  color: var(--vdl-success);
  font-size: 14px;
}

@media (max-width: 980px) {
  .app-header {
    height: auto;
    min-height: 60px;
    align-items: flex-start;
    gap: var(--vdl-space-3);
    padding-block: var(--vdl-space-3);
  }

  .env-badges {
    justify-content: flex-end;
    flex-wrap: wrap;
  }

  .body-left {
    width: 320px;
    min-width: 320px;
  }
}

@media (max-width: 760px) {
  .env-badges {
    display: none;
  }

  .body-left {
    width: 280px;
    min-width: 280px;
    padding: var(--vdl-space-3);
  }

  .body-right {
    padding: var(--vdl-space-3);
  }

  .logo-title p {
    display: none;
  }
}

@media (max-height: 700px) {
  .ports-chart-container {
    display: none;
  }

  .panel-body {
    padding: var(--vdl-space-3);
  }

  .form-group {
    margin-bottom: var(--vdl-space-2);
    gap: var(--vdl-space-1);
  }

  .stat-card {
    padding: var(--vdl-space-2);
  }

  .stat-val {
    font-size: 18px;
  }
}
</style>
