//! `Entity.getDisplayName` for the mobs command replies name: the name as its
//! team shows it, with the `show_entity` hover and the UUID insertion.
use super::super::Game;
use horizon_client::text::Text;

impl Game<'_> {
    /// `Entity.getDisplayName` for the mob `id`: `PlayerTeam.formatNameForTeam`
    /// of `getName()`, then `withHoverEvent(createHoverEvent())` (a `ShowEntity`
    /// of the type, UUID and name) and `withInsertion(getStringUUID())`. A mob
    /// gets its UUID when it joins the level; one without keeps the bare name.
    pub(in crate::game) fn mob_command_name(&self, id: u32) -> Text {
        let Some(m) = self.mobs.iter().find(|m| m.id == id) else {
            return Text::translate("entity.", &[]);
        };
        let name = m.command_name();
        let shown = match self.mob_team_of(m) {
            Some(team) => team.formatted_name(name.clone()),
            None => name.clone(),
        };
        let Some(uuid) = self.persist.uuid_of(id) else {
            return shown;
        };
        let uuid = horizon_client::save::uuid_text(uuid);
        let kind = horizon_client::command::args::normalize(&m.kind.id);
        shown.with_component_fields(vec![
            (
                "hover_event".into(),
                serde_json::json!({"action": "show_entity", "id": kind, "uuid": uuid,
                    "name": name.component()}),
            ),
            ("insertion".into(), uuid.into()),
        ])
    }
}
