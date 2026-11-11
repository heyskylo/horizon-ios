//! `SmithingScreen`'s empty slot icons: the base and addition lists of a
//! template (`SmithingTemplateItem`'s create*IconList helpers).

/// The base and additions slot icons of the template in the smithing table:
/// `SmithingTemplateItem.createNetheriteUpgradeTemplate` (the 11 tools and
/// armour of `SmithingTemplateItem.createNetheriteUpgradeIconList`, then the
/// ingot) and `SmithingTemplateItem.createArmorTrimTemplate` (the four armour
/// pieces, then the seven trim materials), as `getBaseSlotEmptyIcons` and
/// `getAdditionalSlotEmptyIcons` give them.
pub(super) fn smithing_slot_icons(
    template: Option<&str>,
) -> ([&'static str; 11], usize, [&'static str; 7], usize) {
    let none = ([""; 11], 0, [""; 7], 0);
    match template.map(|t| t.trim_start_matches("minecraft:")) {
        Some("netherite_upgrade_smithing_template") => (
            [
                "helmet",
                "sword",
                "chestplate",
                "pickaxe",
                "leggings",
                "axe",
                "boots",
                "hoe",
                "shovel",
                "nautilus_armor",
                "spear",
            ],
            11,
            ["ingot", "", "", "", "", "", ""],
            1,
        ),
        Some(t) if t.ends_with("_armor_trim_smithing_template") => (
            [
                "helmet",
                "chestplate",
                "leggings",
                "boots",
                "",
                "",
                "",
                "",
                "",
                "",
                "",
            ],
            4,
            [
                "ingot",
                "redstone_dust",
                "lapis_lazuli",
                "quartz",
                "diamond",
                "emerald",
                "amethyst_shard",
            ],
            7,
        ),
        _ => none,
    }
}
