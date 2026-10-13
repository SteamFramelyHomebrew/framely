//! Physical panel orientation is distinct from OpenVR's parallel render cameras.
//! Steam Frame exposes the former in its per-device factory calibration.

use anyhow::{ensure, Context, Result};

#[derive(Clone, Copy)]
pub struct PanelGeometry {
    eye_to_head: [[f64; 3]; 3],
}

impl PanelGeometry {
    pub fn load(eye: usize) -> Result<Self> {
        let path = "/persist/device_config.json";
        ensure!(std::fs::metadata(path)?.len() <= 1024 * 1024, "oversized panel calibration");
        Self::parse(&std::fs::read_to_string(path)?, eye)
    }

    fn parse(json: &str, eye: usize) -> Result<Self> {
        let config: serde_json::Value = serde_json::from_str(json)?;
        // Factory JSON retains the internal model name on retail devices.
        ensure!(matches!(config["model_number"].as_str(), Some("Steam Frame" | "Deckard MP")), "not Steam Frame calibration");
        let matrix = &config["tracking_to_eye_transform"][eye]["eye_to_head"];
        let mut rotation = [[0.; 3]; 3];
        for (i, row) in rotation.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                let raw = matrix[i][j].as_f64().context("missing eye rotation")?;
                ensure!(raw.is_finite(), "non-finite eye rotation");
                // The shader already rotates each physical panel by 180°.
                *value = raw * if j < 2 { -1. } else { 1. };
            }
        }
        for i in 0..3 {
            for j in 0..3 {
                let dot: f64 = (0..3).map(|k| rotation[i][k] * rotation[j][k]).sum();
                ensure!((dot - if i == j { 1. } else { 0. }).abs() < 0.01, "invalid eye rotation");
            }
        }
        let determinant = rotation[0][0] * (rotation[1][1] * rotation[2][2] - rotation[1][2] * rotation[2][1])
            - rotation[0][1] * (rotation[1][0] * rotation[2][2] - rotation[1][2] * rotation[2][0])
            + rotation[0][2] * (rotation[1][0] * rotation[2][1] - rotation[1][1] * rotation[2][0]);
        ensure!((determinant - 1.).abs() < 0.01 && rotation[2][2] > 0.5, "invalid panel basis");
        Ok(Self { eye_to_head: rotation })
    }

    pub fn tangents(&self, tx: f64, ty: f64) -> [f64; 2] {
        // Head ray -> physical eye ray, using the transpose of its rotation.
        // OpenVR looks along -Z, while image Y increases downwards.
        let ray = [tx, -ty, -1.];
        let q: [f64; 3] = std::array::from_fn(|j| {
            (0..3).map(|i| self.eye_to_head[i][j] * ray[i]).sum()
        });
        if q[2] >= -1e-6 {
            return [f64::NAN; 2];
        }
        [q[0] / -q[2], q[1] / q[2]]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(matrix: serde_json::Value) -> String {
        serde_json::json!({"model_number":"Steam Frame", "tracking_to_eye_transform":[{"eye_to_head":matrix}]}).to_string()
    }

    #[test]
    fn panel_geometry_removes_scanout_rotation() {
        let p = PanelGeometry::parse(&config(serde_json::json!([[-1,0,0],[0,-1,0],[0,0,1]])), 0).unwrap();
        assert_eq!(p.tangents(0.2, -0.3), [0.2, -0.3]);
    }

    #[test]
    fn panel_geometry_preserves_factory_roll_direction() {
        let angle: f64 = -10_f64.to_radians();
        let (s,c) = angle.sin_cos();
        let p = PanelGeometry::parse(&config(serde_json::json!([[-c,s,0],[-s,-c,0],[0,0,1]])), 0).unwrap();
        let t = p.tangents(0.5, 0.);
        assert!((t[0] - 0.5*c).abs() < 1e-12);
        assert!((t[1] - 0.5*s).abs() < 1e-12);
    }

    #[test]
    fn panel_geometry_rejects_missing_and_invalid_calibration() {
        for matrix in [serde_json::json!(null), serde_json::json!([[0,0,0],[0,0,0],[0,0,0]]), serde_json::json!([[1,0,0],[0,-1,0],[0,0,1]])] {
            assert!(PanelGeometry::parse(&config(matrix), 0).is_err());
        }
        assert!(PanelGeometry::parse("{}", 0).is_err());
    }

    #[test]
    fn panel_geometry_accepts_retail_internal_model_name() {
        let json = config(serde_json::json!([[-1,0,0],[0,-1,0],[0,0,1]]));
        assert!(PanelGeometry::parse(&json.replace("Steam Frame", "Deckard MP"), 0).is_ok());
        assert!(PanelGeometry::parse(&json.replace("Steam Frame", "Other HMD"), 0).is_err());
    }

    #[test]
    fn panel_geometry_device_fixture() {
        let Ok(path) = std::env::var("FRAMELY_PANEL_TEST_FIXTURE") else { return };
        let json = std::fs::read_to_string(path).unwrap();
        for eye in 0..2 {
            let geometry = PanelGeometry::parse(&json, eye).unwrap();
            let ray = geometry.tangents(0.25, 0.25);
            assert!(ray.into_iter().all(f64::is_finite));
        }
    }
}
