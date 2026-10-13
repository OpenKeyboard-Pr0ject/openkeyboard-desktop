import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface NativeDeviceInfo {
  readonly deviceKey: string;
  readonly vendorId: number;
  readonly productId: number;
  readonly productName: string;
  readonly reportDescriptor: readonly number[];
}

export interface NativeInputReport {
  readonly deviceKey: string;
  readonly data: readonly number[];
}

export interface NativeHidTransport {
  listDevices(filters: readonly HIDDeviceFilter[]): Promise<NativeDeviceInfo[]>;
  open(deviceKey: string): Promise<void>;
  close(deviceKey: string): Promise<void>;
  sendOutputReport(deviceKey: string, reportId: number, data: readonly number[]): Promise<void>;
  sendFeatureReport(deviceKey: string, reportId: number, data: readonly number[]): Promise<void>;
  receiveFeatureReport(deviceKey: string, reportId: number, byteLength: number): Promise<number[]>;
  onInputReport(listener: (report: NativeInputReport) => void): void;
  onDevicesChanged(listener: () => void): void;
}

const InputReportEvent = "hid-input-report";
const DevicesChangedEvent = "hid-devices-changed";

export const tauriHidTransport: NativeHidTransport = {
  listDevices: (filters) => invoke("list_hid_devices", { filters }),
  open: (deviceKey) => invoke("open_hid_device", { deviceKey }),
  close: (deviceKey) => invoke("close_hid_device", { deviceKey }),
  sendOutputReport: (deviceKey, reportId, data) => invoke("send_output_report", { deviceKey, reportId, data }),
  sendFeatureReport: (deviceKey, reportId, data) => invoke("send_feature_report", { deviceKey, reportId, data }),
  receiveFeatureReport: (deviceKey, reportId, byteLength) => {
    return invoke("receive_feature_report", { deviceKey, reportId, byteLength });
  },
  onInputReport: (listener) => {
    void listen<NativeInputReport>(InputReportEvent, (event) => listener(event.payload));
  },
  onDevicesChanged: (listener) => {
    void listen(DevicesChangedEvent, () => listener());
  },
};
