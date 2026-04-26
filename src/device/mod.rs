pub mod bootstrap;
pub mod capability;
pub mod info;
pub mod inventory;
pub mod onboarding;
pub mod registration;
pub mod render;
pub mod secret;

pub use bootstrap::{
    DeviceInitializationResult, InitializeUnderlaySiteRequest, InitializeUnderlaySiteResponse,
    SiteInitializationStatus, SwitchBootstrapRequest, UnderlaySiteInitializationService,
};
pub use capability::DeviceCapabilityProfile;
pub use info::{DeviceInfo, DeviceLifecycleState, HostKeyPolicy};
pub use inventory::DeviceInventory;
pub use onboarding::DeviceOnboardingService;
pub use registration::{DeviceRegistrationService, RegisterDeviceRequest, RegisterDeviceResponse};
pub use render::{
    renderer_for_vendor, CiscoRenderer, DeviceConfigRenderer, H3cRenderer, HuaweiRenderer,
    RenderedConfig, RenderedConfigFormat, RuijieRenderer,
};
pub use secret::{InMemorySecretStore, NetconfCredentialInput, SecretStore, StoredNetconfCredential};
