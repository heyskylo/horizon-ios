use super::*;
use horizon_world::attachments::{AttachmentCodec, AttachmentError, AttachmentType, Attachments};
use horizon_world::nbt::Tag;

static PROGRESS: AttachmentType<i32> = AttachmentType {
    namespace: "test",
    name: "progress",
    default: || 0,
    codec: Some(AttachmentCodec {
        encode: |n: &i32| Ok([("n".into(), Tag::Int(*n))].into()),
        decode: |c| match c.get(&"n".into()) {
            Some(Tag::Int(n)) => Ok(*n),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: true,
    sync: false,
};

/// The live value as the named owner file would hold it.
fn saved_value(game: &Game<'_>) -> Option<i32> {
    let Some(Tag::Compound(map)) = game.level.player_extra.as_ref() else {
        return None;
    };
    let data = horizon_world::attachments::attachment_compound(
        map.get(&horizon_world::attachments::ATTACHMENTS_KEY.into()),
    )
    .ok()
    .flatten()?;
    Attachments::from_nbt(data.clone())
        .get_existing(&PROGRESS)
        .ok()
        .flatten()
}

#[test]
#[ignore = "needs prepared resources and scratch homes/cwd"]
fn an_autosave_writes_the_owners_live_attachments_to_the_file_it_loads() {
    let mut game = super::super::native_machines::tests::server_game();
    let _rx = super::world_spawn_sync_tests::host_for(&mut game);
    let uuid = 1;
    game.lan.as_mut().unwrap().owner = Some(Owner { uuid, saved: None });
    // While the owner plays, its attachments are the game's, not the guest's.
    let mut live = Attachments::default();
    live.set(&PROGRESS, 7).unwrap();
    game.guest_attachments.insert(uuid, live);
    assert!(game.store_owner().is_some());
    assert_eq!(
        saved_value(&game),
        Some(7),
        "the autosave stores the live value"
    );
    let host = game.lan.as_ref().unwrap();
    let g = host.hub.guests.iter().find(|g| g.uuid == uuid).unwrap();
    assert!(
        g.attachments.to_nbt().is_empty(),
        "the map stays their one owner"
    );
    // A later change is what the next autosave writes.
    game.guest_attachments
        .get_mut(&uuid)
        .unwrap()
        .set(&PROGRESS, 9)
        .unwrap();
    game.store_owner();
    assert_eq!(saved_value(&game), Some(9));
}
