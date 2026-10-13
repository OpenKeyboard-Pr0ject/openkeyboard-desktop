import { NativeHidDevice } from "./native-hid-device";
import type { NativeDeviceInfo, NativeHidTransport } from "./native-hid-transport";

type ConnectionEventType = "connect" | "disconnect";
type ConnectionListener = (event: HIDConnectionEvent) => void;

export class NativeHid {
  private readonly devices = new Map<string, NativeHidDevice>();
  private readonly listeners: Record<ConnectionEventType, Set<ConnectionListener>> = {
    connect: new Set(),
    disconnect: new Set(),
  };

  constructor(
    private readonly transport: NativeHidTransport,
    private readonly filters: readonly HIDDeviceFilter[],
  ) {
    transport.onInputReport((report) => this.devices.get(report.deviceKey)?.deliverInputReport(report.data));
    transport.onDevicesChanged(() => void this.refresh(true));
  }

  getDevices(): Promise<HIDDevice[]> {
    return this.refresh(false);
  }

  requestDevice(): Promise<HIDDevice[]> {
    return this.refresh(false);
  }

  addEventListener(type: ConnectionEventType, listener: ConnectionListener): void {
    this.listeners[type].add(listener);
  }

  asHid(): HID {
    return this as unknown as HID;
  }

  private async refresh(announceNewDevices: boolean): Promise<HIDDevice[]> {
    const available = await this.transport.listDevices(this.filters);
    this.forgetUnplugged(available);
    for (const info of available) {
      if (this.devices.has(info.deviceKey)) continue;
      const device = new NativeHidDevice(this.transport, info);
      this.devices.set(info.deviceKey, device);
      if (announceNewDevices) this.emit("connect", device);
    }
    return [...this.devices.values()].map((device) => device.asHidDevice());
  }

  private forgetUnplugged(available: readonly NativeDeviceInfo[]): void {
    const availableKeys = new Set(available.map((info) => info.deviceKey));
    for (const [deviceKey, device] of [...this.devices]) {
      if (availableKeys.has(deviceKey)) continue;
      this.devices.delete(deviceKey);
      device.opened = false;
      this.emit("disconnect", device);
    }
  }

  private emit(type: ConnectionEventType, device: NativeHidDevice): void {
    const event = { device: device.asHidDevice() } as HIDConnectionEvent;
    this.listeners[type].forEach((listener) => listener(event));
  }
}
