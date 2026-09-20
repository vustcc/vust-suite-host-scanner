import { createSuiteBridge } from "@vustcc/suite-sdk";

/** 主机扫描套件与 VUST 壳层之间的主题、语言和窗口桥接。 */
export const suiteBridge = createSuiteBridge({
  capabilities: ["theme", "locale", "window"],
  supportedLocales: ["zh-CN", "en-US"],
  defaultLocale: "zh-CN",
});
