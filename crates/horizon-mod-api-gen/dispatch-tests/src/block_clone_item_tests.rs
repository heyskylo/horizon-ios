mod block_clone_item_tests {
    use super::*;
    struct CloneItem {
        responds: bool,
        calls: Arc<Mutex<Vec<bool>>>,
    }
    impl Hooks for CloneItem {
        fn block_clone_item(
            &self,
            _: &dyn Host,
            event: &mut BlockCloneItemEvent<'_>,
        ) -> Option<ItemStack> {
            assert_eq!(event.block, "fixture:heater");
            assert_eq!(event.pos, [2, 64, 2]);
            self.calls.lock().unwrap().push(self.responds);
            self.responds.then_some(ItemStack)
        }
    }
    #[test]
    fn first_clone_response_owns_the_result_and_unanswered_stays_empty() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods: Vec<Box<dyn Hooks>> = [false, true, true]
            .into_iter()
            .map(|responds| {
                Box::new(CloneItem {
                    responds,
                    calls: calls.clone(),
                }) as Box<dyn Hooks>
            })
            .collect();
        let mut result = None;
        dispatch_block_clone_item(
            &TestHost,
            &[],
            "fixture:heater",
            [2, 64, 2],
            BlockStateId,
            &mut result,
        );
        assert!(result.is_none());
        dispatch_block_clone_item(
            &TestHost,
            &mods,
            "fixture:heater",
            [2, 64, 2],
            BlockStateId,
            &mut result,
        );
        assert!(result.is_some());
        assert_eq!(*calls.lock().unwrap(), [false, true]);
    }
}
