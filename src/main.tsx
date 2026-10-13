import "@openkeyboard/app/styles.css";
import { App } from "@openkeyboard/app/App";
import type { AppBranding } from "@openkeyboard/app/branding";
import { KeyboardConnection } from "@openkeyboard/connection/keyboard-connection";
import { keyboardRequestFilters } from "@openkeyboard/drivers/keyboard-driver-registry";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { NativeHid } from "./native-hid/native-hid";
import { tauriHidTransport } from "./native-hid/native-hid-transport";

const DESKTOP_BRANDING: AppBranding = {
  tagline: "Configure your keyboard. No vendor app required.",
  footnote: "Desktop app with direct access to your keyboards.",
};

const appRoot = document.querySelector<HTMLElement>("#app");
if (!appRoot) throw new Error("The page is missing the #app container.");

const nativeHid = new NativeHid(tauriHidTransport, keyboardRequestFilters());
const connection = new KeyboardConnection(nativeHid.asHid());
createRoot(appRoot).render(
  <StrictMode>
    <App connection={connection} branding={DESKTOP_BRANDING} />
  </StrictMode>,
);
void connection.start();
