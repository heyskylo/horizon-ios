mod common;

use std::process::Command;

use common::samples::*;
use common::{TempDir, frame_rows, hot_rows, query, save, split_rows, tree_rows};
use horizon_perf::query::rows::{HotRow, SplitRow, TreeRow};
use horizon_perf::wire::{Candidate, Envelope, SampleBy, Unit};

/// The standard tree: 120 samples of 10 ms over 50 ticks, so a sample is 0.2 ms/tick.
fn standard(dir: &TempDir) -> String {
    save(dir, "samples.hzperf", &waypoints(1, 50).fields())
}

fn hot_ids(reply: &Envelope<horizon_perf::query::Row>) -> Vec<String> {
    hot_rows(reply).iter().map(|r| r.id.clone()).collect()
}

fn hot_row<'a>(rows: &'a [&'a HotRow], id: &str) -> &'a HotRow {
    rows.iter().find(|r| r.id == id).unwrap()
}

fn names(rows: &[TreeRow]) -> Vec<&str> {
    rows.iter().map(|r| r.name.as_str()).collect()
}

fn owned(rows: Vec<&TreeRow>) -> Vec<TreeRow> {
    rows.into_iter().cloned().collect()
}

fn split(reply: &Envelope<horizon_perf::query::Row>) -> Vec<(Option<String>, f64)> {
    split_rows(reply)
        .iter()
        .map(|r| (r.key.clone(), r.value))
        .collect()
}

fn keyed(pairs: &[(&str, f64)]) -> Vec<(Option<String>, f64)> {
    pairs
        .iter()
        .map(|(k, v)| (Some(k.to_string()), *v))
        .collect()
}

#[test]
fn hot_ranks_functions_by_self_time_with_the_arithmetic_done_by_hand() {
    let dir = TempDir::new("hot");
    let reply = query(&["hot", &standard(&dir)]);
    assert_eq!(reply.unit, Some(Unit::MsPerTick));
    let sel = reply.selection.as_ref().unwrap();
    assert_eq!((sel.ticks, sel.samples), (Some(50), Some(120)));
    assert_eq!(sel.thread.as_deref(), Some("tick"));
    // Self samples 45, 35, 20, 10, 10 at 0.2 ms/tick each; ties by full name.
    assert_eq!(
        hot_ids(&reply),
        [LEVEL_ID, UPDATE_ID, TRACK_ID, SERVER_TICK_ID, TEAM_ID]
    );
    let rows = hot_rows(&reply);
    let level = rows[0];
    assert_eq!(level.name, "Waypoints::tick_level");
    assert_eq!(level.full, LEVEL);
    assert_eq!(
        (level.file.as_deref(), level.line),
        (Some("file.rs"), Some(13))
    );
    assert_eq!((level.self_ms, level.total), (9.0, 18.0));
    assert_eq!((level.self_pct, level.total_pct), (37.5, 75.0));
    assert_eq!(level.samples, 90);
    let update = hot_row(&rows, UPDATE_ID);
    assert_eq!(
        (update.self_ms, update.total, update.total_pct),
        (7.0, 7.0, 29.167)
    );
    let server = hot_row(&rows, SERVER_TICK_ID);
    assert_eq!(
        (server.self_ms, server.total, server.samples),
        (2.0, 24.0, 120)
    );
}

#[test]
fn hot_by_total_ranks_inclusive_time_and_match_filters_names() {
    let dir = TempDir::new("hot-total");
    let file = standard(&dir);
    let top = query(&["hot", "--by", "total", "--limit", "4", &file]);
    // The root and the server tick are both 24.0; the full name breaks the tie.
    assert_eq!(hot_ids(&top), [SERVER_TICK_ID, MAIN_ID, LAN_ID, LEVEL_ID]);
    let matched = query(&["hot", "--match", "WAYPOINTS::", &file]);
    assert_eq!(hot_ids(&matched), [LEVEL_ID, UPDATE_ID, TEAM_ID]);
}

#[test]
fn callers_are_the_known_caller_set_merged_over_both_occurrences() {
    let dir = TempDir::new("callers");
    let reply = query(&["callers", LEVEL_ID, "--depth", "3", &standard(&dir)]);
    let rows = owned(common::tree_rows(&reply));
    // tick_level runs under tick_lan (80 samples) and under tick_blocks (10).
    assert_eq!(names(&rows), ["Game::tick_lan", "Game::tick_blocks"]);
    assert_eq!(
        rows.iter()
            .map(|r| (r.total, r.samples))
            .collect::<Vec<_>>(),
        [(16.0, 80), (2.0, 10)]
    );
    assert_eq!(rows[0].pct, 66.667);
    assert_eq!(
        (rows[0].id.as_str(), rows[1].id.as_str()),
        (LAN_ID, BLOCKS_ID)
    );
    for (row, total) in rows.iter().zip([16.0, 2.0]) {
        let [server] = row.children.as_slice() else {
            panic!("{row:?}")
        };
        assert_eq!((server.id.as_str(), server.total), (SERVER_TICK_ID, total));
        let [main] = server.children.as_slice() else {
            panic!("{server:?}")
        };
        assert_eq!((main.id.as_str(), main.total), (MAIN_ID, total));
        assert!(main.children.is_empty());
    }
    assert_eq!(rows[0].self_ms, None);
}

#[test]
fn callers_stop_at_the_requested_depth() {
    let dir = TempDir::new("callers-depth");
    let reply = query(&["callers", UPDATE_ID, "--depth", "1", &standard(&dir)]);
    let rows = owned(tree_rows(&reply));
    // update_or_create: 30 samples under the lan's tick_level, 5 under the blocks' one; one caller function.
    assert_eq!(names(&rows), ["Waypoints::tick_level"]);
    assert_eq!((rows[0].total, rows[0].samples), (7.0, 35));
    assert!(rows[0].children.is_empty());
}

#[test]
fn callees_are_the_known_children_with_the_functions_own_time_last() {
    let dir = TempDir::new("callees");
    let reply = query(&["callees", LEVEL_ID, &standard(&dir)]);
    let rows = owned(tree_rows(&reply));
    assert_eq!(
        names(&rows),
        [
            "Waypoints::update_or_create",
            "waypoints::waypoint_team",
            "(self)"
        ]
    );
    assert_eq!(
        rows.iter()
            .map(|r| (r.total, r.samples))
            .collect::<Vec<_>>(),
        [(7.0, 35), (2.0, 10), (9.0, 45)]
    );
    assert_eq!(
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        [UPDATE_ID, TEAM_ID, LEVEL_ID]
    );
    let sum: f64 = rows.iter().map(|r| r.total).sum();
    assert_eq!(
        sum, 18.0,
        "children and self add up to the function's total"
    );
}

#[test]
fn tree_of_a_function_merges_its_occurrences_and_folds_small_children() {
    let dir = TempDir::new("tree-f");
    let file = standard(&dir);
    let reply = query(&["tree", LEVEL_ID, &file]);
    let rows = owned(tree_rows(&reply));
    let [level] = rows.as_slice() else {
        panic!("{rows:?}")
    };
    assert_eq!(level.id, LEVEL_ID);
    assert_eq!(
        (level.total, level.self_ms, level.pct, level.samples),
        (18.0, Some(9.0), 75.0, 90)
    );
    // update_or_create runs under two call paths, so its row names the function; team has one path.
    assert_eq!(
        level
            .children
            .iter()
            .map(|c| (c.id.as_str(), c.total))
            .collect::<Vec<_>>(),
        [(UPDATE_ID, 7.0), ("p:bkrz8s6r", 2.0)]
    );
    let folded = owned(tree_rows(&query(&[
        "tree",
        LEVEL_ID,
        "--min-pct",
        "10",
        &file,
    ])));
    assert_eq!(names(&folded[0].children), ["Waypoints::update_or_create"]);
    let hidden = folded[0].folded.as_ref().unwrap();
    assert_eq!((hidden.rows, hidden.total), (1, 2.0));
}

#[test]
fn tree_of_a_call_path_and_of_the_whole_thread() {
    let dir = TempDir::new("tree-p");
    let file = standard(&dir);
    let path = owned(tree_rows(&query(&["tree", "p:8hhyycyv", &file])));
    assert_eq!(path[0].id, "p:8hhyycyv");
    // Only the lan's tick_level: 80 samples, its update_or_create 30.
    assert_eq!((path[0].total, path[0].samples), (16.0, 80));
    assert_eq!(
        path[0]
            .children
            .iter()
            .map(|c| (c.id.as_str(), c.total))
            .collect::<Vec<_>>(),
        [("p:104ng1nz", 6.0), ("p:bkrz8s6r", 2.0)]
    );
    let root = owned(tree_rows(&query(&["tree", "--depth", "2", &file])));
    let [main] = root.as_slice() else {
        panic!("{root:?}")
    };
    assert_eq!((main.id.as_str(), main.total), ("p:s66b9tzf", 24.0));
    let [server] = main.children.as_slice() else {
        panic!("{main:?}")
    };
    assert_eq!(server.id, "p:79gfmx25");
    assert_eq!(
        server
            .children
            .iter()
            .map(|c| (c.id.as_str(), c.total))
            .collect::<Vec<_>>(),
        [("p:wm3e6jmv", 20.0), (BLOCKS_ID_PATH, 2.0)]
    );
    assert!(
        server.children[0].children.is_empty(),
        "depth 2 stops there"
    );
}

const BLOCKS_ID_PATH: &str = "p:0nm0y52m";

#[test]
fn split_by_entity_sums_to_the_functions_total() {
    let dir = TempDir::new("split-entity");
    let file = standard(&dir);
    let reply = query(&["split", LEVEL_ID, "--by", "entity", &file]);
    // tick_level subtree: zombie 25 + 20 + 5 + 15 (nether), item 15 + 10.
    assert_eq!(split(&reply), keyed(&[("e:zombie", 13.0), ("e:item", 5.0)]));
    let rows = split_rows(&reply);
    assert_eq!(
        (rows[0].pct, rows[0].samples, rows[0].by),
        (72.222, 65, SampleBy::Entity)
    );
    let total: f64 = rows.iter().map(|r| r.value).sum();
    let hot = query(&["hot", &file]);
    assert_eq!(total, hot_row(&hot_rows(&hot), LEVEL_ID).total);
}

#[test]
fn split_by_dimension_chunk_and_span() {
    let dir = TempDir::new("split-ctx");
    let file = standard(&dir);
    assert_eq!(
        split(&query(&["split", LEVEL_ID, "--by", "dim", &file])),
        keyed(&[("d:overworld", 15.0), ("d:the_nether", 3.0)])
    );
    assert_eq!(
        split(&query(&["split", LEVEL_ID, "--by", "chunk", &file])),
        keyed(&[
            ("c:overworld/12,-4", 10.0),
            ("c:overworld/13,-4", 5.0),
            ("c:the_nether/-3,7", 3.0)
        ])
    );
    assert_eq!(
        split(&query(&["split", LEVEL_ID, "--by", "span", &file])),
        keyed(&[("s:tick_lan", 15.0), ("s:tick_blocks", 3.0)])
    );
    assert_eq!(
        split(&query(&["split", LEVEL_ID, "--by", "thread", &file])),
        keyed(&[("t:tick", 18.0)])
    );
}

#[test]
fn split_by_state_sums_to_the_node_total_of_the_wall_samples() {
    let dir = TempDir::new("split-state");
    let file = standard(&dir);
    let reply = query(&[
        "split", LEVEL_ID, "--by", "state", "--select", "wall", &file,
    ]);
    // 12 running and 8 waiting samples of 10 ms over 50 ticks.
    assert_eq!(split(&reply), keyed(&[("running", 2.4), ("waiting", 1.6)]));
    let sum: f64 = split_rows(&reply).iter().map(|r| r.value).sum();
    let wall = query(&["hot", "--select", "wall", "--by", "total", &file]);
    assert_eq!(sum, hot_row(&hot_rows(&wall), LEVEL_ID).total);
    assert_eq!(sum, 4.0);
}

#[test]
fn split_by_slow_separates_the_slow_ticks_samples() {
    let dir = TempDir::new("split-slow");
    let reply = query(&["split", UPDATE_ID, "--by", "slow", &standard(&dir)]);
    // 10 of update_or_create's 35 samples are in slow ticks.
    assert_eq!(split(&reply), keyed(&[("normal", 5.0), ("slow", 2.0)]));
}

#[test]
fn split_narrows_to_a_span_or_a_context_value() {
    let dir = TempDir::new("split-scope");
    let file = standard(&dir);
    let by_span = query(&["split", "s:tick_blocks", "--by", "entity", &file]);
    assert_eq!(split(&by_span), keyed(&[("e:zombie", 3.0)]));
    let by_entity = query(&["split", "e:item", "--by", "dim", &file]);
    assert_eq!(split(&by_entity), keyed(&[("d:overworld", 5.0)]));
    let bad = query(&["split", "g:terrain", "--by", "dim", &file]);
    assert!(!bad.ok);
}

#[test]
fn group_divides_the_whole_selection_with_a_row_for_samples_without_context() {
    let dir = TempDir::new("group");
    let reply = query(&["group", "--by", "entity", &standard(&dir)]);
    let rows = split_rows(&reply);
    // 65 zombie, 25 item and 30 without an entity (the server tick and track_entities) of 120.
    assert_eq!(
        rows.iter()
            .map(|r| (r.key.as_deref(), r.label.as_str(), r.value))
            .collect::<Vec<_>>(),
        [
            (Some("e:zombie"), "zombie", 13.0),
            (None, "(none)", 6.0),
            (Some("e:item"), "item", 5.0)
        ]
    );
    assert_eq!(rows[1].pct, 25.0);
    let total: f64 = rows.iter().map(|r| r.value).sum();
    assert_eq!(total, 24.0);
}

#[test]
fn group_by_mod_charges_each_sample_to_the_innermost_mod_frame() {
    use horizon_perf_format::Field;
    use horizon_perf_format::v1::{Mod, ModKind};
    let dir = TempDir::new("group-mod");
    let mut b = Builder::new(50, &[25, 25]);
    b.func_in("horizon_client::tick", "horizon_client");
    b.func_in("my_mod::on_tick", "my_mod");
    b.func_in("horizon_world::helper", "horizon_world");
    b.func_in("other_mod::run", "other_mod");
    let root = b.node(None, "horizon_client::tick");
    let hook = b.node(Some(root), "my_mod::on_tick");
    let helper = b.node(Some(hook), "horizon_world::helper");
    let other = b.node(Some(root), "other_mod::run");
    b.weight(root, 10, W::default());
    b.weight(hook, 20, W::default());
    b.weight(helper, 5, W::default());
    b.weight(other, 15, W::default());
    let mut fields = b.fields();
    let Field::Start(start) = &mut fields[0] else {
        panic!("a capture opens with its start");
    };
    start.mods = [("my-mod", "my-mod"), ("other", "other_mod")]
        .map(|(id, crate_name)| Mod {
            id: id.into(),
            kind: ModKind::Native as i32,
            crate_name: crate_name.into(),
            ..Default::default()
        })
        .to_vec();
    let file = save(&dir, "mods.hzperf", &fields);
    // 10 ms samples over 50 ticks: a sample is 0.2 ms/tick; the helper runs under my_mod's hook.
    let reply = query(&["group", "--by", "mod", &file]);
    assert_eq!(
        split_rows(&reply)
            .iter()
            .map(|r| (r.key.as_deref(), r.value))
            .collect::<Vec<_>>(),
        [(Some("my-mod"), 5.0), (Some("other"), 3.0), (None, 2.0)]
    );
}

#[test]
fn a_time_window_counts_the_blocks_that_start_inside_it() {
    let dir = TempDir::new("window");
    let file = standard(&dir);
    // Block 1 (t0 = 5, 25 ticks) holds 35 of tick_level's 90 samples.
    let late = query(&["hot", "--by", "total", "--from", "5", &file]);
    assert_eq!(late.selection.as_ref().unwrap().ticks, Some(25));
    assert_eq!(late.selection.as_ref().unwrap().from, Some(5.0));
    let late_level = hot_row(&hot_rows(&late), LEVEL_ID).total;
    assert_eq!(late_level, 14.0);
    let early = query(&["hot", "--by", "total", "--to", "5", &file]);
    let early_level = hot_row(&hot_rows(&early), LEVEL_ID).total;
    assert_eq!(early_level, 22.0);
    assert_eq!((late_level * 25.0 + early_level * 25.0) / 50.0, 18.0);
}

#[test]
fn slow_keeps_the_slow_ticks_samples_per_slow_tick() {
    let dir = TempDir::new("slow");
    let file = standard(&dir);
    let reply = query(&["hot", "--slow", &file]);
    assert_eq!(reply.unit, Some(Unit::MsPerSlowTick));
    let sel = reply.selection.as_ref().unwrap();
    assert_eq!(
        (sel.ticks, sel.samples, sel.slow),
        (Some(5), Some(10), true)
    );
    // 10 samples of 10 ms over 5 slow ticks.
    let rows = hot_rows(&reply);
    assert_eq!(hot_ids(&reply), [UPDATE_ID]);
    assert_eq!(rows[0].self_ms, 20.0);
}

#[test]
fn where_ands_keys_and_ors_repeated_values() {
    let dir = TempDir::new("where");
    let file = standard(&dir);
    let nether = query(&[
        "hot",
        "--where",
        "entity=zombie",
        "--where",
        "dim=the_nether",
        &file,
    ]);
    assert_eq!(hot_ids(&nether), [UPDATE_ID]);
    assert_eq!(hot_rows(&nether)[0].self_ms, 3.0);
    let either = query(&["hot", "--where", "entity=zombie,entity=item", &file]);
    let rows = hot_rows(&either);
    assert_eq!(hot_row(&rows, UPDATE_ID).self_ms, 7.0);
    assert!(
        rows.iter().all(|r| r.id != TRACK_ID),
        "track_entities has no entity"
    );
    let chunk = query(&["hot", "--where", "chunk=c:overworld/12,-4", &file]);
    // 25 + 5 tick_level samples and 20 of update_or_create.
    let rows = hot_rows(&chunk);
    assert_eq!(hot_row(&rows, LEVEL_ID).self_ms, 6.0);
    assert_eq!(hot_row(&rows, UPDATE_ID).self_ms, 4.0);
}

#[test]
fn frames_with_three_captures_are_three_columns_equal_to_the_single_runs() {
    let dir = TempDir::new("frames");
    let files: Vec<String> = [(1, 50), (2, 50), (3, 100)]
        .iter()
        .enumerate()
        .map(|(i, (scale, ticks))| {
            save(
                &dir,
                &format!("run{i}.hzperf"),
                &waypoints(*scale, *ticks).fields(),
            )
        })
        .collect();
    let mut args = vec!["frames", "Waypoints::tick_level", UPDATE_ID];
    args.extend(files.iter().map(String::as_str));
    let all = query(&args);
    let rows = frame_rows(&all);
    assert_eq!(rows.len(), 2);
    // 90, 180 and 270 inclusive samples of 10 ms over 50, 50 and 100 ticks.
    assert_eq!(rows[0].id, LEVEL_ID);
    assert_eq!(rows[0].values, [Some(18.0), Some(36.0), Some(27.0)]);
    assert_eq!(rows[0].self_ms, [Some(9.0), Some(18.0), Some(13.5)]);
    assert_eq!(rows[0].samples, [Some(90), Some(180), Some(270)]);
    assert_eq!(all.captures.len(), 3);
    for (i, file) in files.iter().enumerate() {
        let single = query(&["frames", "Waypoints::tick_level", UPDATE_ID, file]);
        let one = frame_rows(&single);
        for (wide, narrow) in rows.iter().zip(&one) {
            assert_eq!(wide.id, narrow.id);
            assert_eq!(wide.values[i], narrow.values[0]);
            assert_eq!(wide.self_ms[i], narrow.self_ms[0]);
            assert_eq!(wide.samples[i], narrow.samples[0]);
        }
    }
}

#[test]
fn frames_warn_when_the_captures_sampled_different_threads() {
    let dir = TempDir::new("frames-threads");
    let same = save(&dir, "a.hzperf", &waypoints(1, 50).fields());
    let twin = save(&dir, "b.hzperf", &waypoints(2, 50).fields());
    let quiet = query(&["frames", UPDATE_ID, &same, &twin]);
    assert!(quiet.warnings.unwrap_or_default().is_empty());
    let mut b = waypoints(1, 50);
    b.thread = "render".into();
    let other = save(&dir, "c.hzperf", &b.fields());
    let mixed = query(&["frames", UPDATE_ID, &same, &other]);
    let warnings = mixed.warnings.unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w.contains("different threads")),
        "{warnings:?}"
    );
}

#[test]
fn frames_take_ids_regexes_and_report_unknown_or_ambiguous_names() {
    let dir = TempDir::new("frames-names");
    let file = standard(&dir);
    let by_regex = query(&["frames", "/waypoints::/", &file]);
    let ids: Vec<&str> = frame_rows(&by_regex)
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(ids, [LEVEL_ID, UPDATE_ID, TEAM_ID]);
    let unknown = query(&["frames", "tick_levl", &file]);
    assert!(!unknown.ok);
    let near = query(&["frames", "tick_lev", &file]);
    assert_eq!(near.candidates, [Candidate::new(LEVEL_ID, LEVEL)]);
    let mut b = Builder::new(10, &[10]);
    let main = b.node(None, "a::main");
    let first = b.node(Some(main), "left::Foo::new");
    let second = b.node(Some(main), "right::Foo::new");
    b.weight(first, 1, W::default());
    b.weight(second, 1, W::default());
    let dup = save(&dir, "dup.hzperf", &b.fields());
    let ambiguous = query(&["frames", "Foo::new", &dup]);
    assert!(!ambiguous.ok);
    assert_eq!(ambiguous.candidates.len(), 2);
    assert!(ambiguous.error.unwrap().contains("names 2 functions"));
}

#[test]
fn monomorphizations_and_hashes_normalize_to_one_function_with_a_known_id() {
    let dir = TempDir::new("normalize");
    let mut b = Builder::new(10, &[10]);
    let main = b.node(None, "a::main");
    let one = b.node(
        Some(main),
        "<horizon_world::level::Sim<horizon_world::level::NoHooks>>::set_block_limit",
    );
    let two = b.node(
        Some(main),
        "<horizon_world::level::Sim<…>>::set_block_limit::h0123456789abcdef",
    );
    b.weight(one, 3, W::default());
    b.weight(two, 4, W::default());
    let reply = query(&["hot", &save(&dir, "n.hzperf", &b.fields())]);
    let rows = hot_rows(&reply);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "f:m8r50yj8");
    assert_eq!(rows[0].name, "Sim::set_block_limit");
    assert_eq!(rows[0].full, "horizon_world::level::Sim::set_block_limit");
    // 7 samples of 10 ms over 10 ticks.
    assert_eq!((rows[0].self_ms, rows[0].samples), (7.0, 7));
}

#[test]
fn ids_do_not_depend_on_the_order_of_the_string_and_function_tables() {
    let dir = TempDir::new("ids");
    let plain = save(&dir, "a.hzperf", &waypoints(1, 50).fields());
    let mut shifted = Builder::new(50, &[25, 25]);
    shifted.text("unrelated");
    shifted.func("zzz::unused");
    shifted.func("aaa::also_unused");
    let shifted = save(&dir, "b.hzperf", &waypoints_in(shifted, 1).fields());
    assert_eq!(
        hot_ids(&query(&["hot", &plain])),
        hot_ids(&query(&["hot", &shifted]))
    );
    let tree =
        |file: &str| -> Vec<TreeRow> { owned(tree_rows(&query(&["tree", "--depth", "9", file]))) };
    assert_eq!(tree(&plain), tree(&shifted));
    assert_eq!(
        tree_rows(&query(&["tree", "p:8hhyycyv", &shifted]))[0].total,
        16.0
    );
}

#[test]
fn recursion_counts_the_outermost_call_once() {
    let dir = TempDir::new("recursion");
    let mut b = Builder::new(10, &[10]);
    let main = b.node(None, "a::main");
    let outer = b.node(Some(main), "a::walk");
    let inner = b.node(Some(outer), "a::walk");
    let leaf = b.node(Some(inner), "a::leaf");
    b.weight(outer, 2, W::default());
    b.weight(inner, 3, W::default());
    b.weight(leaf, 5, W::default());
    let file = save(&dir, "r.hzperf", &b.fields());
    let hot = query(&["hot", "--by", "total", &file]);
    let rows = hot_rows(&hot);
    let walk = rows.iter().find(|r| r.name == "a::walk").unwrap();
    // 10 samples of 10 ms over 10 ticks; the inner call is inside the outer one, not added to it.
    assert_eq!(
        (walk.self_ms, walk.total, walk.total_pct),
        (5.0, 10.0, 100.0)
    );
    let walk_id = walk.id.clone();
    let callers = owned(tree_rows(&query(&["callers", &walk_id, &file])));
    assert_eq!(names(&callers), ["a::main"]);
    assert_eq!(callers[0].total, 10.0);
}

#[test]
fn allocation_samples_are_bytes_per_tick() {
    let dir = TempDir::new("alloc");
    let mut b = Builder::new(10, &[10]);
    let main = b.node(None, "a::main");
    let grow = b.node(Some(main), "a::grow");
    b.weight(grow, 1, W::default());
    b.alloc_weight(grow, 5);
    let file = save(&dir, "alloc.hzperf", &b.fields());
    let reply = query(&["hot", "--select", "alloc", &file]);
    // 5 samples of 64 bytes over 10 ticks.
    assert_eq!(reply.unit, Some(Unit::BytesPerTick));
    assert_eq!(hot_rows(&reply)[0].self_ms, 32.0);
}

#[test]
fn a_selection_the_capture_cannot_answer_says_why() {
    let dir = TempDir::new("errors");
    let file = standard(&dir);
    let no_wall = query(&["hot", "--select", "alloc", &file]);
    assert_eq!(
        no_wall.error.as_deref(),
        Some("samples.hzperf has no alloc samples")
    );
    let live = query(&["hot", "--select", "live", &file]);
    assert!(live.error.unwrap().contains("no live-allocation samples"));
    let thread = query(&["hot", "--thread", "render", &file]);
    assert!(thread.error.unwrap().contains("sampled threads: tick"));
    let unknown = query(&["callers", "f:00000000", &file]);
    assert!(!unknown.ok);
    let two = query(&["hot", &file, &file]);
    assert!(two.error.unwrap().contains("takes one capture"));
    let missing = query(&["split", &file]);
    assert!(missing.error.unwrap().contains("split needs --by"));
}

fn run(args: &[&str]) -> Option<i32> {
    Command::new(env!("CARGO_BIN_EXE_horizon-perf"))
        .args(args)
        .output()
        .unwrap()
        .status
        .code()
}

#[test]
fn by_is_checked_against_the_vocabulary_of_its_op() {
    let dir = TempDir::new("by");
    let file = standard(&dir);
    assert_eq!(run(&["query", "hot", "--by", "total", &file]), Some(0));
    assert_eq!(run(&["query", "hot", "--by", "entity", &file]), Some(2));
    assert_eq!(run(&["query", "group", "--by", "state", &file]), Some(2));
    assert_eq!(run(&["query", "split", "--by", "state", &file]), Some(0));
    assert_eq!(run(&["query", "world", "--by", "total", &file]), Some(2));
}

#[test]
fn replies_survive_a_trip_through_the_row_types() {
    let dir = TempDir::new("roundtrip");
    let file = standard(&dir);
    for args in [
        vec!["hot", file.as_str()],
        vec!["tree", LEVEL_ID, file.as_str()],
        vec!["callers", LEVEL_ID, file.as_str()],
        vec!["callees", LEVEL_ID, file.as_str()],
        vec!["split", LEVEL_ID, "--by", "entity", file.as_str()],
        vec!["group", "--by", "dim", file.as_str()],
        vec!["frames", LEVEL_ID, file.as_str()],
    ] {
        let reply = query(&args);
        assert!(reply.ok, "{args:?}: {reply:?}");
        let text = serde_json::to_string(&reply).unwrap();
        let back: Envelope<horizon_perf::query::Row> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, reply, "{args:?}");
        assert!(!reply.rows().is_empty());
    }
}

#[test]
fn split_rows_keep_their_wire_names() {
    let dir = TempDir::new("wire");
    let reply = query(&["split", LEVEL_ID, "--by", "dimension", &standard(&dir)]);
    let text = serde_json::to_string(&reply).unwrap();
    assert!(text.contains(r#""by":"dim""#), "{text}");
    let row: &SplitRow = split_rows(&reply)[0];
    assert_eq!(row.by, SampleBy::Dim);
}

/// The standard tree on a 50-tick thread, plus a 10-tick worker with 25 samples of 10 ms.
fn two_threads(dir: &TempDir) -> String {
    let mut b = waypoints(1, 50);
    b.extra_threads.push(("worker".into(), 10, 0));
    let root = b.root_on(1, "worker::run");
    let job = b.node(Some(root), "worker::job");
    let on_worker = W {
        thread: 1,
        ..W::default()
    };
    b.weight(root, 5, on_worker);
    b.weight(job, 20, on_worker);
    save(dir, "threads.hzperf", &b.fields())
}

#[test]
fn split_by_thread_id_divides_by_the_named_threads_own_ticks() {
    let dir = TempDir::new("split-thread-id");
    let file = two_threads(&dir);
    let reply = query(&["split", "t:worker", "--by", "thread", &file]);
    // 25 samples of 10 ms over the worker's 10 ticks, not the tick thread's 50.
    assert_eq!(split(&reply), keyed(&[("t:worker", 25.0)]));
    let sel = reply.selection.as_ref().unwrap();
    assert_eq!(sel.thread.as_deref(), Some("worker"));
    assert_eq!((sel.ticks, sel.samples), (Some(10), Some(25)));
    let spans = query(&["split", "t:worker", "--by", "span", &file]);
    assert_eq!(split(&spans), vec![(None, 25.0)]);
    let tick = query(&["split", "t:tick", "--by", "thread", &file]);
    assert_eq!(split(&tick), keyed(&[("t:tick", 24.0)]));
    assert_eq!(tick.selection.as_ref().unwrap().ticks, Some(50));
    let missing = query(&["split", "t:nobody", "--by", "thread", &file]);
    assert!(!missing.ok);
}

#[test]
fn thread_filter_and_group_by_thread_use_the_chosen_threads_ticks() {
    let dir = TempDir::new("group-thread");
    let file = two_threads(&dir);
    let hot = query(&["hot", "--thread", "worker", &file]);
    assert_eq!(hot.selection.as_ref().unwrap().ticks, Some(10));
    let rows = hot_rows(&hot);
    assert_eq!(rows[0].full, "worker::job");
    assert_eq!(rows[0].self_ms, 20.0);
    let group = query(&["group", "--by", "thread", &file]);
    // 1200 and 250 ms over the tick thread's 50 ticks, with a warning saying so.
    assert_eq!(split(&group), keyed(&[("t:tick", 24.0), ("t:worker", 5.0)]));
    let warnings = group.warnings.unwrap_or_default();
    assert!(warnings.iter().any(|w| w.contains("tick")), "{warnings:?}");
}
