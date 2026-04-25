use tonic::transport::Channel;

use crate::adapter_client::mapper::{capability_from_proto, device_ref_from_info};
use crate::device::{DeviceCapabilityProfile, DeviceInfo};
use crate::proto::adapter::underlay_adapter_client::UnderlayAdapterClient;
use crate::proto::adapter::{GetCapabilitiesRequest, RequestContext};
use crate::{UnderlayError, UnderlayResult};

#[derive(Debug, Clone)]
pub struct AdapterClient {
    inner: UnderlayAdapterClient<Channel>,
}

impl AdapterClient {
    pub async fn connect(endpoint: String) -> UnderlayResult<Self> {
        let inner = UnderlayAdapterClient::connect(endpoint)
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?;
        Ok(Self { inner })
    }

    pub async fn get_capabilities(
        &mut self,
        device: &DeviceInfo,
    ) -> UnderlayResult<DeviceCapabilityProfile> {
        let request = GetCapabilitiesRequest {
            context: Some(RequestContext {
                request_id: uuid::Uuid::new_v4().to_string(),
                tx_id: String::new(),
                trace_id: uuid::Uuid::new_v4().to_string(),
                tenant_id: device.tenant_id.clone(),
                site_id: device.site_id.clone(),
            }),
            device: Some(device_ref_from_info(device)),
        };

        let response = self
            .inner
            .get_capabilities(request)
            .await
            .map_err(|err| UnderlayError::AdapterTransport(err.to_string()))?
            .into_inner();

        if let Some(error) = response.errors.into_iter().next() {
            return Err(UnderlayError::AdapterOperation {
                code: error.code,
                message: error.message,
                retryable: error.retryable,
            });
        }

        let capability = response
            .capability
            .ok_or_else(|| UnderlayError::AdapterOperation {
                code: "MISSING_CAPABILITY".into(),
                message: "adapter returned no capability".into(),
                retryable: false,
            })?;

        Ok(capability_from_proto(capability))
    }
}
