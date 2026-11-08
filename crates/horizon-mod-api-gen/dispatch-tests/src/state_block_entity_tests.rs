// The generated dispatcher only needs borrowed property lookup. The actual
// Game fixture separately exercises the registry's compact BlockProperties.
type BlockProperties = std::collections::BTreeMap<String, String>;

mod state_block_entity_tests {
    use super::*;

    struct Factory {
        id: u32,
        declares: bool,
        responds: bool,
        calls: Arc<Mutex<Vec<u32>>>,
    }
    impl Hooks for Factory {
        fn stateful_block_entity(&self, _: &dyn Host, event: &mut StatefulBlockEntityEvent<'_>) {
            assert_eq!(event.block, "fixture:heater");
            self.calls.lock().unwrap().push(self.id);
            if self.declares {
                event.stateful = true;
            }
        }
        fn block_entity_from_state(
            &self,
            event: &mut BlockEntityFromStateEvent<'_>,
        ) -> Option<(String, Compound)> {
            assert_eq!(event.block, "fixture:heater");
            assert_eq!(
                event.properties.get("filled").map(String::as_str),
                Some("true")
            );
            self.calls.lock().unwrap().push(self.id);
            self.responds
                .then(|| (format!("fixture:entity_{}", self.id), Compound))
        }
    }
    fn factories(calls: &Arc<Mutex<Vec<u32>>>, responses: &[bool]) -> Vec<Box<dyn Hooks>> {
        responses
            .iter()
            .enumerate()
            .map(|(index, responds)| {
                Box::new(Factory {
                    id: index as u32 + 1,
                    declares: index == 1,
                    responds: *responds,
                    calls: calls.clone(),
                }) as Box<dyn Hooks>
            })
            .collect()
    }
    #[test]
    fn declarations_listen_in_order_without_clearing_an_earlier_owner() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods = factories(&calls, &[false, false, false]);
        let mut stateful = false;
        dispatch_stateful_block_entity(&TestHost, &mods, "fixture:heater", &mut stateful);
        assert!(stateful);
        assert_eq!(*calls.lock().unwrap(), [1, 2, 3]);
        dispatch_stateful_block_entity(&TestHost, &[], "fixture:heater", &mut stateful);
        assert!(
            stateful,
            "an unanswered declaration does not clear its caller's value"
        );
    }
    #[test]
    fn empty_state_factory_remains_empty_and_first_response_wins() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut entity = None;
        let properties = std::collections::BTreeMap::from([("filled".into(), "true".into())]);
        dispatch_block_entity_from_state(&[], "fixture:heater", &properties, &mut entity);
        assert!(entity.is_none());
        let mods = factories(&calls, &[false, false]);
        dispatch_block_entity_from_state(&mods, "fixture:heater", &properties, &mut entity);
        assert!(entity.is_none());
        assert_eq!(*calls.lock().unwrap(), [1, 2]);
        calls.lock().unwrap().clear();
        let mods = factories(&calls, &[false, true, true]);
        dispatch_block_entity_from_state(&mods, "fixture:heater", &properties, &mut entity);
        assert_eq!(
            entity.as_ref().map(|v| v.0.as_str()),
            Some("fixture:entity_2")
        );
        assert_eq!(
            *calls.lock().unwrap(),
            [1, 2],
            "later factories must not run"
        );
    }
}
