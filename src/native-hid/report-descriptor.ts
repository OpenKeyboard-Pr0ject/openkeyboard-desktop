const LONG_ITEM_PREFIX = 0xfe;
const LONG_ITEM_HEADER_BYTES = 3;
const DATA_SIZES = [0, 1, 2, 4] as const;
const BITS_PER_BYTE = 8;
const EXTENDED_USAGE_BYTES = 4;
const USAGE_PAGE_SHIFT = 16;
const USAGE_MASK = 0xffff;
const CONSTANT_FLAG = 0x01;

const ItemType = { main: 0, global: 1, local: 2 } as const;
const MainTag = { input: 0x8, output: 0x9, collection: 0xa, feature: 0xb, endCollection: 0xc } as const;
const GlobalTag = { usagePage: 0x0, reportSize: 0x7, reportId: 0x8, reportCount: 0x9, push: 0xa, pop: 0xb } as const;
const LocalTag = { usage: 0x0 } as const;

type ReportKind = "inputReports" | "outputReports" | "featureReports";

const REPORT_KIND_BY_TAG: ReadonlyMap<number, ReportKind> = new Map([
  [MainTag.input, "inputReports"],
  [MainTag.output, "outputReports"],
  [MainTag.feature, "featureReports"],
]);

interface GlobalState {
  readonly usagePage: number;
  readonly reportSize: number;
  readonly reportCount: number;
  readonly reportId: number;
}

interface DescriptorItem {
  readonly type: number;
  readonly tag: number;
  readonly value: number;
  readonly size: number;
}

interface CollectionBuilder {
  readonly usagePage: number;
  readonly usage: number;
  readonly type: number;
  readonly children: CollectionBuilder[];
  readonly reports: Record<ReportKind, Map<number, HIDReportItem[]>>;
}

export class ReportDescriptorError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ReportDescriptorError";
  }
}

export function parseReportDescriptor(descriptor: Uint8Array): HIDCollectionInfo[] {
  const roots: CollectionBuilder[] = [];
  const openCollections: CollectionBuilder[] = [];
  const globalStack: GlobalState[] = [];
  let global: GlobalState = { usagePage: 0, reportSize: 0, reportCount: 0, reportId: 0 };
  let localUsages: number[] = [];
  for (const item of descriptorItems(descriptor)) {
    if (item.type === ItemType.global) {
      if (item.tag === GlobalTag.push) globalStack.push(global);
      else if (item.tag === GlobalTag.pop) global = globalStack.pop() ?? global;
      else global = withGlobalItem(global, item);
    } else if (item.type === ItemType.local) {
      if (item.tag === LocalTag.usage) localUsages.push(extendedUsage(item, global.usagePage));
    } else if (item.type === ItemType.main) {
      applyMainItem(item, { roots, openCollections, global, firstUsage: localUsages[0] });
      localUsages = [];
    }
  }
  return roots.map(toCollectionInfo);
}

interface MainItemContext {
  readonly roots: CollectionBuilder[];
  readonly openCollections: CollectionBuilder[];
  readonly global: GlobalState;
  readonly firstUsage: number | undefined;
}

function applyMainItem(item: DescriptorItem, context: MainItemContext): void {
  const { roots, openCollections, global, firstUsage } = context;
  if (item.tag === MainTag.collection) {
    const usage = firstUsage ?? (global.usagePage << USAGE_PAGE_SHIFT);
    const collection = emptyCollection(usage >>> USAGE_PAGE_SHIFT, usage & USAGE_MASK, item.value);
    (openCollections.at(-1)?.children ?? roots).push(collection);
    openCollections.push(collection);
    return;
  }
  if (item.tag === MainTag.endCollection) {
    openCollections.pop();
    return;
  }
  const kind = REPORT_KIND_BY_TAG.get(item.tag);
  const collection = openCollections.at(-1);
  if (!kind || !collection) return;
  const items = collection.reports[kind].get(global.reportId) ?? [];
  items.push({
    reportSize: global.reportSize,
    reportCount: global.reportCount,
    isConstant: (item.value & CONSTANT_FLAG) === CONSTANT_FLAG,
  });
  collection.reports[kind].set(global.reportId, items);
}

function* descriptorItems(descriptor: Uint8Array): Generator<DescriptorItem> {
  let offset = 0;
  while (offset < descriptor.length) {
    const prefix = descriptor[offset];
    if (prefix === LONG_ITEM_PREFIX) {
      offset += LONG_ITEM_HEADER_BYTES + (descriptor[offset + 1] ?? 0);
      continue;
    }
    const size = DATA_SIZES[prefix & 0x03];
    if (offset + 1 + size > descriptor.length) {
      throw new ReportDescriptorError("The report descriptor ends in the middle of an item.");
    }
    yield { type: (prefix >> 2) & 0x03, tag: prefix >> 4, value: littleEndian(descriptor, offset + 1, size), size };
    offset += 1 + size;
  }
}

function withGlobalItem(global: GlobalState, item: DescriptorItem): GlobalState {
  switch (item.tag) {
    case GlobalTag.usagePage:
      return { ...global, usagePage: item.value };
    case GlobalTag.reportSize:
      return { ...global, reportSize: item.value };
    case GlobalTag.reportId:
      return { ...global, reportId: item.value };
    case GlobalTag.reportCount:
      return { ...global, reportCount: item.value };
    default:
      return global;
  }
}

function extendedUsage(item: DescriptorItem, usagePage: number): number {
  if (item.size === EXTENDED_USAGE_BYTES) return item.value;
  return ((usagePage << USAGE_PAGE_SHIFT) | item.value) >>> 0;
}

function littleEndian(bytes: Uint8Array, offset: number, size: number): number {
  let value = 0;
  for (let index = size - 1; index >= 0; index -= 1) value = value * 2 ** BITS_PER_BYTE + bytes[offset + index];
  return value;
}

function emptyCollection(usagePage: number, usage: number, type: number): CollectionBuilder {
  return {
    usagePage,
    usage,
    type,
    children: [],
    reports: { inputReports: new Map(), outputReports: new Map(), featureReports: new Map() },
  };
}

function toCollectionInfo(collection: CollectionBuilder): HIDCollectionInfo {
  return {
    usagePage: collection.usagePage,
    usage: collection.usage,
    type: collection.type,
    children: collection.children.map(toCollectionInfo),
    inputReports: toReports(collection.reports.inputReports),
    outputReports: toReports(collection.reports.outputReports),
    featureReports: toReports(collection.reports.featureReports),
  };
}

function toReports(reports: ReadonlyMap<number, HIDReportItem[]>): HIDReportInfo[] {
  return [...reports].map(([reportId, items]) => ({ reportId, items }));
}
