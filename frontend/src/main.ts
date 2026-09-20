import { createApp } from "vue";
import App from "./App.vue";
import router from "./router";
import { setLocale } from "./i18n";
import { suiteBridge } from "./suite-bridge";

// Subscribe to locale changes and synchronize with our i18n state
suiteBridge.subscribeLocale(({ locale }) => {
  setLocale(locale);
});

suiteBridge.ready();

const app = createApp(App);

app.use(router);

app.mount("#app");
