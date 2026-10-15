use std::collections::HashMap;
use std::time::SystemTime;

use hidapi::HidApi;

use crate::capture_import::{DecodedCapture, captures_for_scope, decode_usbpcap_capture, import_capture_file};
use crate::device_report::{CaptureSource, DeviceReport, ReportContext, ReportRequest, build_device_report};
use crate::device_selection::select_devices;
use crate::environment::current_environment;
use crate::hid_inventory::enumerate_hid_collections;
use crate::hid_traffic::{CaptureTimeline, TrafficCapture, TrafficScope, vendor_defined_interfaces};
use crate::hid_transport::open_hidapi_transport;
use crate::physical_devices::{DiscoveredInterface, PhysicalDevice, group_into_physical_devices};
use crate::read_only_queries::{HARDWARE_TIMING, query_physical_device};
use crate::usbpcap_live::{LIVE_CAPTURE_SOURCE, record_live_capture};
use crate::vendor_software::installed_peripheral_software;
use crate::windows_devices::{WindowsDeviceLookup, look_up_windows_devices};

pub fn collect_device_report(request: ReportRequest) -> Result<DeviceReport, String> {
    let api = HidApi::new().map_err(|error| format!("Could not start hidapi: {error}"))?;
    let mut selected_devices = select_devices(discover_physical_devices(&api), request.selection);
    if request.read_versions {
        let open_transport =
            |collection: &_, numbered_input_reports| open_hidapi_transport(&api, collection, numbered_input_reports);
        for device in &mut selected_devices {
            query_physical_device(device, &open_transport, HARDWARE_TIMING);
        }
    }
    let traffic_captures = collect_traffic(&request, &selected_devices)?;
    let context = ReportContext {
        request,
        environment: current_environment(),
        vendor_software: installed_peripheral_software(),
        traffic_captures,
        created_at: SystemTime::now(),
    };
    Ok(build_device_report(&selected_devices, context))
}

fn discover_physical_devices(api: &HidApi) -> Vec<PhysicalDevice> {
    let mut lookups_by_path: HashMap<String, WindowsDeviceLookup> = HashMap::new();
    let interfaces = enumerate_hid_collections(api)
        .into_iter()
        .map(|collection| {
            let lookup = lookups_by_path
                .entry(collection.path.clone())
                .or_insert_with(|| look_up_windows_devices(&collection.path));
            DiscoveredInterface {
                windows_devices: lookup.chain.clone(),
                container_key: lookup.container_key.clone(),
                queries: Vec::new(),
                collection,
            }
        })
        .collect();
    group_into_physical_devices(interfaces)
}

fn collect_traffic(
    request: &ReportRequest,
    selected_devices: &[PhysicalDevice],
) -> Result<Vec<TrafficCapture>, String> {
    let Some(vendor_id) = request.selection.vendor_id else {
        return match request.capture {
            CaptureSource::None => Ok(Vec::new()),
            _ => Err("Name the keyboard with --vendor-id so that only its traffic is recorded.".to_string()),
        };
    };
    let scope = TrafficScope {
        vendor_id,
        product_id: request.selection.product_id,
        vendor_defined_interfaces: vendor_defined_interfaces(selected_devices),
    };
    match &request.capture {
        CaptureSource::None => Ok(Vec::new()),
        CaptureSource::File(path) => import_capture_file(path, &scope),
        CaptureSource::LiveUsbPcap => record_live_traffic(&scope),
    }
}

fn record_live_traffic(scope: &TrafficScope) -> Result<Vec<TrafficCapture>, String> {
    let live = record_live_capture(&mut std::io::stdin().lock(), &mut std::io::stdout())?;
    let mut transfers: Vec<_> = live
        .files
        .iter()
        .filter_map(|file| decode_usbpcap_capture(file).ok())
        .flat_map(|decoded| decoded.transfers)
        .collect();
    transfers.sort_by_key(|transfer| transfer.timestamp_micros());
    let decoded = DecodedCapture { transfers, first_timestamp_micros: live.started_micros };
    let timeline =
        CaptureTimeline { source: LIVE_CAPTURE_SOURCE, started_micros: live.started_micros, marks: &live.marks };
    captures_for_scope(&decoded, scope, &timeline)
}
