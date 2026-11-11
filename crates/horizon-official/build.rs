//! Gives the Windows executable Horizon's icon and its stack, as horizon-client's
//! (`packaging/windows/exe.rs`).
#[path = "../../packaging/windows/exe.rs"]
mod windows_exe;

fn main() {
    windows_exe::resources("horizon-official");
}
