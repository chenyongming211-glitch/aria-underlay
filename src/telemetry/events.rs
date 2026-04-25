#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnderlayEventKind {
    UnderlayDeviceRegistered,
    UnderlayDeviceCapabilityDetected,
    UnderlayDriftDetected,
    UnderlayDeviceLockTimeout,
    UnderlayForceUnlockRequested,
    UnderlayJournalGcCompleted,
}

