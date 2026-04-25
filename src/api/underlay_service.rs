use async_trait::async_trait;

use crate::api::force_unlock::{ForceUnlockRequest, ForceUnlockResponse};
use crate::api::request::{ApplyIntentRequest, DriftAuditRequest, RefreshStateRequest};
use crate::api::response::{
    ApplyIntentResponse, DeviceOnboardingResponse, DriftAuditResponse, DryRunResponse,
    RefreshStateResponse,
};
use crate::device::{RegisterDeviceRequest, RegisterDeviceResponse};
use crate::model::DeviceId;
use crate::state::DeviceShadowState;
use crate::tx::recovery::RecoveryReport;
use crate::UnderlayResult;

#[async_trait]
pub trait UnderlayService: Send + Sync {
    async fn register_device(
        &self,
        request: RegisterDeviceRequest,
    ) -> UnderlayResult<RegisterDeviceResponse>;

    async fn onboard_device(
        &self,
        device_id: DeviceId,
    ) -> UnderlayResult<DeviceOnboardingResponse>;

    async fn apply_intent(
        &self,
        request: ApplyIntentRequest,
    ) -> UnderlayResult<ApplyIntentResponse>;

    async fn dry_run(&self, request: ApplyIntentRequest) -> UnderlayResult<DryRunResponse>;

    async fn refresh_state(
        &self,
        request: RefreshStateRequest,
    ) -> UnderlayResult<RefreshStateResponse>;

    async fn get_device_state(&self, device_id: DeviceId) -> UnderlayResult<DeviceShadowState>;

    async fn recover_pending_transactions(&self) -> UnderlayResult<RecoveryReport>;

    async fn run_drift_audit(
        &self,
        request: DriftAuditRequest,
    ) -> UnderlayResult<DriftAuditResponse>;

    async fn force_unlock(
        &self,
        request: ForceUnlockRequest,
    ) -> UnderlayResult<ForceUnlockResponse>;
}

