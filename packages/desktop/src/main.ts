import { createApp } from "vue";
import App from "./App.vue";
import "./styles/index.css";

// 先用 UA 猜一个平台，避免 Windows 上首帧闪一下 macOS 的圆角与玻璃
const ua = navigator.userAgent;
document.documentElement.dataset.platform = ua.includes("Android")
  ? "android"
  : ua.includes("Windows")
    ? "windows"
    : "mac";

createApp(App).mount("#app");
