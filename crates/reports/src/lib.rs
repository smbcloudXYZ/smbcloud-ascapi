//! Sales report downloads from App Store Connect.
//!
//! Apple returns these reports as gzip-compressed TSV rather than JSON:API.
//! This crate owns the request shape; consumers decide how to persist or
//! transform the downloaded file.

use async_trait::async_trait;
use reqwest::Method;
use smbcloud_ascapi_core::{Client, Result};

/// Arguments accepted by `GET /v1/salesReports`.
#[derive(Debug, Clone)]
pub struct SalesReportRequest {
    pub vendor_number: String,
    pub frequency: String,
    pub report_date: String,
    pub report_type: String,
    pub report_subtype: String,
    pub version: Option<String>,
}

impl SalesReportRequest {
    /// Build the documented App Store Connect query filters.
    pub fn query(&self) -> Vec<(&str, &str)> {
        let mut query = vec![
            ("filter[frequency]", self.frequency.as_str()),
            ("filter[reportDate]", self.report_date.as_str()),
            ("filter[reportSubType]", self.report_subtype.as_str()),
            ("filter[reportType]", self.report_type.as_str()),
            ("filter[vendorNumber]", self.vendor_number.as_str()),
        ];
        if let Some(version) = self.version.as_deref() {
            query.push(("filter[version]", version));
        }
        query
    }
}

/// Download account-level sales data. The successful body is Apple’s
/// gzip-compressed TSV report.
#[async_trait]
pub trait SalesReportsApi {
    async fn download_sales_report(&self, request: &SalesReportRequest) -> Result<Vec<u8>>;
}

#[async_trait]
impl SalesReportsApi for Client {
    async fn download_sales_report(&self, request: &SalesReportRequest) -> Result<Vec<u8>> {
        self.request_bytes(
            Method::GET,
            "/v1/salesReports",
            &request.query(),
            None::<&()>,
            &[("Accept", "application/a-gzip")],
        )
        .await
    }
}

/// Traits to import when using this crate.
pub mod prelude {
    pub use crate::SalesReportsApi;
}

#[cfg(test)]
mod tests {
    use super::SalesReportRequest;

    #[test]
    fn query_contains_defaults_and_optional_version() {
        let request = SalesReportRequest {
            vendor_number: "12345678".into(),
            frequency: "DAILY".into(),
            report_date: "2026-09-11".into(),
            report_type: "SALES".into(),
            report_subtype: "SUMMARY".into(),
            version: Some("1_0".into()),
        };

        assert_eq!(
            request.query(),
            vec![
                ("filter[frequency]", "DAILY"),
                ("filter[reportDate]", "2026-09-11"),
                ("filter[reportSubType]", "SUMMARY"),
                ("filter[reportType]", "SALES"),
                ("filter[vendorNumber]", "12345678"),
                ("filter[version]", "1_0"),
            ]
        );
    }
}
