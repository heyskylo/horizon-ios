//! `AnvilScreen`'s rename box: the 50-character `EditBox`, read-only while
//! the input slot is empty (`AnvilScreen.slotChanged`).
use super::Station;

/// `AnvilScreen`: `name.setMaxLength(50)`.
const NAME_CHARS: usize = 50;

/// `AnvilScreen.init`: the unbordered box of 50 at the menu's name position,
/// focused; `slotChanged` makes it editable once the input slot has an item.
pub(super) fn rename_box() -> horizon_client::ui::EditBox {
    let mut edit = horizon_client::ui::EditBox::new(0.0, 0.0, 103.0, 12.0);
    edit.message = super::super::screen_info::tr("container.repair", &[]);
    edit.bordered = false;
    edit.max_length = NAME_CHARS;
    edit.text_color = u32::MAX;
    // AnvilScreen also sets `setTextColorUneditable(-1)`: white either way.
    edit.text_color_uneditable = u32::MAX;
    edit.focused = true;
    edit.editable = false;
    edit
}

/// `AnvilScreen.slotChanged`: the box shows the input's name, and only an
/// input item makes it editable (`name.setEditable(!itemStack.isEmpty())`).
/// A read-only box still keeps the focus, the caret keys and copy.
pub(super) fn input_changed(station: &mut Station, name: String) {
    station.name = name;
    let value = station.name.clone();
    station.rename.set_value(&value);
    station.rename.editable = station.inputs[0].is_some();
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, Station};
    use super::{input_changed, rename_box};

    #[test]
    fn the_rename_box_is_read_only_without_an_input_item() {
        // AnvilScreen.init: no input yet, so the box is read-only but focused.
        let station_box = rename_box();
        assert!(station_box.focused && !station_box.editable);

        // slotChanged with an item: editable, and it shows the item's name.
        let mut station = Station::new_kind(Kind::Anvil, [0; 3]);
        station.inputs[0] = Some(crate::items::ItemStack::new("minecraft:diamond_sword", 1));
        input_changed(&mut station, "Diamond Sword".into());
        assert!(station.rename.editable);
        assert_eq!(station.rename.value, "Diamond Sword");

        // The input is taken out again: read-only, and the name stays.
        station.inputs[0] = None;
        input_changed(&mut station, "Diamond Sword".into());
        assert!(!station.rename.editable);
        assert_eq!(station.rename.value, "Diamond Sword");

        // A read-only box still keeps the caret keys; Backspace is consumed
        // without taking a character.
        assert!(
            station
                .rename
                .key(winit::keyboard::KeyCode::ArrowLeft, false, false)
        );
        assert!(
            !station
                .rename
                .key(winit::keyboard::KeyCode::Backspace, false, false)
        );
        assert_eq!(station.rename.value, "Diamond Sword");
    }
}
