//! Which compiler turns DirectX 12's shaders into bytecode: DXC on integrated
//! GPUs, FXC on every other adapter (#8513).
//!
//! Measured on one laptop at render distance 32 (2560x1440, the same world and
//! view): DXC took Intel Iris Xe from 44 to 65 FPS (22.1 to 14.7 ms of GPU
//! time) and an NVIDIA RTX 3050 Ti from 159 down to 150 FPS. An RX 9070 gained
//! about 9 % of GPU time at render distance 32 and lost 0.03 ms at 8. So
//! discrete GPUs keep FXC (`d3dcompiler_47.dll`, part of Windows), and only
//! integrated ones load DXC (`dxcompiler.dll`, which the Windows x64 zip puts
//! beside the game: `tools/ci/fetch_dxc.py`).
//!
//! wgpu chooses the compiler for a whole instance, before any adapter is
//! known. `open_backend` therefore asks for the adapter on an FXC instance and,
//! when that adapter is an integrated GPU on DirectX 12, asks again on an
//! instance with DXC. That happens once, when the device is opened; nothing
//! here runs per frame. Without the DLL, or when it does not load, the game
//! keeps the FXC instance and logs one line. `WGPU_DX12_COMPILER` (`fxc`,
//! `dxc`, `auto`) overrides the choice: wgpu applies it to every instance.

use std::path::PathBuf;

/// The DLL's name, as the Windows zip ships it beside `horizon-client.exe`.
pub(super) const DLL: &str = "dxcompiler.dll";

/// Whether `info`, opened on an FXC instance, should be opened again with DXC:
/// an integrated GPU on DirectX 12, unless `forced` (`WGPU_DX12_COMPILER` is
/// set, so its compiler is already the one every instance uses).
pub(super) fn wants_dxc(info: &wgpu::AdapterInfo, forced: bool) -> bool {
    !forced
        && info.backend == wgpu::Backend::Dx12
        && info.device_type == wgpu::DeviceType::IntegratedGpu
}

/// Whether `WGPU_DX12_COMPILER` names the compiler.
pub(super) fn forced() -> bool {
    std::env::var_os("WGPU_DX12_COMPILER").is_some()
}

/// `dxcompiler.dll` beside the running executable, if it is there. An absolute
/// path, so a copy elsewhere on `PATH` (an older Windows SDK's) is never the
/// one loaded.
pub(super) fn bundled_dll() -> Option<PathBuf> {
    let dll = std::env::current_exe().ok()?.parent()?.join(DLL);
    dll.is_file().then_some(dll)
}

/// The compiler setting that loads `dll`, and fails the instance (rather
/// than falling back to FXC on its own) when it doesn't load: `open_backend`
/// then keeps the FXC instance it already has.
pub(super) fn dxc(dll: &std::path::Path) -> wgpu::Dx12Compiler {
    wgpu::Dx12Compiler::DynamicDxc {
        dxc_path: dll.to_string_lossy().into_owned(),
    }
}

/// Whether the DXC instance offered the adapter the FXC instance had: on a
/// laptop with two GPUs, the same request should pick the same one, and the
/// game must not switch GPUs because the compiler changed.
pub(super) fn same_adapter(fxc: &wgpu::AdapterInfo, dxc: &wgpu::AdapterInfo) -> bool {
    fxc.backend == dxc.backend
        && fxc.vendor == dxc.vendor
        && fxc.device == dxc.device
        && fxc.device_type == dxc.device_type
        && fxc.device_pci_bus_id == dxc.device_pci_bus_id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(backend: wgpu::Backend, device_type: wgpu::DeviceType) -> wgpu::AdapterInfo {
        wgpu::AdapterInfo {
            name: "Intel(R) Iris(R) Xe Graphics".into(),
            vendor: 0x8086,
            device: 0x9a49,
            device_type,
            device_pci_bus_id: "0000:00:02.0".into(),
            driver: String::new(),
            driver_info: String::new(),
            backend,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: None,
            limit_bucket: None,
        }
    }

    #[test]
    fn only_integrated_gpus_on_directx_12_get_dxc() {
        use wgpu::{Backend, DeviceType};
        assert!(wants_dxc(
            &adapter(Backend::Dx12, DeviceType::IntegratedGpu),
            false
        ));
        for kind in [
            DeviceType::DiscreteGpu,
            DeviceType::VirtualGpu,
            DeviceType::Cpu,
            DeviceType::Other,
        ] {
            assert!(!wants_dxc(&adapter(Backend::Dx12, kind), false), "{kind:?}");
        }
        // Other backends have no shader compiler to choose.
        for backend in [Backend::Vulkan, Backend::Gl, Backend::Metal] {
            assert!(
                !wants_dxc(&adapter(backend, DeviceType::IntegratedGpu), false),
                "{backend:?}"
            );
        }
    }

    #[test]
    fn wgpu_dx12_compiler_overrides_the_choice() {
        let igpu = adapter(wgpu::Backend::Dx12, wgpu::DeviceType::IntegratedGpu);
        assert!(!wants_dxc(&igpu, true));
    }

    #[test]
    fn the_dxc_instance_must_offer_the_same_gpu() {
        let igpu = adapter(wgpu::Backend::Dx12, wgpu::DeviceType::IntegratedGpu);
        assert!(same_adapter(&igpu, &igpu.clone()));
        let dgpu = wgpu::AdapterInfo {
            name: "NVIDIA GeForce RTX 3050 Ti Laptop GPU".into(),
            vendor: 0x10de,
            device: 0x25a0,
            device_type: wgpu::DeviceType::DiscreteGpu,
            device_pci_bus_id: "0000:01:00.0".into(),
            ..igpu.clone()
        };
        assert!(!same_adapter(&igpu, &dgpu));
    }
}
