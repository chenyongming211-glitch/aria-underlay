use aria_underlay::worker::drift_auditor::DriftAuditor;

#[tokio::test]
async fn drift_auditor_initially_reports_nothing() {
    let reports = DriftAuditor.run_once().await;
    assert!(reports.is_empty());
}

