//! Native EquipmentLayerRenderer color inputs, through the production shared decision.
#![forbid(unsafe_code)]
use horizon_client::equipment_color::{Dyeable, layer_color};
use std::{fs, io::Write, path::PathBuf};

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<PathBuf> = crate::args_os().skip(1).map(PathBuf::from).collect();
    let [output] = args.as_slice() else {
        return Err("usage: equipment_color_fixture OUTPUT".into());
    };
    let mut out = fs::File::create(output)?;
    for (mode, dyeable) in [
        ("plain", None),
        (
            "only_dyed",
            Some(Dyeable {
                color_when_undyed: None,
            }),
        ),
        (
            "black",
            Some(Dyeable {
                color_when_undyed: Some(0),
            }),
        ),
        (
            "brown",
            Some(Dyeable {
                color_when_undyed: Some(0xA06540),
            }),
        ),
        (
            "white",
            Some(Dyeable {
                color_when_undyed: Some(0xFFFFFF),
            }),
        ),
    ] {
        for dyed in [0u32, 0xFF000000, 0xFF123456] {
            let color = layer_color(dyeable, (dyed != 0).then_some(dyed & 0xFFFFFF));
            writeln!(
                out,
                "{}",
                serde_json::json!({"mode":mode,"dyed":dyed,"color":color,"draw":color != 0})
            )?;
        }
    }
    Ok(())
}
