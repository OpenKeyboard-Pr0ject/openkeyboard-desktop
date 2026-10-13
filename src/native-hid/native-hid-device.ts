import type { NativeDeviceInfo, NativeHidTransport } from "./native-hid-transport";
import { parseReportDescriptor } from "./report-descriptor";

type InputReportListener = (event: HIDInputReportEvent) => void;

const BITS_PER_BYTE = 8;
const NO_REPORT_ID = 0;

export class NativeHidDevice {
  opened = false;
  readonly deviceKey: string;
  readonly vendorId: number;
  readonly productId: number;
  readonly productName: string;
  readonly collections: HIDCollectionInfo[];
  private readonly inputListeners = new Set<InputReportListener>();

  constructor(private readonly transport: NativeHidTransport, info: NativeDeviceInfo) {
    this.deviceKey = info.deviceKey;
    this.vendorId = info.vendorId;
    this.productId = info.productId;
    this.productName = info.productName;
    this.collections = parseReportDescriptor(Uint8Array.from(info.reportDescriptor));
  }

  async open(): Promise<void> {
    if (this.opened) return;
    await this.transport.open(this.deviceKey);
    this.opened = true;
  }

  async close(): Promise<void> {
    if (!this.opened) return;
    this.opened = false;
    await this.transport.close(this.deviceKey);
  }

  async forget(): Promise<void> {
    await this.close();
  }

  async sendReport(reportId: number, data: BufferSource): Promise<void> {
    await this.transport.sendOutputReport(this.deviceKey, reportId, toByteArray(data));
  }

  async sendFeatureReport(reportId: number, data: BufferSource): Promise<void> {
    await this.transport.sendFeatureReport(this.deviceKey, reportId, toByteArray(data));
  }

  async receiveFeatureReport(reportId: number): Promise<DataView> {
    const byteLength = this.featureReportByteLength(reportId);
    const report = await this.transport.receiveFeatureReport(this.deviceKey, reportId, byteLength);
    return new DataView(Uint8Array.from(report).buffer);
  }

  addEventListener(_type: "inputreport", listener: InputReportListener): void {
    this.inputListeners.add(listener);
  }

  removeEventListener(_type: "inputreport", listener: InputReportListener): void {
    this.inputListeners.delete(listener);
  }

  deliverInputReport(data: readonly number[]): void {
    if (!this.opened || data.length === 0) return;
    const bytes = Uint8Array.from(data);
    const usesReportIds = this.usesInputReportIds();
    const reportId = usesReportIds ? bytes[0] : NO_REPORT_ID;
    const payload = usesReportIds ? bytes.slice(1) : bytes;
    const event = { reportId, data: new DataView(payload.buffer), device: this.asHidDevice() } as HIDInputReportEvent;
    [...this.inputListeners].forEach((listener) => listener(event));
  }

  asHidDevice(): HIDDevice {
    return this as unknown as HIDDevice;
  }

  private usesInputReportIds(): boolean {
    return allCollections(this.collections)
      .flatMap((collection) => collection.inputReports ?? [])
      .some((report) => (report.reportId ?? NO_REPORT_ID) !== NO_REPORT_ID);
  }

  private featureReportByteLength(reportId: number): number {
    const bitLength = allCollections(this.collections)
      .flatMap((collection) => collection.featureReports ?? [])
      .filter((report) => (report.reportId ?? NO_REPORT_ID) === reportId)
      .flatMap((report) => report.items ?? [])
      .reduce((total, item) => total + (item.reportSize ?? 0) * (item.reportCount ?? 0), 0);
    return Math.ceil(bitLength / BITS_PER_BYTE);
  }
}

function allCollections(collections: readonly HIDCollectionInfo[]): HIDCollectionInfo[] {
  return collections.flatMap((collection) => [collection, ...allCollections(collection.children ?? [])]);
}

function toByteArray(data: BufferSource): number[] {
  const bytes = ArrayBuffer.isView(data)
    ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength)
    : new Uint8Array(data);
  return [...bytes];
}
