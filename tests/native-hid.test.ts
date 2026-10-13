import { keyboardRequestFilters } from "@openkeyboard/drivers/keyboard-driver-registry";
import { razerKeyboardDriver } from "@openkeyboard/drivers/razer/razer-keyboard-driver";
import { FakeRazerKeyboard } from "@openkeyboard-tests/fakes/fake-razer-keyboard";
import { describe, expect, it } from "vitest";
import { NativeHid } from "../src/native-hid/native-hid";
import type { NativeDeviceInfo, NativeHidTransport, NativeInputReport } from "../src/native-hid/native-hid-transport";

const RAZER_CONTROL_DESCRIPTOR = [0x05, 0x0c, 0x09, 0x01, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x5a, 0xb1, 0x03, 0xc0];
const NUMBERED_VENDOR_DESCRIPTOR = [
  0x06, 0x55, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x01, 0x75, 0x08, 0x95, 0x1f, 0x81, 0x02, 0xc0,
];
const RAZER_KEY = "razer-control";
const noAuthorizedDevices = async (): Promise<HIDDevice[]> => [];

class FakeTransport implements NativeHidTransport {
  readonly opened: string[] = [];
  readonly closed: string[] = [];
  requestedFilters: readonly HIDDeviceFilter[] = [];
  private inputListener: (report: NativeInputReport) => void = () => undefined;
  private devicesChangedListener: () => void = () => undefined;

  constructor(
    public devices: NativeDeviceInfo[],
    private readonly keyboards: ReadonlyMap<string, FakeRazerKeyboard> = new Map(),
  ) {}

  async listDevices(filters: readonly HIDDeviceFilter[]): Promise<NativeDeviceInfo[]> {
    this.requestedFilters = filters;
    return this.devices;
  }

  async open(deviceKey: string): Promise<void> {
    this.opened.push(deviceKey);
  }

  async close(deviceKey: string): Promise<void> {
    this.closed.push(deviceKey);
  }

  async sendOutputReport(): Promise<void> {}

  async sendFeatureReport(deviceKey: string, reportId: number, data: readonly number[]): Promise<void> {
    await this.keyboards.get(deviceKey)?.sendFeatureReport(reportId, Uint8Array.from(data));
  }

  async receiveFeatureReport(deviceKey: string, reportId: number): Promise<number[]> {
    const view = await this.keyboards.get(deviceKey)?.receiveFeatureReport(reportId);
    return view ? [...new Uint8Array(view.buffer, view.byteOffset, view.byteLength)] : [];
  }

  onInputReport(listener: (report: NativeInputReport) => void): void {
    this.inputListener = listener;
  }

  onDevicesChanged(listener: () => void): void {
    this.devicesChangedListener = listener;
  }

  pushInputReport(report: NativeInputReport): void {
    this.inputListener(report);
  }

  announceDevicesChanged(): void {
    this.devicesChangedListener();
  }
}

function razerInfo(deviceKey = RAZER_KEY, productId = 0x0258): NativeDeviceInfo {
  return {
    deviceKey,
    vendorId: 0x1532,
    productId,
    productName: "Razer BlackWidow V3 Mini",
    reportDescriptor: RAZER_CONTROL_DESCRIPTOR,
  };
}

function vendorInfo(): NativeDeviceInfo {
  return {
    deviceKey: "wooting-config",
    vendorId: 0x31e3,
    productId: 0x1402,
    productName: "Wooting 80HE TenZ",
    reportDescriptor: NUMBERED_VENDOR_DESCRIPTOR,
  };
}

describe("NativeHid", () => {
  it("asks the native side only for devices the drivers support", async () => {
    const transport = new FakeTransport([]);
    const filters = keyboardRequestFilters();

    await new NativeHid(transport, filters).getDevices();

    expect(transport.requestedFilters).toEqual(filters);
  });

  it("runs the Razer driver on the control interface that Chrome hides", async () => {
    const keyboard = new FakeRazerKeyboard();
    const transport = new FakeTransport([razerInfo()], new Map([[RAZER_KEY, keyboard]]));
    const [device] = await new NativeHid(transport, keyboardRequestFilters()).getDevices();
    const session = await razerKeyboardDriver.open(device, noAuthorizedDevices);

    await session.lightingEditor?.updateLighting({ brightnessPercent: 40 });

    await expect(session.readIdentity()).resolves.toMatchObject({ vendorName: "Razer", firmwareVersion: "2.3" });
    expect(keyboard.state.brightness).toBe(0x66);
    expect(transport.opened).toEqual([RAZER_KEY]);
  });

  it("returns the same device object every time", async () => {
    const hid = new NativeHid(new FakeTransport([razerInfo()]), []);

    const [first] = await hid.getDevices();
    const [second] = await hid.requestDevice();

    expect(second).toBe(first);
  });

  it("splits the report id off input reports of numbered collections", async () => {
    const transport = new FakeTransport([vendorInfo()]);
    const [device] = await new NativeHid(transport, []).getDevices();
    await device.open();
    const received: Array<[number, number[]]> = [];
    device.addEventListener("inputreport", (event) => {
      received.push([event.reportId, [...new Uint8Array(event.data.buffer)]]);
    });

    transport.pushInputReport({ deviceKey: "wooting-config", data: [0x01, 0xd1, 0xda, 0x01] });

    expect(received).toEqual([[1, [0xd1, 0xda, 0x01]]]);
  });

  it("delivers nothing to a closed device", async () => {
    const transport = new FakeTransport([vendorInfo()]);
    const [device] = await new NativeHid(transport, []).getDevices();
    const received: number[] = [];
    device.addEventListener("inputreport", (event) => received.push(event.reportId));

    transport.pushInputReport({ deviceKey: "wooting-config", data: [0x01, 0x00] });

    expect(received).toEqual([]);
  });

  it("announces devices that appear and disappear", async () => {
    const transport = new FakeTransport([razerInfo()]);
    const hid = new NativeHid(transport, []);
    const [wired] = await hid.getDevices();
    const events: string[] = [];
    hid.addEventListener("connect", (event) => events.push(`connect ${event.device.productId.toString(16)}`));
    hid.addEventListener("disconnect", (event) => events.push(`disconnect ${event.device.productId.toString(16)}`));

    transport.devices = [razerInfo("razer-receiver", 0x0271)];
    transport.announceDevicesChanged();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(events).toEqual(["disconnect 258", "connect 271"]);
    expect(wired.opened).toBe(false);
  });
});
