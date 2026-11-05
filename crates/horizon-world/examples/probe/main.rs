//! Every horizon-world parity probe in one binary: `probe <name> [arguments]` runs `<name>.rs` as its own
//! example did (tools/test_*_parity.py). Linking ~170 examples, each instantiating the crate's generics again,
//! was most of the parity job's probe build; one binary builds them once.
//!
//! This file only lists the probes, so that adding one reruns no harness: tools/parity.py leaves it out of
//! their dependencies. How a probe runs is in dispatch.rs.
#[macro_use]
mod dispatch;

use dispatch::args;

/// Code shared by several probes.
mod support {
    pub mod common;
    pub mod decoration_storage;
    pub mod placement_queries;
    pub mod shapes;
}

mod active_effect_tick_fixture;
mod active_effects_fixture;
mod anchor_fixture;
mod attribute_fixture;
mod attribute_health_fixture;
mod attribute_map_queries_fixture;
mod attribute_updates_fixture;
mod basic_feature_fixture;
mod biome_chunk_fixture;
mod biome_generation_fixture;
mod biome_placement_fixture;
mod biome_source_codec_fixture;
mod biome_source_fixture;
mod biome_temperature_fixture;
mod block_collision_scan_fixture;
mod block_entity_storage_fixture;
mod block_landing_fixture;
mod block_properties_fixture;
mod block_push_fixture;
mod block_state_codec_fixture;
mod block_support_fixture;
mod block_survival_fixture;
mod block_update_fixture;
mod blocking_item_damage_fixture;
mod blocking_rules_fixture;
mod bulk_section_fixture;
mod bundle_weight_fixture;
mod canyon_carver_fixture;
mod carving_geometry_fixture;
mod carving_mask_fixture;
mod cave_carver_fixture;
mod chunk_dump;
mod chunk_stage_fixture;
mod chunk_status_fixture;
mod climbing_fixture;
mod collision_context_fixture;
mod collision_fixture;
mod combat_rules_fixture;
mod component_durability_fixture;
mod component_patch_fixture;
mod consumable_emission_fixture;
mod consumable_fixture;
mod consume_effect_fixture;
mod damage_immunity_fixture;
mod damage_reduction_fixture;
mod damage_source_fixture;
mod datafix_world;
mod decoration_fixture;
mod decoration_region_fixture;
mod dynamic_support_fixture;
mod edge_backoff_fixture;
mod effect_attribute_modifiers_fixture;
mod effect_notifications_fixture;
mod effect_sync_fixture;
mod entity_attribute_fixture;
mod entity_collision_fixture;
mod entity_fluid_fixture;
mod entity_move_fixture;
mod entity_obstruction_fixture;
mod extra_items_fixture;
mod face_support_fixture;
mod face_visibility_fixture;
mod fall_damage_fixture;
mod feature_catalog_fixture;
mod feature_graph_fixture;
mod feature_placement_fixture;
mod feature_region_fixture;
mod feature_sort_fixture;
mod fill_layer_fixture;
mod fluid_box_fixture;
mod fluid_fixture;
mod fluid_flow_fixture;
mod fluid_occupancy_fixture;
mod fluid_shape_fixture;
mod food_fixture;
mod food_properties_fixture;
mod food_storage_fixture;
mod ground_state_fixture;
mod height_provider_fixture;
mod heightmap_fixture;
mod interaction_shape_fixture;
mod inventory_pickup_fixture;
mod inventory_tick_fixture;
mod inventory_use_fixture;
mod inventory_use_start_fixture;
mod item_after_use_fixture;
mod item_blocking_fixture;
mod item_cooldowns_fixture;
mod item_durability_fixture;
mod item_particles_fixture;
mod item_stack_storage_fixture;
mod item_template_fixture;
mod item_use_catalog_fixture;
mod item_use_fixture;
mod item_use_lifecycle_fixture;
mod item_use_start_fixture;
mod java_hash_map_fixture;
mod java_identity_map_fixture;
mod level_place_fixture;
mod level_shape_fixture;
mod live_attribute_updates_fixture;
mod live_attributes_fixture;
mod living_air_fixture;
mod living_damage_fixture;
mod living_fall_fixture;
mod living_fluid_fixture;
mod living_health_fixture;
mod living_hurt_fixture;
mod living_landing_fixture;
mod living_motion_fixture;
mod material_condition_fixture;
mod material_extension_fixture;
mod material_stage_fixture;
mod material_surface_fixture;
mod movement_emission_fixture;
mod movement_registry_fixture;
mod movement_support_fixture;
mod object_open_map_fixture;
mod object_open_set_fixture;
mod piston_movement_fixture;
mod piston_push_fixture;
mod piston_shape_fixture;
mod placement_fixture;
mod player_equipment_fixture;
mod player_food_fixture;
mod player_health_fixture;
mod player_hurt_fixture;
mod player_inventory_fixture;
mod player_travel_fixture;
mod portal_travel_fixture;
mod redstone_catalog_fixture;
mod registered_player_combat_fixture;
mod registry_collision_scan_fixture;
mod registry_fall_ray_fixture;
mod registry_fluid_fixture;
mod replace_block_fixture;
mod restitution_fixture;
mod retrogen_data_fixture;
mod rule_test_fixture;
mod scattered_ore_fixture;
mod server_player_combat_fixture;
mod server_player_damage_fixture;
mod simple_block_fixture;
mod simple_container_fixture;
mod spawn_origin;
mod state_provider_fixture;
mod status_effect_fixture;
mod status_tick_fixture;
mod stepped_interpolation_fixture;
mod stored_consume_fixture;
mod stored_effect_attributes_fixture;
mod stored_effect_notifications_fixture;
mod structure_density_fixture;
mod structure_pieces;
mod structure_start_nbt;
mod suffocation_fixture;
mod swimming_fixture;
mod synced_bytes_fixture;
mod synced_floats_fixture;
mod teleport_consume_fixture;
mod terrain_fixture;
mod tick_fixture;
mod tree_feature_fixture;
mod use_flag_reaction_fixture;
mod use_remainder_fixture;
mod value_provider_fixture;
mod view_direction_fixture;
mod water_splash_fixture;
mod world_border_fixture;
mod world_fixture;
mod world_ray_fixture;
mod worldgen_random_fixture;

probes! {
    active_effect_tick_fixture,
    active_effects_fixture,
    anchor_fixture,
    attribute_fixture,
    attribute_health_fixture,
    attribute_map_queries_fixture,
    attribute_updates_fixture,
    basic_feature_fixture,
    biome_chunk_fixture,
    biome_generation_fixture,
    biome_placement_fixture,
    biome_source_codec_fixture,
    biome_source_fixture,
    biome_temperature_fixture,
    block_collision_scan_fixture,
    block_entity_storage_fixture,
    block_landing_fixture,
    block_properties_fixture,
    block_push_fixture,
    block_state_codec_fixture,
    block_support_fixture,
    block_survival_fixture,
    block_update_fixture,
    blocking_item_damage_fixture,
    blocking_rules_fixture,
    bulk_section_fixture,
    bundle_weight_fixture,
    canyon_carver_fixture,
    carving_geometry_fixture,
    carving_mask_fixture,
    cave_carver_fixture,
    chunk_dump,
    chunk_stage_fixture,
    chunk_status_fixture,
    climbing_fixture,
    collision_context_fixture,
    collision_fixture,
    combat_rules_fixture,
    component_durability_fixture,
    component_patch_fixture,
    consumable_emission_fixture,
    consumable_fixture,
    consume_effect_fixture,
    damage_immunity_fixture,
    damage_reduction_fixture,
    damage_source_fixture,
    datafix_world,
    decoration_fixture,
    decoration_region_fixture,
    dynamic_support_fixture,
    edge_backoff_fixture,
    effect_attribute_modifiers_fixture,
    effect_notifications_fixture,
    effect_sync_fixture,
    entity_attribute_fixture,
    entity_collision_fixture,
    entity_fluid_fixture,
    entity_move_fixture,
    entity_obstruction_fixture,
    extra_items_fixture,
    face_support_fixture,
    face_visibility_fixture,
    fall_damage_fixture,
    feature_catalog_fixture,
    feature_graph_fixture,
    feature_placement_fixture,
    feature_region_fixture,
    feature_sort_fixture,
    fill_layer_fixture,
    fluid_box_fixture,
    fluid_fixture,
    fluid_flow_fixture,
    fluid_occupancy_fixture,
    fluid_shape_fixture,
    food_fixture,
    food_properties_fixture,
    food_storage_fixture,
    ground_state_fixture,
    height_provider_fixture,
    heightmap_fixture,
    interaction_shape_fixture,
    inventory_pickup_fixture,
    inventory_tick_fixture,
    inventory_use_fixture,
    inventory_use_start_fixture,
    item_after_use_fixture,
    item_blocking_fixture,
    item_cooldowns_fixture,
    item_durability_fixture,
    item_particles_fixture,
    item_stack_storage_fixture,
    item_template_fixture,
    item_use_catalog_fixture,
    item_use_fixture,
    item_use_lifecycle_fixture,
    item_use_start_fixture,
    java_hash_map_fixture,
    java_identity_map_fixture,
    level_place_fixture,
    level_shape_fixture,
    live_attribute_updates_fixture,
    live_attributes_fixture,
    living_air_fixture,
    living_damage_fixture,
    living_fall_fixture,
    living_fluid_fixture,
    living_health_fixture,
    living_hurt_fixture,
    living_landing_fixture,
    living_motion_fixture,
    material_condition_fixture,
    material_extension_fixture,
    material_stage_fixture,
    material_surface_fixture,
    movement_emission_fixture,
    movement_registry_fixture,
    movement_support_fixture,
    object_open_map_fixture,
    object_open_set_fixture,
    piston_movement_fixture,
    piston_push_fixture,
    piston_shape_fixture,
    placement_fixture,
    player_equipment_fixture,
    player_food_fixture,
    player_health_fixture,
    player_hurt_fixture,
    player_inventory_fixture,
    player_travel_fixture,
    portal_travel_fixture,
    redstone_catalog_fixture,
    registered_player_combat_fixture,
    registry_collision_scan_fixture,
    registry_fall_ray_fixture,
    registry_fluid_fixture,
    replace_block_fixture,
    restitution_fixture,
    retrogen_data_fixture,
    rule_test_fixture,
    scattered_ore_fixture,
    server_player_combat_fixture,
    server_player_damage_fixture,
    simple_block_fixture,
    simple_container_fixture,
    spawn_origin,
    state_provider_fixture,
    status_effect_fixture,
    status_tick_fixture,
    stepped_interpolation_fixture,
    stored_consume_fixture,
    stored_effect_attributes_fixture,
    stored_effect_notifications_fixture,
    structure_density_fixture,
    structure_pieces,
    structure_start_nbt,
    suffocation_fixture,
    swimming_fixture,
    synced_bytes_fixture,
    synced_floats_fixture,
    teleport_consume_fixture,
    terrain_fixture,
    tick_fixture,
    tree_feature_fixture,
    use_flag_reaction_fixture,
    use_remainder_fixture,
    value_provider_fixture,
    view_direction_fixture,
    water_splash_fixture,
    world_border_fixture,
    world_fixture,
    world_ray_fixture,
    worldgen_random_fixture,
}
