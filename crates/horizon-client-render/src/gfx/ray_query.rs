//! Hardware ray queries, only on request (#13780, after the #1180 spike): `HORIZON_RAY_TRACING=1`
//! at startup asks the device for wgpu's `EXPERIMENTAL_RAY_QUERY` when the GPU has it. Nothing
//! draws with it yet. Without the variable the device is opened exactly as before.

/// The ray query feature.
const RAY_QUERY: wgpu::Features = wgpu::Features::EXPERIMENTAL_RAY_QUERY;

/// Whether the player asked for ray tracing (the device needs the feature from its creation).
fn requested() -> bool {
    std::env::var("HORIZON_RAY_TRACING").as_deref() == Ok("1")
}

/// The features to request on `adapter`: ray queries, when asked for and offered.
pub(super) fn features(adapter: &wgpu::Adapter) -> wgpu::Features {
    let features = pick(requested(), adapter.features());
    if requested() {
        let info = adapter.get_info();
        if features.is_empty() {
            log::warn!(
                "Horizon renderer: ray tracing asked for, but {} on {:?} has no hardware ray \
                 queries; it stays off",
                info.name,
                info.backend
            );
        } else {
            log::info!(
                "Horizon renderer: ray queries on ({} on {:?})",
                info.name,
                info.backend
            );
        }
    }
    features
}

fn pick(requested: bool, offered: wgpu::Features) -> wgpu::Features {
    if requested {
        offered & RAY_QUERY
    } else {
        wgpu::Features::empty()
    }
}

/// The device's opt-in to `features`' experimental ones.
pub(super) fn experimental(features: wgpu::Features) -> wgpu::ExperimentalFeatures {
    if features.contains(RAY_QUERY) {
        horizon_hdr::experimental_features()
    } else {
        wgpu::ExperimentalFeatures::disabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_queries_only_when_asked_and_offered() {
        let all = wgpu::Features::all();
        assert_eq!(pick(true, all), RAY_QUERY);
        assert!(pick(false, all).is_empty());
        assert!(pick(true, all - RAY_QUERY).is_empty());
        assert!(!experimental(wgpu::Features::empty()).is_enabled());
    }
}
