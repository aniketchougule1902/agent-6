use crate::types::{FeatureSnapshot, TimeframeAnalysis};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
use tracing::info;

#[allow(dead_code)]

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub schema_version: u32,
    pub model_family: String,
    pub feature_schema_version: String,
    pub training_data_id: String,
    pub code_revision: String,
    pub created_at_ms: u64,
    pub artifact_sha256: String,
    pub metrics: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureConfig {
    pub name: String,
    pub mean: f64,
    pub scale: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationConfig {
    pub method: String,
    pub a: f64,
    pub b: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbstentionConfig {
    pub threshold: f64,
    pub min_coverage: f64,
    pub validation_utility: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPayload {
    pub model_version: String,
    pub intercept: f64,
    pub features: Vec<FeatureConfig>,
    pub calibration: CalibrationConfig,
    pub abstention: AbstentionConfig,
    pub metrics: HashMap<String, f64>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactBundle {
    pub manifest: ModelManifest,
    pub model: ModelPayload,
}

#[derive(Debug, Clone)]
pub struct ModelPrediction {
    pub calibrated_probability: f64,
    pub raw_score: f64,
    pub abstain: bool,
    pub threshold: f64,
    pub brier: f64,
    pub ece: f64,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct ModelEvaluator {
    pub manifest: ModelManifest,
    pub model: ModelPayload,
}

impl ModelEvaluator {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())?;
        Self::load_from_json(&content)
    }

    pub fn load_from_json(json_str: &str) -> anyhow::Result<Self> {
        let root: serde_json::Value = serde_json::from_str(json_str)?;
        let manifest_val = root.get("manifest").ok_or_else(|| anyhow::anyhow!("missing manifest"))?;
        let model_val = root.get("model").ok_or_else(|| anyhow::anyhow!("missing model"))?;

        let manifest: ModelManifest = serde_json::from_value(manifest_val.clone())?;

        // Fail-closed SHA-256 integrity verification
        let model: ModelPayload = if let Some(raw) = root.get("model_raw").and_then(|v| v.as_str()) {
            let digest = sha256_hex(raw.as_bytes());
            if digest != manifest.artifact_sha256 {
                return Err(anyhow::anyhow!(
                    "Model artifact integrity check failed: digest {} does not match manifest {}",
                    digest,
                    manifest.artifact_sha256
                ));
            }
            serde_json::from_str(raw)?
        } else {
            let canonical_model_json = canonical_json(model_val);
            let digest = sha256_hex(canonical_model_json.as_bytes());
            if digest != manifest.artifact_sha256 {
                return Err(anyhow::anyhow!(
                    "Model artifact integrity check failed: digest {} does not match manifest {}",
                    digest,
                    manifest.artifact_sha256
                ));
            }
            serde_json::from_value(model_val.clone())?
        };

        info!(
            version = %model.model_version,
            features = model.features.len(),
            auc = model.metrics.get("auc").copied().unwrap_or(0.0),
            brier = model.metrics.get("brier").copied().unwrap_or(0.0),
            ece = model.metrics.get("ece").copied().unwrap_or(0.0),
            "Loaded and verified calibrated champion ML model"
        );

        Ok(Self { manifest, model })
    }

    pub fn deployment_allowed(&self)->bool {
        let m=&self.model.metrics;
        self.manifest.schema_version == 1
        && self.manifest.created_at_ms <= crate::state::now_ms()
        && self.model.calibration.method == "platt"
        && self.model.abstention.threshold.is_finite()
        && self.model.abstention.threshold > 0.0 && self.model.abstention.threshold < 1.0
        && self.model.features.iter().all(|f| f.mean.is_finite() && f.scale.is_finite() && f.scale > 0.0)
        && self.manifest.feature_schema_version=="a6.live.signals.v3" && self.manifest.training_data_id.starts_with("live-journal:")
        && self.manifest.code_revision=="a6-live-v3"
        && crate::state::now_ms().saturating_sub(self.manifest.created_at_ms)<30*86400_000
        && m.get("independent_test_samples").is_some_and(|v|v.is_finite()&&*v>=100.0)
        && m.get("ece").is_some_and(|v|v.is_finite()&&*v>=0.0&&*v<=0.08)
        && m.get("brier").zip(m.get("baseline_brier")).is_some_and(|(b,base)|b.is_finite()&&base.is_finite()&&*base<=1.0&&*b>=0.0&&b<base)
        && self.model.features.len()==1 && self.model.features[0].name=="quality_score"
        && [self.model.intercept,self.model.calibration.a,self.model.calibration.b,self.model.features[0].weight].iter().all(|v|v.is_finite())
    }
    pub fn predict(&self, f: &FeatureSnapshot, a: &TimeframeAnalysis) -> ModelPrediction {
        let price = f.last_price.max(1e-9);
        let mut linear_score = self.model.intercept;

        for feat in &self.model.features {
            let val = match feat.name.as_str() {
                "quality_score" => a.quality,
                "adx14" => a.adx14,
                "rsi14" => a.rsi14,
                "atr_bps" => (a.atr14 / price) * 10_000.0,
                "macd_hist_bps" => (a.macd_histogram / price) * 10_000.0,
                "ema9_21_bps" => ((a.ema9 - a.ema21) / price) * 10_000.0,
                "ema21_50_bps" => ((a.ema21 - a.ema50) / price) * 10_000.0,
                "vwap_dev_bps" => ((price - a.vwap20) / price) * 10_000.0,
                "relative_volume" => a.relative_volume,
                "momentum_5_bps" => f.momentum_1m_bps,
                "directional_alignment" => {
                    if (a.ema9 >= a.ema21) == (price >= a.vwap20) {
                        1.0
                    } else {
                        -1.0
                    }
                }
                "imbalance_proxy" => f.book_imbalance.clamp(-1.0, 1.0),
                _ => 0.0,
            };

            let z = if feat.scale.abs() > 1e-9 {
                (val - feat.mean) / feat.scale
            } else {
                0.0
            };
            linear_score += feat.weight * z;
        }

        // Logistic / sigmoid
        let logit = linear_score.clamp(-15.0, 15.0);

        // Platt calibration: P = 1 / (1 + exp(-(A * logit + B)))
        let cal_arg = self.model.calibration.a * logit + self.model.calibration.b;
        let calibrated_prob = 1.0 / (1.0 + (-cal_arg.clamp(-20.0, 20.0)).exp());

        let threshold = self.model.abstention.threshold;
        let abstain = calibrated_prob < threshold;
        let brier = self.model.metrics.get("brier").copied().unwrap_or(0.0);
        let ece = self.model.metrics.get("ece").copied().unwrap_or(0.0);

        ModelPrediction {
            calibrated_probability: calibrated_prob,
            raw_score: linear_score,
            abstain,
            threshold,
            brier,
            ece,
            version: self.model.model_version.clone(),
        }
    }
}

/// Recursively produces canonical JSON with sorted keys and compact separators.
fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort();
            let mut s = String::from("{");
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&serde_json::to_string(k).unwrap_or_default());
                s.push(':');
                s.push_str(&canonical_json(&map[k]));
            }
            s.push('}');
            s
        }
        serde_json::Value::Array(arr) => {
            let mut s = String::from("[");
            for (i, v) in arr.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&canonical_json(v));
            }
            s.push(']');
            s
        }
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// Self-contained FIPS 180-4 SHA-256 implementation with zero external dependencies.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut h_var = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h_var.wrapping_add(s1).wrapping_add(ch).wrapping_add(k[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h_var = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(h_var);
    }

    let mut out = String::with_capacity(64);
    for val in h {
        out.push_str(&format!("{:08x}", val));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deployment_rejects_invalid_configuration_even_with_good_metrics() {
        let mut m = ModelEvaluator {
            manifest: ModelManifest { schema_version: 1, model_family: "platt_live_signal_outcome".into(), feature_schema_version: "a6.live.signals.v3".into(), training_data_id: "live-journal:test".into(), code_revision: "a6-live-v3".into(), created_at_ms: crate::state::now_ms(), artifact_sha256: String::new(), metrics: HashMap::new() },
            model: ModelPayload { model_version: "test".into(), intercept: 0.0, features: vec![FeatureConfig { name: "quality_score".into(), mean: 0.0, scale: 1.0, weight: 1.0 }], calibration: CalibrationConfig { method: "platt".into(), a: 1.0, b: 0.0 }, abstention: AbstentionConfig { threshold: 0.5, min_coverage: 0.0, validation_utility: 0.0 }, metrics: HashMap::from([("independent_test_samples".into(), 120.0), ("ece".into(), 0.04), ("brier".into(), 0.15), ("baseline_brier".into(), 0.25)]) }
        };
        assert!(m.deployment_allowed());
        for threshold in [f64::NAN, -1.0, 0.0, 1.0, f64::INFINITY] {
            m.model.abstention.threshold = threshold;
            assert!(!m.deployment_allowed());
        }
        m.model.abstention.threshold = 0.5;
        m.model.features[0].scale = 0.0;
        assert!(!m.deployment_allowed());
        m.model.features[0].scale = 1.0;
        m.manifest.created_at_ms += 86400_000;
        assert!(!m.deployment_allowed());
        m.manifest.created_at_ms = crate::state::now_ms();
        m.model.metrics.insert("baseline_brier".into(), f64::INFINITY);
        assert!(!m.deployment_allowed());
    }

    #[test]
    fn test_sha256_known_vectors() {
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn test_load_real_champion_model() {
        let manifest_path = Path::new("../../models/champion_model.json");
        let fallback_path = Path::new("models/champion_model.json");
        let target_path = if manifest_path.exists() { manifest_path } else { fallback_path };

        if target_path.exists() {
            let evaluator = ModelEvaluator::load_from_file(target_path).expect("failed to load model");
            assert_eq!(evaluator.model.model_version, "champion-v1");
            assert!(!evaluator.model.features.is_empty());
        }
    }

    #[test]
    fn test_tampered_model_fails_integrity() {
        let raw = r#"{
            "manifest": {
                "schema_version": 1,
                "model_family": "test",
                "feature_schema_version": "v1",
                "training_data_id": "test_id",
                "code_revision": "test",
                "created_at_ms": 100,
                "artifact_sha256": "0000000000000000000000000000000000000000000000000000000000000000",
                "metrics": {}
            },
            "model": {
                "model_version": "test-v1",
                "intercept": 0.0,
                "features": [],
                "calibration": { "method": "platt", "a": 1.0, "b": 0.0 },
                "abstention": { "threshold": 0.5, "min_coverage": 0.1, "validation_utility": 0.0 },
                "metrics": {}
            }
        }"#;

        let res = ModelEvaluator::load_from_json(raw);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("integrity check failed"));
    }
}

pub fn calibration_status()->serde_json::Value {
    std::fs::read("data/calibration-status.json").ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_else(||serde_json::json!({"state":"starting","reason":"Calibration worker not yet completed"}))
}
pub async fn watch(state:crate::state::AppState){
 loop {
  let python=std::env::var("A6_PYTHON").unwrap_or_else(|_|"python".into());
  let result=tokio::time::timeout(std::time::Duration::from_secs(120),tokio::process::Command::new(python).arg("scripts/calibrate_signals.py").env("A6_MODEL_PATH",&state.config.model_path).env("A6_JOURNAL_PATH",&state.config.journal_path).kill_on_drop(true).output()).await;
  if !matches!(result,Ok(Ok(ref out)) if out.status.success()){
   tracing::warn!("Calibration worker unavailable; inspect Python setup");
   let _=std::fs::write("data/calibration-status.json",serde_json::json!({"state":"unavailable","reason":"Calibration job failed. Check Python and research dependencies.","updated_ms":crate::state::now_ms()}).to_string());
  }
  state.inner.write().model=ModelEvaluator::load_from_file(&state.config.model_path).ok().filter(|m|m.deployment_allowed());
  tokio::time::sleep(std::time::Duration::from_secs(300)).await;
 }
}
