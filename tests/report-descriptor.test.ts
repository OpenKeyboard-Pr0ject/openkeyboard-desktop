import { featureReportByteLength } from "@openkeyboard/hid/collections";
import { describe, expect, it } from "vitest";
import { parseReportDescriptor, ReportDescriptorError } from "../src/native-hid/report-descriptor";

function bytesOf(hex: string): Uint8Array {
  return Uint8Array.from((hex.match(/../g) ?? []).map((byte) => parseInt(byte, 16)));
}

const razerControlDescriptor = bytesOf("050c0901a1017508955ab103c0");
const razerKeyboardDescriptor = bytesOf(
  "05010906a101050719e029e715002501750195088102750895018103190029ff150026ff0075089506810005081901290315002501"
  + "750195039102750595019103c0",
);
const razerNumberedKeyboardDescriptor = bytesOf(
  "05010906a1018501050719e029e7150025017501950881021900299f15002501750195a0810205081901290315002501750195039102"
  + "750595019103c0",
);
const razerMediaKeysDescriptor = bytesOf("050c0901a101850819d029d715002501750195088102c0");

function asDevice(collections: HIDCollectionInfo[]): HIDDevice {
  return { collections } as unknown as HIDDevice;
}

describe("parseReportDescriptor on descriptors read from the BlackWidow V3 Mini", () => {
  it("finds the 90-byte control report that Chrome hides, counting constant bytes", () => {
    const collections = parseReportDescriptor(razerControlDescriptor);

    expect(collections).toMatchObject([{
      usagePage: 0x0c,
      usage: 0x01,
      featureReports: [{ reportId: 0, items: [{ reportSize: 8, reportCount: 90, isConstant: true }] }],
    }]);
    expect(featureReportByteLength(asDevice(collections), 0)).toBe(90);
  });

  it("reads the boot keyboard collection with its input and output reports", () => {
    const [keyboard] = parseReportDescriptor(razerKeyboardDescriptor);

    expect([keyboard.usagePage, keyboard.usage]).toEqual([0x01, 0x06]);
    expect(keyboard.inputReports?.map((report) => report.reportId)).toEqual([0]);
    expect(keyboard.outputReports?.[0].items?.map((item) => item.reportSize! * item.reportCount!)).toEqual([3, 5]);
  });

  it("keeps the report id of a numbered collection", () => {
    const [keyboard] = parseReportDescriptor(razerNumberedKeyboardDescriptor);

    expect(keyboard.inputReports?.map((report) => report.reportId)).toEqual([1]);
  });

  it("reads a consumer collection with report id 8", () => {
    const [mediaKeys] = parseReportDescriptor(razerMediaKeysDescriptor);

    expect(mediaKeys).toMatchObject({ usagePage: 0x0c, usage: 0x01, inputReports: [{ reportId: 8 }] });
  });
});

describe("parseReportDescriptor on synthetic descriptors", () => {
  it("nests child collections and keeps reports in the collection that declares them", () => {
    const descriptor = bytesOf(
      "0600ff0901a101" + "0902a102" + "85017508951fb102" + "c0" + "85027508953f9102" + "c0",
    );

    const [vendor] = parseReportDescriptor(descriptor);

    expect([vendor.usagePage, vendor.usage]).toEqual([0xff00, 0x01]);
    expect(vendor.children?.[0].featureReports).toMatchObject([{ reportId: 1, items: [{ reportCount: 31 }] }]);
    expect(vendor.outputReports).toMatchObject([{ reportId: 2, items: [{ reportCount: 63 }] }]);
  });

  it("restores global state after push and pop", () => {
    const descriptor = bytesOf("0600ff0901a101" + "75089502" + "a4" + "75109504" + "8102" + "b4" + "8102" + "c0");

    const [collection] = parseReportDescriptor(descriptor);

    expect(collection.inputReports?.[0].items).toMatchObject([
      { reportSize: 16, reportCount: 4 },
      { reportSize: 8, reportCount: 2 },
    ]);
  });

  it("uses the usage page inside a four-byte usage", () => {
    const descriptor = bytesOf("0b010053ffa101c0");

    expect(parseReportDescriptor(descriptor)).toMatchObject([{ usagePage: 0xff53, usage: 0x01 }]);
  });

  it("rejects a descriptor that ends inside an item", () => {
    expect(() => parseReportDescriptor(bytesOf("0600"))).toThrow(ReportDescriptorError);
  });
});
