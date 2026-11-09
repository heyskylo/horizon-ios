// Reuse the concrete attribute fixture's arena, supplier and observer binding.
#[allow(
    dead_code,
    reason = "this fixture shares another one's source and uses part of it"
)]
mod integration {
    include!("stored_effect_attributes_fixture.rs");

    use horizon_world::{
        active_effects::{ActiveEffects, EffectHooks},
        effect_notifications::{self as notify, EffectPacket, NotificationWorld},
        java_hash_map::JavaKey,
        object_open_set::ObjectOpenSet,
        status_effect::{Details, EffectInstance},
        stored_effect_attributes::{self as stored, AttributeAccess},
    };
    impl AttributeAccess<Key, Handle> for World {
        type Error = Stop;
        fn create_instance(
            &mut self,
            _: &mut Definitions<Key>,
            map: &mut Map,
            key: Option<Key>,
        ) -> Result<Option<Handle>, Stop> {
            self.create(map, key)
        }
        fn get_instance(
            &mut self,
            _: &mut Definitions<Key>,
            map: &mut Map,
            key: Option<Key>,
        ) -> Result<Option<Handle>, Stop> {
            self.get(map, key)
        }
        fn remove_modifier(
            &mut self,
            _: &mut Definitions<Key>,
            map: &mut Map,
            h: &Handle,
            id: &str,
        ) -> Result<(), Stop> {
            self.edit(map, h, |a, cb| a.remove(id, cb)).map(|_| ())
        }
        fn add_permanent(
            &mut self,
            _: &mut Definitions<Key>,
            map: &mut Map,
            h: &Handle,
            modifier: Modifier,
        ) -> Result<(), Stop> {
            self.edit(map, h, |a, cb| a.add_permanent(modifier, cb))
        }
    }
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
    struct Entity<'a> {
        world: &'a mut World,
        map: &'a mut Map,
        definitions: &'a mut Definitions<Key>,
    }
    type Storage = ActiveEffects<EffectKey>;
    type Effect = EffectInstance<EffectKey>;
    impl EffectHooks<EffectKey> for Entity<'_> {
        type Error = Stop;
        type Source = ();
        type Level = ();
        fn can_be_affected(&mut self, _: &mut Storage, _: Effect) -> Result<bool, Stop> {
            Ok(true)
        }
        fn on_added(&mut self, s: &mut Storage, e: Effect, _: Option<()>) -> Result<(), Stop> {
            notify::on_added(s, &e, self)
        }
        fn on_updated(
            &mut self,
            s: &mut Storage,
            e: Effect,
            apply: bool,
            _: Option<()>,
        ) -> Result<(), Stop> {
            notify::on_updated(s, &e, apply, self)
        }
        fn on_removed(&mut self, s: &mut Storage, es: Vec<Effect>) -> Result<(), Stop> {
            notify::on_removed(s, &es, self)
        }
        fn effect_added(&mut self, _: &mut Storage, _: EffectKey, _: i32) -> Result<(), Stop> {
            Ok(())
        }
        fn effect_started(&mut self, _: &mut Storage, _: EffectKey, _: i32) -> Result<(), Stop> {
            Ok(())
        }
        fn level(&mut self, _: &mut Storage) -> Result<(), Stop> {
            self.world.event(self.map, None, "level".into())
        }
        fn is_client(&mut self, _: &mut Storage, _: ()) -> Result<bool, Stop> {
            self.world.event(self.map, None, "client".into())?;
            Ok(self.world.op["client"].as_bool().unwrap_or(false))
        }
        fn actor_removed(&self) -> bool {
            false
        }
    }
    impl NotificationWorld<EffectKey> for Entity<'_> {
        type Attributes = ();
        type UpdateSet = ();
        type AttributeInstance = Handle;
        type Attribute = Key;
        type Passenger = ();
        type Connection = ();
        fn attributes(&mut self, _: &mut Storage) -> Result<(), Stop> {
            self.world.event(self.map, None, "attributes".into())
        }
        fn add_modifiers(
            &mut self,
            _: &mut Storage,
            _: EffectKey,
            _: (),
            amp: i32,
        ) -> Result<(), Stop> {
            stored::add(self.definitions, self.map, self.world, amp).map_err(effect_error)
        }
        fn remove_modifiers(&mut self, _: &mut Storage, _: EffectKey, _: ()) -> Result<(), Stop> {
            stored::remove(self.definitions, self.map, self.world).map_err(effect_error)
        }
        fn pending_updates(&mut self, _: &mut Storage, _: ()) -> Result<(), Stop> {
            Ok(())
        }
        fn update_set(&self, _: ()) -> &ObjectOpenSet<Handle> {
            &self.map.updates
        }
        fn update_set_mut(&mut self, _: ()) -> &mut ObjectOpenSet<Handle> {
            &mut self.map.updates
        }
        fn iterator_error(e: CursorError) -> Stop {
            match e {
                CursorError::IndexOutOfBounds => Stop::Bounds,
                _ => Stop::Null,
            }
        }
        fn attribute_of(&mut self, _: &mut Storage, h: Option<Handle>) -> Result<Key, Stop> {
            let h = h.ok_or(Stop::Null)?;
            Ok(self.world.instances[h.id].key.clone())
        }
        fn attribute_updated(&mut self, _: &mut Storage, key: Key) -> Result<(), Stop> {
            self.world
                .event(self.map, None, format!("updated {}", key.id))
        }
        fn passengers(&self) -> Vec<()> {
            Vec::new()
        }
        fn is_server_player(&self, _: ()) -> bool {
            false
        }
        fn connection(&self, _: ()) {}
        fn entity_id(&mut self, _: &mut Storage) -> Result<i32, Stop> {
            Ok(0)
        }
        fn send_packet(
            &mut self,
            _: &mut Storage,
            _: (),
            _: EffectPacket<EffectKey>,
        ) -> Result<(), Stop> {
            Ok(())
        }
    }

    use horizon_world::attribute_map::{ModifierValueError, QueryWorld, ValueKind};
    impl QueryWorld<Key, Handle> for World {
        type Error = Stop;
        fn supplier_has_attribute(&mut self, _: &mut Map, key: Option<Key>) -> Result<bool, Stop> {
            Ok(key.is_some_and(|k| self.suppliers[k.id].is_some()))
        }
        fn supplier_has_modifier(
            &mut self,
            _: &mut Map,
            key: Option<Key>,
            id: &str,
        ) -> Result<bool, Stop> {
            Ok(key
                .and_then(|k| self.suppliers[k.id].as_ref())
                .is_some_and(|a| a.modifier(id).is_some()))
        }
        fn supplier_value(
            &mut self,
            _: &mut Map,
            key: Option<Key>,
            kind: ValueKind,
        ) -> Result<f64, Stop> {
            let k = key.ok_or(Stop::Null)?;
            let a = self.suppliers[k.id].as_mut().ok_or(Stop::Duplicate)?;
            Ok(match kind {
                ValueKind::Computed => a.value(),
                ValueKind::Base => a.base_value(),
            })
        }
        fn supplier_modifier_value(
            &mut self,
            _: &mut Map,
            key: Option<Key>,
            id: &str,
        ) -> Result<f64, Stop> {
            let k = key.ok_or(Stop::Null)?;
            self.suppliers[k.id]
                .as_ref()
                .and_then(|a| a.modifier(id))
                .map(|m| m.amount())
                .ok_or(Stop::Duplicate)
        }
        fn instance_value(&mut self, _: &mut Map, h: Handle, kind: ValueKind) -> Result<f64, Stop> {
            let a = self.instances[h.id].value.as_mut().unwrap();
            Ok(match kind {
                ValueKind::Computed => a.value(),
                ValueKind::Base => a.base_value(),
            })
        }
        fn instance_modifier(
            &mut self,
            _: &mut Map,
            h: Handle,
            id: &str,
        ) -> Result<Option<f64>, Stop> {
            Ok(self.instances[h.id]
                .value
                .as_ref()
                .unwrap()
                .modifier(id)
                .map(|m| m.amount()))
        }
        fn set_instance_base(&mut self, map: &mut Map, h: Handle, value: f64) -> Result<(), Stop> {
            self.edit(map, &h, |a, cb| a.set_base_value(value, cb))
        }
    }
    impl World {
        fn prototype_states(&mut self) {
            for id in 0..self.suppliers.len() {
                let Some(a) = self.suppliers[id].as_ref() else {
                    self.logs.push(format!("prototype {id} absent"));
                    continue;
                };
                let prefix = format!(
                    "prototype {id} {} {} {}",
                    f64_hex(a.base_value()),
                    a.is_dirty(),
                    f64_hex(a.cached_value())
                );
                let all = a.modifiers().to_vec();
                let permanent = a.permanent_modifiers().to_vec();
                let all = self.refs(&all);
                let permanent = self.refs(&permanent);
                self.logs.push(format!("{prefix} | {all} | {permanent}"));
            }
        }
        fn prototype_edit(&mut self, op: &Value) -> Result<(), Stop> {
            let key = self.key(op).ok_or(Stop::Null)?;
            let a = self.suppliers[key.id].as_mut().ok_or(Stop::Null)?;
            let template = &self.templates[op["template"].as_u64().unwrap() as usize];
            match op["op"].as_str().unwrap() {
                "prototype-base" => a.set_base_value(number(&op["value"]), &mut Silent),
                "prototype-permanent" => a.add_permanent(
                    template.create(op["amplifier"].as_i64().unwrap() as i32),
                    &mut Silent,
                ),
                _ => a.remove(template.id(), &mut Silent).map(|_| ()),
            }
            .map_err(attribute_error)
        }
    }
    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<_> = crate::args().collect();
        let dir = Path::new(&args[1]);
        let mut out =
            io::BufWriter::new(fs::File::create(dir.join(format!("rust-{}.txt", args[2])))?);
        for line in fs::read_to_string(dir.join(format!("inputs-{}.txt", args[2])))?.lines() {
            let f: Value = serde_json::from_str(line)?;
            let keys: Vec<_> = f["hashes"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(id, h)| Key {
                    id,
                    hash: h.as_i64().unwrap() as i32,
                })
                .collect();
            let defaults: Vec<_> = f["defaults"]
                .as_array()
                .unwrap()
                .iter()
                .map(number)
                .collect();
            let suppliers = f["bases"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(id, v)| {
                    if v.is_null() {
                        None
                    } else {
                        let mut a = Instance::new(Definition::basic(defaults[id]));
                        a.set_base_value(number(v), &mut Silent).unwrap();
                        Some(a)
                    }
                })
                .collect();
            let templates = f["templates"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    Template::new(
                        v["id"].as_str().unwrap(),
                        number(&v["amount"]),
                        Operation::ALL[v["operation"].as_u64().unwrap() as usize],
                    )
                })
                .collect::<io::Result<Vec<_>>>()?;
            let syncable = f["syncable"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_bool().unwrap())
                .collect();
            let hashes = f["instance_hashes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let mut world = World {
                keys,
                defaults,
                suppliers,
                templates,
                syncable,
                hashes,
                instances: Vec::new(),
                pool: Vec::new(),
                calls: 0,
                changed: false,
                op: Value::Null,
                logs: Vec::new(),
            };
            let mut map = Map::default();
            let mut defs = Definitions::default();
            let mut storage = ActiveEffects::default();
            for op in f["definitions"].as_array().unwrap() {
                effects::add_definition(
                    &mut defs,
                    world.key(op),
                    world.templates[op["template"].as_u64().unwrap() as usize].clone(),
                );
            }
            for op in f["ops"].as_array().unwrap() {
                world.op = op.clone();
                world.calls = 0;
                world.changed = false;
                let result = match op["op"].as_str().unwrap() {
                    "attribute-has" => map
                        .has_attribute(world.key(op), &mut world)
                        .map(|v| v.to_string()),
                    "modifier-has" => {
                        let id = world.templates[op["template"].as_u64().unwrap() as usize]
                            .id()
                            .to_owned();
                        map.has_modifier(world.key(op), &id, &mut world)
                            .map(|v| v.to_string())
                    }
                    "attribute-value" | "attribute-base" => {
                        let kind = if op["op"] == "attribute-value" {
                            ValueKind::Computed
                        } else {
                            ValueKind::Base
                        };
                        map.value(world.key(op), kind, &mut world).map(f64_hex)
                    }
                    "modifier-value" => {
                        let id = world.templates[op["template"].as_u64().unwrap() as usize]
                            .id()
                            .to_owned();
                        map.modifier_value(world.key(op), &id, &mut world)
                            .map(f64_hex)
                            .map_err(|e| match e {
                                ModifierValueError::World(e) => e,
                                ModifierValueError::MissingInstanceModifier => Stop::Null,
                            })
                    }
                    "reset-base" => map
                        .reset_base(world.key(op), &mut world)
                        .map(|v| v.to_string()),
                    "base" => world
                        .handle(op["instance"].as_i64().unwrap())
                        .and_then(|h| h.ok_or(Stop::Null))
                        .and_then(|h| {
                            world.edit(&mut map, &h, |a, cb| {
                                a.set_base_value(number(&op["value"]), cb)
                            })
                        })
                        .map(|()| "ok".into()),
                    "prototype-base" | "prototype-permanent" | "prototype-remove" => {
                        world.prototype_edit(op).map(|()| "ok".into())
                    }
                    "dirty" => {
                        storage.dirty = op["flag"].as_bool().unwrap();
                        Ok("ok".into())
                    }
                    "notify-add" | "notify-update" | "notify-remove" | "refresh" => {
                        let instance = EffectInstance::new(
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
                        let mut entity = Entity {
                            world: &mut world,
                            map: &mut map,
                            definitions: &mut defs,
                        };
                        let result = match op["op"].as_str().unwrap() {
                            "notify-add" => notify::on_added(&mut storage, &instance, &mut entity),
                            "notify-update" => notify::on_updated(
                                &mut storage,
                                &instance,
                                op["reapply"].as_bool().unwrap(),
                                &mut entity,
                            ),
                            "notify-remove" => notify::on_removed(
                                &mut storage,
                                &vec![instance; op["count"].as_u64().unwrap() as usize],
                                &mut entity,
                            ),
                            _ => notify::refresh_attributes(&mut storage, &mut entity),
                        };
                        result.map(|()| "ok".into())
                    }
                    "get" => world
                        .get(&mut map, world.key(op))
                        .map(|h| reference(h.as_ref())),
                    "lookup" => Ok(reference(
                        map.instances.get(world.key(op).as_ref()).as_ref(),
                    )),
                    "add" => effects::add_modifiers(
                        &mut defs,
                        &mut Bound {
                            world: &mut world,
                            map: &mut map,
                        },
                        op["amplifier"].as_i64().unwrap() as i32,
                    )
                    .map_err(effect_error)
                    .map(|()| "ok".into()),
                    "remove" => effects::remove_modifiers(
                        &mut defs,
                        &mut Bound {
                            world: &mut world,
                            map: &mut map,
                        },
                    )
                    .map_err(effect_error)
                    .map(|()| "ok".into()),
                    "modified" => world
                        .handle(op["instance"].as_i64().unwrap())
                        .and_then(|h| world.modified(&mut map, h, None))
                        .map(|()| "ok".into()),
                    "value" => world
                        .handle(op["instance"].as_i64().unwrap())
                        .and_then(|h| h.ok_or(Stop::Null))
                        .map(|h| f64_hex(world.instances[h.id].value.as_mut().unwrap().value())),
                    _ => world.raw(&mut map, op).map(|()| "ok".into()),
                };
                world.logs.push(format!(
                    "result {}",
                    result.unwrap_or_else(|e| match e {
                        Stop::Callback => "callback",
                        Stop::Duplicate => "duplicate",
                        Stop::Null => "null",
                        Stop::Bounds => "bounds",
                    }
                    .into())
                ));
                world.logs.push(format!("effect-dirty {}", storage.dirty));
                world.state(&map, None);
                world.prototype_states();
                for line in world.logs.drain(..) {
                    writeln!(out, "{line}")?;
                }
            }
        }
        Ok(())
    }
}
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    integration::run()
}
