# smbcloud-ascapi-reports

Sales report downloads from App Store Connect.

Apple returns these reports as gzip-compressed TSV rather than JSON:API.
This crate owns the request shape and hands back the raw bytes; you decide
how to persist or transform them.

```toml
[dependencies]
smbcloud-ascapi-core = "0.1"
smbcloud-ascapi-reports = "0.1"
```

```rust
use smbcloud_ascapi_core::{ApiKey, Client};
use smbcloud_ascapi_reports::{SalesReportRequest, SalesReportsApi};

let client = Client::new(ApiKey::from_p8_file("KEY_ID", "ISSUER_ID", "AuthKey.p8")?);
let gzipped_tsv = client
    .download_sales_report(&SalesReportRequest {
        vendor_number: "12345678".into(),
        frequency: "DAILY".into(),
        report_date: "2026-09-11".into(),
        report_type: "SALES".into(),
        report_subtype: "SUMMARY".into(),
        version: None,
    })
    .await?;
```

See the [API docs](https://docs.rs/smbcloud-ascapi-reports).

## Copyright

© 2026 PT Sigit Mitra Bangun ([smbCloud](https://smbcloud.xyz)).
