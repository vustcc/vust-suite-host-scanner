import { createApp } from "vue";
import { createSuiteBridge } from "@vustcc/suite-sdk";
import App from "./App.vue";
import router from "./router";
import { setLocale } from "./i18n";

const suiteBridge = createSuiteBridge({
  capabilities: ["theme", "locale", "window"],
  supportedLocales: ["zh-CN", "en-US"],
  defaultLocale: "zh-CN",
});

// Subscribe to locale changes and synchronize with our i18n state
suiteBridge.subscribeLocale(({ locale }) => {
  setLocale(locale);
});

suiteBridge.ready();

const app = createApp(App);

app.use(router);

app.mount("#app");
