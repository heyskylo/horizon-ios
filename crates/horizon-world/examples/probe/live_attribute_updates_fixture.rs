#[allow(
    dead_code,
    reason = "this fixture shares another one's source and uses part of it"
)]
mod integration {
    include!("live_attributes_fixture.rs");
    use horizon_world::{
        active_effects::{ActiveEffects, EffectHooks},
        attribute_updates::{self, UpdateWorld, UpdatedAttribute},
        attributes::live::Key,
        effect_notifications::{self as notify, EffectPacket, NotificationWorld},
        java_hash_map::JavaKey,
        live_attribute_updates::{self as live_updates, LiveAttributeWorld},
        living_health::{self, HealthWorld, SyncedFloat},
        object_open_set::{CursorError, ObjectOpenSet},
        status_effect::{Details, EffectInstance},
    };
    type Live = LiveAttributes<Box<dyn FnMut() -> i32>>;
    type Result<T, E = &'static str> = std::result::Result<T, E>;
    #[derive(Clone, PartialEq, Eq)]
    struct EffectKey;
    impl JavaKey for EffectKey {
        fn java_hash(&self) -> i32 {
            0
        }
        fn tie_break(&self, _: &Self) -> std::cmp::Ordering {
            std::cmp::Ordering::Less
        }
    }
    type Storage = ActiveEffects<EffectKey>;
    type Effect = EffectInstance<EffectKey>;
    fn single(v: &Value) -> f32 {
        f32::from_bits(v.as_u64().unwrap() as u32)
    }
    fn float_bits(v: f32) -> u32 {
        if v.is_nan() { 0x7fc00000 } else { v.to_bits() }
    }
    fn stop(e: Error) -> &'static str {
        match e {
            Error::NullAttribute | Error::MissingInstanceModifier => "null",
            Error::MissingAttribute | Error::MissingModifier | Error::DuplicateModifier(_) => {
                "argument"
            }
            _ => panic!("invalid fixture: {e:?}"),
        }
    }
    fn cursor_stop(e: CursorError) -> &'static str {
        match e {
            CursorError::IndexOutOfBounds => "bounds",
            CursorError::NoSuchElement => "empty",
            _ => "null",
        }
    }
    struct World<'a> {
        maps: [Live; 2],
        health: [SyncedFloat; 2],
        absorption: f32,
        selection: usize,
        direct: usize,
        data: usize,
        manager: usize,
        server: bool,
        client: bool,
        calls: i64,
        op: Value,
        definitions: Definitions<Key>,
        pools: [Vec<Modifier>; 2],
        out: &'a mut dyn Write,
    }
    impl World<'_> {
        fn key(&self, op: &Value) -> Option<Key> {
            let index = op["attribute"].as_i64().unwrap();
            (index >= 0).then(|| self.maps[0].keys()[index as usize].clone())
        }
        fn controls(&mut self, c: &Value) {
            if let Some(v) = c["selection"].as_u64() {
                self.selection = v as usize;
            }
            if let Some(v) = c["direct"].as_u64() {
                self.direct = v as usize;
            }
            if let Some(v) = c["data"].as_u64() {
                self.data = v as usize;
            }
            if let Some(v) = c["manager"].as_u64() {
                self.manager = v as usize;
            }
            if let Some(v) = c["server"].as_bool() {
                self.server = v;
            }
            if let Some(v) = c["client"].as_bool() {
                self.client = v;
            }
            if !c["absorption"].is_null() {
                self.absorption = single(&c["absorption"]);
            }
        }
        fn event(&mut self, name: impl AsRef<str>) -> Result<()> {
            let call = self.calls;
            self.calls += 1;
            writeln!(self.out, "event {call} {}", name.as_ref()).unwrap();
            if call == self.op["fail"].as_i64().unwrap() {
                return Err("callback");
            }
            if call == self.op["mutate"].as_i64().unwrap() {
                let changes = self.op["changes"].clone();
                self.controls(&changes);
                for action in self.op["actions"].as_array().unwrap().clone() {
                    self.raw(&action)?;
                }
            }
            Ok(())
        }
        fn attribute_value(&mut self, key: Option<Key>) -> Result<f64> {
            self.event(format!("value {}", key.as_ref().map_or("null", Key::name)))?;
            live_updates::value(self, key, ValueKind::Computed)
        }
        fn raw(&mut self, op: &Value) -> Result<String> {
            let key = self.key(op);
            let target = op["target"].as_u64().unwrap() as usize;
            let m = Modifier::new(
                op["id"].as_str().unwrap(),
                number(&op["amount"]),
                Operation::ALL[op["operation"].as_u64().unwrap() as usize],
            )
            .unwrap();
            match op["mode"].as_str().unwrap() {
                "controls" => {
                    self.controls(&op["changes"]);
                    Ok("ok".into())
                }
                "ensure" => self.maps[target]
                    .get_instance(key)
                    .map(|h| handle(h.as_ref()))
                    .map_err(stop),
                "reset" => self.maps[target]
                    .reset_base(key)
                    .map(|v| v.to_string())
                    .map_err(stop),
                "define" => {
                    effects::add_definition(
                        &mut self.definitions,
                        key,
                        Template::new(m.id(), m.amount(), m.operation()).unwrap(),
                    );
                    Ok("ok".into())
                }
                "effect-add" => self.maps[target]
                    .add_effect_modifiers(
                        &mut self.definitions,
                        op["amplifier"].as_i64().unwrap() as i32,
                    )
                    .map(|()| "ok".into())
                    .map_err(|e| stop(effect_error(e))),
                "effect-remove" => self.maps[target]
                    .remove_effect_modifiers(&mut self.definitions)
                    .map(|()| "ok".into())
                    .map_err(|e| stop(effect_error(e))),
                "clear-updates" => {
                    self.maps[target].updates_mut().clear();
                    Ok("ok".into())
                }
                "clear-sync" => {
                    self.maps[target].sync_mut().clear();
                    Ok("ok".into())
                }
                "null-update" => {
                    self.maps[target].updates_mut().add(None);
                    Ok("ok".into())
                }
                mode => {
                    let Some(h) = self.maps[target].get_instance(key).map_err(stop)? else {
                        return Ok("absent".into());
                    };
                    self.maps[target]
                        .edit(&h, |a, o| match mode {
                            "set" => a
                                .set_base_value(number(&op["base"]), o)
                                .map(|()| "ok".into()),
                            "update" => a.add_or_update_transient(m, o).map(|()| "ok".into()),
                            "permanent" => a.add_permanent(m, o).map(|()| "ok".into()),
                            "remove" => a.remove(m.id(), o).map(|v| v.to_string()),
                            "clear" => a.remove_all(o).map(|()| "ok".into()),
                            _ => panic!("raw mode: {mode}"),
                        })
                        .map_err(stop)
                }
            }
        }
        fn state(&mut self, storage: &Storage) {
            writeln!(
                self.out,
                "controls {} {} {} {} {} {} {} {}",
                self.selection,
                self.direct,
                self.data,
                self.manager,
                self.server,
                self.client,
                self.calls,
                storage.dirty
            )
            .unwrap();
            for (i, h) in self.health.iter().enumerate() {
                writeln!(
                    self.out,
                    "health {i} {} {} {} {}",
                    float_bits(h.value()),
                    h.is_dirty(),
                    h.item_dirty(),
                    h.is_default()
                )
                .unwrap();
            }
            writeln!(self.out, "absorption {}", float_bits(self.absorption)).unwrap();
            for (i, map) in self.maps.iter().enumerate() {
                writeln!(self.out, "attributes {i}").unwrap();
                writeln!(
                    self.out,
                    "map {}",
                    map.entries()
                        .iter()
                        .map(|(k, h)| format!(
                            "{}={}",
                            k.as_ref().map_or("null", Key::name),
                            handle(h.as_ref())
                        ))
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
                writeln!(
                    self.out,
                    "updates {}",
                    map.updates()
                        .values()
                        .map(handle)
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
                writeln!(
                    self.out,
                    "sync {}",
                    map.sync()
                        .values()
                        .map(handle)
                        .collect::<Vec<_>>()
                        .join(",")
                )
                .unwrap();
                for (h, key, a) in map.instances() {
                    writeln!(
                        self.out,
                        "instance {} {} {} {} {} | {} | {}",
                        h.index(),
                        key.name(),
                        f64_hex(a.base_value()),
                        a.is_dirty(),
                        f64_hex(a.cached_value()),
                        refs(a.modifiers(), &mut self.pools[i]),
                        refs(a.permanent_modifiers(), &mut self.pools[i])
                    )
                    .unwrap();
                }
            }
        }
    }
    impl HealthWorld for World<'_> {
        type Error = &'static str;
        type Data = usize;
        fn health(&mut self) -> Result<f32> {
            self.event("health")?;
            Ok(self.health[self.data].value())
        }
        fn health_data(&self) -> usize {
            self.data
        }
        fn max_health_attribute(&mut self) -> Result<f64> {
            self.attribute_value(self.maps[0].key("minecraft:max_health"))
        }
        fn max_absorption_attribute(&mut self) -> Result<f64> {
            self.attribute_value(self.maps[0].key("minecraft:max_absorption"))
        }
        fn store_health(&mut self, data: usize, value: f32) -> Result<()> {
            let mut h = self.health[data];
            let result = h.set(value, false, |current| {
                self.health[data] = *current;
                let result = self.event("health-updated");
                *current = self.health[data];
                result
            });
            self.health[data] = h;
            result
        }
        fn store_absorption(&mut self, value: f32) -> Result<()> {
            self.event(format!("store-absorption {}", float_bits(value)))?;
            self.absorption = value;
            Ok(())
        }
        fn dead_flag(&self) -> bool {
            false
        }
        fn dispatch_set_health(&mut self, value: f32) -> Result<()> {
            self.event(format!("set-health {}", float_bits(value)))?;
            living_health::set_health(self, value)
        }
    }
    impl UpdateWorld<Key> for World<'_> {
        type Attributes = usize;
        type Level = bool;
        type Manager = usize;
        fn attribute_kind(&self, key: &Key) -> UpdatedAttribute {
            live_updates::kind(key)
        }
        fn absorption(&mut self) -> Result<f32> {
            self.event("absorption")?;
            Ok(self.absorption)
        }
        fn refresh_dimensions(&mut self) -> Result<()> {
            self.event("dimensions")
        }
        fn level(&mut self) -> Result<bool> {
            self.event("level")?;
            Ok(self.server)
        }
        fn is_server_level(&self, level: bool) -> bool {
            level
        }
        fn waypoint_manager(&mut self, _: bool) -> Result<usize> {
            self.event("manager")?;
            Ok(self.manager)
        }
        fn attributes_field(&self) -> usize {
            self.direct
        }
        fn attribute_value_in(&mut self, map: usize, key: Key) -> Result<f64> {
            live_updates::value_in(self, map, Some(key), ValueKind::Computed)
        }
        fn track_waypoint(&mut self, manager: usize, track: bool) -> Result<()> {
            self.event(format!("waypoint {manager} {track}"))
        }
    }
    impl LiveAttributeWorld for World<'_> {
        type Identity = Box<dyn FnMut() -> i32>;
        fn get_attributes(&mut self) -> Result<usize> {
            self.event("attributes")?;
            Ok(self.selection)
        }
        fn live_attributes(&self, map: usize) -> &Live {
            &self.maps[map]
        }
        fn live_attributes_mut(&mut self, map: usize) -> &mut Live {
            &mut self.maps[map]
        }
        fn instance_attribute(&self, instance: Option<Handle>) -> Result<Key> {
            let h = instance.ok_or("null")?;
            self.maps
                .iter()
                .find_map(|m| m.attribute(&h).ok().cloned())
                .ok_or("null")
        }
        fn attribute_error(error: Error) -> &'static str {
            stop(error)
        }
        fn cursor_error(error: CursorError) -> &'static str {
            cursor_stop(error)
        }
        fn dispatch_attribute_updated(&mut self, key: Key) -> Result<()> {
            self.event(format!("updated {}", key.name()))?;
            attribute_updates::updated(self, key)
        }
    }
    impl EffectHooks<EffectKey> for World<'_> {
        type Error = &'static str;
        type Source = ();
        type Level = bool;
        fn can_be_affected(&mut self, _: &mut Storage, _: Effect) -> Result<bool> {
            Ok(true)
        }
        fn on_added(&mut self, s: &mut Storage, e: Effect, _: Option<()>) -> Result<()> {
            notify::on_added(s, &e, self)
        }
        fn on_updated(
            &mut self,
            s: &mut Storage,
            e: Effect,
            reapply: bool,
            _: Option<()>,
        ) -> Result<()> {
            notify::on_updated(s, &e, reapply, self)
        }
        fn on_removed(&mut self, s: &mut Storage, es: Vec<Effect>) -> Result<()> {
            notify::on_removed(s, &es, self)
        }
        fn effect_added(&mut self, _: &mut Storage, _: EffectKey, _: i32) -> Result<()> {
            Ok(())
        }
        fn effect_started(&mut self, _: &mut Storage, _: EffectKey, _: i32) -> Result<()> {
            Ok(())
        }
        fn level(&mut self, _: &mut Storage) -> Result<bool> {
            UpdateWorld::level(self)
        }
        fn is_client(&mut self, _: &mut Storage, level: bool) -> Result<bool> {
            if !level {
                return Err("null");
            }
            self.event("client")?;
            Ok(self.client)
        }
        fn actor_removed(&self) -> bool {
            false
        }
    }
    impl NotificationWorld<EffectKey> for World<'_> {
        type Attributes = usize;
        type UpdateSet = usize;
        type AttributeInstance = Handle;
        type Attribute = Key;
        type Passenger = ();
        type Connection = ();
        fn attributes(&mut self, _: &mut Storage) -> Result<usize> {
            self.get_attributes()
        }
        fn add_modifiers(
            &mut self,
            _: &mut Storage,
            _: EffectKey,
            map: usize,
            amp: i32,
        ) -> Result<()> {
            self.maps[map]
                .add_effect_modifiers(&mut self.definitions, amp)
                .map_err(|e| stop(effect_error(e)))
        }
        fn remove_modifiers(&mut self, _: &mut Storage, _: EffectKey, map: usize) -> Result<()> {
            self.maps[map]
                .remove_effect_modifiers(&mut self.definitions)
                .map_err(|e| stop(effect_error(e)))
        }
        fn pending_updates(&mut self, _: &mut Storage, map: usize) -> Result<usize> {
            Ok(map)
        }
        fn update_set(&self, set: usize) -> &ObjectOpenSet<Handle> {
            self.maps[set].updates()
        }
        fn update_set_mut(&mut self, set: usize) -> &mut ObjectOpenSet<Handle> {
            self.maps[set].updates_mut()
        }
        fn iterator_error(e: CursorError) -> &'static str {
            cursor_stop(e)
        }
        fn attribute_of(&mut self, _: &mut Storage, instance: Option<Handle>) -> Result<Key> {
            self.instance_attribute(instance)
        }
        fn attribute_updated(&mut self, _: &mut Storage, key: Key) -> Result<()> {
            self.dispatch_attribute_updated(key)
        }
        fn passengers(&self) -> Vec<()> {
            vec![]
        }
        fn is_server_player(&self, _: ()) -> bool {
            false
        }
        fn connection(&self, _: ()) {}
        fn entity_id(&mut self, _: &mut Storage) -> Result<i32> {
            Ok(0)
        }
        fn send_packet(
            &mut self,
            _: &mut Storage,
            _: (),
            _: EffectPacket<EffectKey>,
        ) -> Result<()> {
            Ok(())
        }
    }
    pub fn run() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let args: Vec<_> = crate::args().collect();
        let dir = Path::new(&args[1]);
        let catalog = Arc::new(Catalog::from_json(&serde_json::from_str(
            &fs::read_to_string(dir.join("catalog.json"))?,
        )?)?);
        let mut out =
            io::BufWriter::new(fs::File::create(dir.join(format!("rust-{}.txt", args[2])))?);
        for line in fs::read_to_string(dir.join(format!("inputs-{}.txt", args[2])))?.lines() {
            let f: Value = serde_json::from_str(line)?;
            let hashes: Vec<_> = f["holder_hashes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let maps = std::array::from_fn(|i| {
                let mut ids = f["instance_hashes"][i]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap() as i32)
                    .collect::<Vec<_>>()
                    .into_iter();
                Live::new(
                    catalog.clone(),
                    f["entity"].as_str().unwrap(),
                    &hashes,
                    Box::new(move || ids.next().unwrap()),
                )
                .unwrap()
            });
            let mut w = World {
                maps,
                health: std::array::from_fn(|i| SyncedFloat::new(single(&f["health"][i]))),
                absorption: single(&f["absorption"]),
                selection: 0,
                direct: 0,
                data: 0,
                manager: 0,
                server: true,
                client: false,
                calls: 0,
                op: Value::Null,
                definitions: Definitions::default(),
                pools: [vec![], vec![]],
                out: &mut out,
            };
            let mut storage = Storage::default();
            for op in f["setup"].as_array().unwrap() {
                w.raw(op).unwrap();
            }
            for op in f["ops"].as_array().unwrap() {
                w.op = op.clone();
                w.calls = 0;
                let key = w.key(op);
                let effect = EffectInstance::new(
                    EffectKey,
                    Details {
                        duration: 200,
                        amplifier: op["amplifier"].as_i64().unwrap() as i32,
                        ambient: false,
                        visible: true,
                        show_icon: true,
                    },
                    None,
                );
                let result = match op["mode"].as_str().unwrap() {
                    "value" => w.attribute_value(key).map(f64_hex),
                    "base" => live_updates::value(&mut w, key, ValueKind::Base).map(f64_hex),
                    "set-health" => w
                        .dispatch_set_health(single(&op["value"]))
                        .map(|()| "ok".into()),
                    "set-absorption" => living_health::set_absorption(&mut w, single(&op["value"]))
                        .map(|()| "ok".into()),
                    "heal" => {
                        living_health::heal(&mut w, single(&op["value"])).map(|()| "ok".into())
                    }
                    "attribute-update" => match key {
                        Some(key) => w.dispatch_attribute_updated(key),
                        None => w.event("updated null").and(Err("null")),
                    }
                    .map(|()| "ok".into()),
                    "refresh" => live_updates::refresh(&mut w).map(|()| "ok".into()),
                    "notify-add" => {
                        notify::on_added(&mut storage, &effect, &mut w).map(|()| "ok".into())
                    }
                    "notify-update" => notify::on_updated(
                        &mut storage,
                        &effect,
                        op["reapply"].as_bool().unwrap(),
                        &mut w,
                    )
                    .map(|()| "ok".into()),
                    "notify-remove" => notify::on_removed(
                        &mut storage,
                        &vec![effect; op["count"].as_u64().unwrap() as usize],
                        &mut w,
                    )
                    .map(|()| "ok".into()),
                    "pack" => Ok(w.health[op["target"].as_u64().unwrap() as usize]
                        .take_dirty()
                        .map_or("none".into(), |v| float_bits(v).to_string())),
                    "dirty" => {
                        storage.dirty = op["flag"].as_bool().unwrap();
                        Ok("ok".into())
                    }
                    _ => w.raw(op),
                };
                writeln!(
                    w.out,
                    "result {}",
                    result.unwrap_or_else(|e| format!("error {e}"))
                )?;
                w.state(&storage);
            }
        }
        Ok(())
    }
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    integration::run()
}
