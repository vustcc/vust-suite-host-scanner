import { ref, computed } from "vue";
import { zhCN } from "./zh-CN";
import { enUS } from "./en-US";

export type LocaleType = "zh-CN" | "en-US";
export type TranslationType = typeof zhCN;

const translations: Record<LocaleType, TranslationType> = {
  "zh-CN": zhCN,
  "en-US": enUS,
};

// Default locale
export const currentLocale = ref<LocaleType>("zh-CN");

// The translation computed getter
export const t = computed<TranslationType>(() => {
  return translations[currentLocale.value] || zhCN;
});

export function setLocale(locale: string) {
  if (locale.startsWith("en")) {
    currentLocale.value = "en-US";
  } else {
    currentLocale.value = "zh-CN";
  }
}
