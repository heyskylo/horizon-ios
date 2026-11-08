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
