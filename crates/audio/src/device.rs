//! Enumerate input audio devices so the web UI can populate a picker.

use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

use crate::AudioError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InputDevice {
    /// Stable identifier we hand to the UI and accept back in `start_mic`.
    /// On most platforms this is the device's reported name; if cpal gains
    /// a stable id API later we'll switch to that without changing the type.
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub default_sample_rate: u32,
    pub default_channels: u16,
}

pub fn list_input_devices() -> Result<Vec<InputDevice>, AudioError> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .as_ref()
        .and_then(|d| d.name().ok());

    let mut out = Vec::new();
    for device in host.input_devices()? {
        let name = match device.name() {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(?e, "skipping device with unreadable name");
                continue;
            }
        };
        let cfg = match device.default_input_config() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(device = %name, ?e, "skipping device with no input config");
                continue;
            }
        };
        let is_default = default_name.as_deref() == Some(name.as_str());
        out.push(InputDevice {
            id: name.clone(),
            name,
            is_default,
            default_sample_rate: cfg.sample_rate().0,
            default_channels: cfg.channels(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumeration_does_not_panic() {
        // We can't assert there are any devices (CI hosts often have none),
        // but we can assert the call itself works and the default flag is
        // mutually exclusive — at most one device should be marked default.
        let devices = list_input_devices().expect("enumeration");
        let default_count = devices.iter().filter(|d| d.is_default).count();
        assert!(
            default_count <= 1,
            "at most one device should be the default, got {default_count}"
        );
    }
}
