use govee::backend::GoveeBackend;
use govee::backend::local::LocalBackend;
use govee::types::Device;

use crate::store::{Rgb, SavedDevice};

pub struct AppDevice {
    pub saved: SavedDevice,
    pub live: Option<Device>,
    pub on: bool,
    pub kelvin: Option<u32>,
    pub color: Option<Rgb>,
    pub brightness: Option<u8>,
}

impl AppDevice {
    pub fn from_saved(saved: SavedDevice) -> Self {
        Self {
            saved,
            live: None,
            on: false,
            kelvin: None,
            color: None,
            brightness: None,
        }
    }

    pub fn online(&self) -> bool {
        self.live.is_some()
    }
}

pub async fn merge(backend: &LocalBackend, devices: &mut Vec<AppDevice>, found: Vec<Device>) {
    for f in found {
        let id = f.id.to_string();
        match devices.iter_mut().find(|d| d.saved.id == id) {
            Some(d) => d.live = Some(f),
            None => devices.push(AppDevice {
                saved: SavedDevice {
                    id,
                    model: f.model.clone(),
                    name: f.model.clone(),
                },
                live: Some(f),
                on: false,
                kelvin: None,
                color: None,
                brightness: None,
            }),
        }
    }

    for d in devices.iter_mut() {
        if let Some(live) = &d.live
            && let Ok(state) = backend.get_state(&live.id).await
        {
            d.on = state.on;
            d.brightness = Some(state.brightness);
            d.kelvin = state.color_temp_kelvin;
            d.color = match d.kelvin {
                Some(_) => None,
                None => Some((state.color.r, state.color.g, state.color.b)),
            };
        }
    }
}
