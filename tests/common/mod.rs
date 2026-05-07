use async_trait::async_trait;
use aria_underlay::proto::adapter;
use aria_underlay::proto::adapter::underlay_adapter_server::{
    UnderlayAdapter, UnderlayAdapterServer,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tonic::{Request, Response, Status};

#[derive(Debug, Clone)]
pub struct TestAdapter {
    pub capability: Option<adapter::DeviceCapability>,
    pub capability_warnings: Vec<String>,
    pub current_state: Option<adapter::ObservedDeviceState>,
    pub current_warnings: Vec<String>,
    pub dry_run_result: adapter::AdapterResult,
    pub prepare_result: adapter::AdapterResult,
    pub commit_result: adapter::AdapterResult,
    pub commit_confirm_timeouts: Option<Arc<Mutex<Vec<u32>>>>,
    pub final_confirm_result: adapter::AdapterResult,
    pub rollback_result: adapter::AdapterResult,
    pub rollback_calls: Option<Arc<AtomicUsize>>,
    pub verify_result: adapter::AdapterResult,
    pub recover_result: adapter::AdapterResult,
    pub force_unlock_result: adapter::AdapterResult,
}

impl Default for TestAdapter {
    fn default() -> Self {
        Self {
            capability: Some(confirmed_commit_capability()),
            capability_warnings: Vec::new(),
            current_state: None,
            current_warnings: Vec::new(),
            dry_run_result: adapter_result(adapter::AdapterOperationStatus::NoChange),
            prepare_result: adapter_result(adapter::AdapterOperationStatus::Prepared),
            commit_result: adapter_result(
                adapter::AdapterOperationStatus::ConfirmedCommitPending,
            ),
            commit_confirm_timeouts: None,
            final_confirm_result: adapter_result(adapter::AdapterOperationStatus::Committed),
            rollback_result: adapter_result(adapter::AdapterOperationStatus::RolledBack),
            rollback_calls: None,
            verify_result: adapter_result(adapter::AdapterOperationStatus::Committed),
            recover_result: adapter_result(adapter::AdapterOperationStatus::NoChange),
            force_unlock_result: adapter_result(adapter::AdapterOperationStatus::Committed),
        }
    }
}

pub async fn start_test_adapter(adapter: TestAdapter) -> String {
    let listener =
        std::net::TcpListener::bind("127.0.0.1:0").expect("test adapter listener should bind");
    let addr = listener.local_addr().expect("test adapter addr should exist");
    drop(listener);
    start_test_adapter_at(adapter, addr).await
}

pub async fn start_test_adapter_at(
    adapter: TestAdapter,
    addr: std::net::SocketAddr,
) -> String {
    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(UnderlayAdapterServer::new(adapter))
            .serve(addr)
            .await
            .expect("test adapter server should run");
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{addr}")
}

pub fn adapter_result(status: adapter::AdapterOperationStatus) -> adapter::AdapterResult {
    adapter::AdapterResult {
        status: status as i32,
        changed: status != adapter::AdapterOperationStatus::NoChange,
        warnings: Vec::new(),
        errors: Vec::new(),
        rollback_artifact: None,
        normalized_state: None,
    }
}

pub fn failed_result(code: &str) -> adapter::AdapterResult {
    adapter::AdapterResult {
        status: adapter::AdapterOperationStatus::Failed as i32,
        changed: false,
        warnings: Vec::new(),
        errors: vec![adapter::AdapterError {
            code: code.into(),
            message: format!("{code} for test adapter"),
            normalized_error: code.into(),
            raw_error_summary: code.into(),
            retryable: false,
        }],
        rollback_artifact: None,
        normalized_state: None,
    }
}

pub fn confirmed_commit_capability() -> adapter::DeviceCapability {
    adapter::DeviceCapability {
        vendor: adapter::Vendor::Unknown as i32,
        model: "test-adapter".into(),
        os_version: "test".into(),
        raw_capabilities: Vec::new(),
        supports_netconf: true,
        supports_candidate: true,
        supports_validate: true,
        supports_confirmed_commit: true,
        supports_persist_id: true,
        supports_rollback_on_error: false,
        supports_writable_running: false,
        supported_backends: vec![adapter::BackendKind::Netconf as i32],
    }
}

pub fn observed_access_state(device_id: &str, vlan_id: u32) -> adapter::ObservedDeviceState {
    adapter::ObservedDeviceState {
        device_id: device_id.into(),
        vlans: vec![adapter::VlanConfig {
            vlan_id,
            name: Some("prod".into()),
            description: None,
        }],
        interfaces: vec![adapter::InterfaceConfig {
            name: "GE1/0/1".into(),
            admin_state: adapter::AdminState::Up as i32,
            description: None,
            mode: Some(adapter::PortMode {
                kind: adapter::PortModeKind::Access as i32,
                access_vlan: Some(vlan_id),
                native_vlan: None,
                allowed_vlans: Vec::new(),
            }),
        }],
    }
}

#[async_trait]
impl UnderlayAdapter for TestAdapter {
    async fn get_capabilities(
        &self,
        _request: Request<adapter::GetCapabilitiesRequest>,
    ) -> Result<Response<adapter::GetCapabilitiesResponse>, Status> {
        Ok(Response::new(adapter::GetCapabilitiesResponse {
            capability: self.capability.clone(),
            warnings: self.capability_warnings.clone(),
            errors: Vec::new(),
        }))
    }

    async fn get_current_state(
        &self,
        _request: Request<adapter::GetCurrentStateRequest>,
    ) -> Result<Response<adapter::GetCurrentStateResponse>, Status> {
        Ok(Response::new(adapter::GetCurrentStateResponse {
            state: self.current_state.clone(),
            warnings: self.current_warnings.clone(),
            errors: Vec::new(),
        }))
    }

    async fn dry_run(
        &self,
        _request: Request<adapter::DryRunRequest>,
    ) -> Result<Response<adapter::DryRunResponse>, Status> {
        Ok(Response::new(adapter::DryRunResponse {
            result: Some(self.dry_run_result.clone()),
        }))
    }

    async fn prepare(
        &self,
        _request: Request<adapter::PrepareRequest>,
    ) -> Result<Response<adapter::PrepareResponse>, Status> {
        Ok(Response::new(adapter::PrepareResponse {
            result: Some(self.prepare_result.clone()),
        }))
    }

    async fn commit(
        &self,
        request: Request<adapter::CommitRequest>,
    ) -> Result<Response<adapter::CommitResponse>, Status> {
        if let Some(timeouts) = &self.commit_confirm_timeouts {
            timeouts
                .lock()
                .expect("commit timeout recorder should not be poisoned")
                .push(request.into_inner().confirm_timeout_secs);
        }
        Ok(Response::new(adapter::CommitResponse {
            result: Some(self.commit_result.clone()),
        }))
    }

    async fn final_confirm(
        &self,
        _request: Request<adapter::FinalConfirmRequest>,
    ) -> Result<Response<adapter::FinalConfirmResponse>, Status> {
        Ok(Response::new(adapter::FinalConfirmResponse {
            result: Some(self.final_confirm_result.clone()),
        }))
    }

    async fn rollback(
        &self,
        _request: Request<adapter::RollbackRequest>,
    ) -> Result<Response<adapter::RollbackResponse>, Status> {
        if let Some(calls) = &self.rollback_calls {
            calls.fetch_add(1, Ordering::SeqCst);
        }
        Ok(Response::new(adapter::RollbackResponse {
            result: Some(self.rollback_result.clone()),
        }))
    }

    async fn verify(
        &self,
        _request: Request<adapter::VerifyRequest>,
    ) -> Result<Response<adapter::VerifyResponse>, Status> {
        Ok(Response::new(adapter::VerifyResponse {
            result: Some(self.verify_result.clone()),
        }))
    }

    async fn recover(
        &self,
        _request: Request<adapter::RecoverRequest>,
    ) -> Result<Response<adapter::RecoverResponse>, Status> {
        Ok(Response::new(adapter::RecoverResponse {
            result: Some(self.recover_result.clone()),
        }))
    }

    async fn force_unlock(
        &self,
        _request: Request<adapter::ForceUnlockRequest>,
    ) -> Result<Response<adapter::ForceUnlockResponse>, Status> {
        Ok(Response::new(adapter::ForceUnlockResponse {
            result: Some(self.force_unlock_result.clone()),
        }))
    }
}
