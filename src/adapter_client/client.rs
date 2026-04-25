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
            .map_err(|err| UnderlayError::Adapter(err.to_string()))?;
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
            .map_err(|err| UnderlayError::Adapter(err.to_string()))?
            .into_inner();

        let capability = response
            .capability
            .ok_or_else(|| UnderlayError::Adapter("adapter returned no capability".into()))?;

        Ok(capability_from_proto(capability))
    }
}

