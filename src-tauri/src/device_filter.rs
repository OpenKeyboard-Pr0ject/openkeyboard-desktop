use serde::Deserialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFilter {
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub usage_page: Option<u16>,
    pub usage: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumeratedCollection {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
}

impl DeviceFilter {
    pub fn matches(&self, collection: &EnumeratedCollection) -> bool {
        matches_when_set(self.vendor_id, collection.vendor_id)
            && matches_when_set(self.product_id, collection.product_id)
            && matches_when_set(self.usage_page, collection.usage_page)
            && matches_when_set(self.usage, collection.usage)
    }
}

pub fn matches_any(filters: &[DeviceFilter], collection: &EnumeratedCollection) -> bool {
    filters.iter().any(|filter| filter.matches(collection))
}

fn matches_when_set(expected: Option<u16>, actual: u16) -> bool {
    expected.is_none_or(|expected| expected == actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn razer_control() -> EnumeratedCollection {
        EnumeratedCollection { vendor_id: 0x1532, product_id: 0x0258, usage_page: 0x0c, usage: 0x01 }
    }

    #[test]
    fn matches_every_field_that_is_set() {
        let filter = DeviceFilter {
            vendor_id: Some(0x1532),
            product_id: Some(0x0258),
            usage_page: Some(0x0c),
            usage: Some(0x01),
        };

        assert!(filter.matches(&razer_control()));
    }

    #[test]
    fn treats_missing_fields_as_wildcards() {
        let via_filter = DeviceFilter { usage_page: Some(0x0c), usage: Some(0x01), ..DeviceFilter::default() };

        assert!(via_filter.matches(&razer_control()));
    }

    #[test]
    fn rejects_a_different_vendor() {
        let wooting = DeviceFilter { vendor_id: Some(0x31e3), ..DeviceFilter::default() };

        assert!(!wooting.matches(&razer_control()));
    }

    #[test]
    fn matches_nothing_without_filters() {
        assert!(!matches_any(&[], &razer_control()));
    }

    #[test]
    fn reads_web_hid_filters_in_camel_case() {
        let filter: DeviceFilter = serde_json::from_str(r#"{"vendorId":5426,"usagePage":12,"usage":1}"#).unwrap();

        assert_eq!(
            filter,
            DeviceFilter { vendor_id: Some(0x1532), product_id: None, usage_page: Some(0x0c), usage: Some(1) }
        );
    }
}
