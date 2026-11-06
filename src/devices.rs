use govee::backend::GoveeBackend;
use govee::backend::local::LocalBackend;
use govee::types::Device;

use crate::store::SavedDevice;

pub struct AppDevice {
    pub saved: SavedDevice,
    pub live: Option<Device>,
    pub on: bool,
    pub marked: bool,
    pub kelvin: Option<u32>,
}

impl AppDevice {
    pub fn from_saved(saved: SavedDevice) -> Self {
        Self {
            saved,
            live: None,
            on: false,
            marked: false,
            kelvin: None,
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
                marked: false,
                kelvin: None,
            }),
        }
    }

    for d in devices.iter_mut() {
        if let Some(live) = &d.live
            && let Ok(state) = backend.get_state(&live.id).await
        {
            d.on = state.on;
            d.kelvin = state.color_temp_kelvin;
        }
    }
}
